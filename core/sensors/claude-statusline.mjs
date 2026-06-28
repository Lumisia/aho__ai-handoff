import { join } from 'node:path';
import { readFileSync, readdirSync } from 'node:fs';
import { claudeRateLimitDir } from '../lib/paths.mjs';
import { sha256Hex } from '../lib/hash.mjs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { appendSample } from './samples.mjs';
import { projectFingerprint } from '../lib/fingerprint.mjs';

function samplePath(sessionId) {
  if (!sessionId) return null;
  return join(claudeRateLimitDir(), `${sha256Hex(String(sessionId))}.json`);
}

export function recordClaudeRateLimit(input, { now = Date.now() } = {}) {
  const fiveHour = input?.rate_limits?.five_hour;
  const used = fiveHour?.used_percentage;
  const path = samplePath(input?.session_id);
  if (!path || typeof used !== 'number' || !Number.isFinite(used) || used < 0 || used > 100) return false;
  const sevenDay = input?.rate_limits?.seven_day;
  const weeklyUsed = sevenDay?.used_percentage;
  const weekly = typeof weeklyUsed === 'number' && Number.isFinite(weeklyUsed) && weeklyUsed >= 0 && weeklyUsed <= 100
    ? {
        used_percent: weeklyUsed,
        window_minutes: 10_080,
        resets_at: sevenDay.resets_at ?? null,
        captured_at: now,
      }
    : null;

  writeFileAtomic(path, JSON.stringify({
    session_id: input.session_id,
    used_percent: used,
    resets_at: fiveHour.resets_at ?? null,
    weekly,
    captured_at: now,
  }, null, 2) + '\n');
  const cwd = input.cwd || input.workspace?.current_dir;
  if (cwd) { try { appendSample(projectFingerprint(cwd), 'claude-code', { usedPercent: used, at: now }); } catch {} }
  return true;
}

function sampleIsUsable(sample, freshnessMs, now) {
  if (!sample || typeof sample.used_percent !== 'number') return false;
  // The setup command installs a short status-line refresh interval, so a stale
  // sample usually means the bridge is not running and must not drive a trigger.
  if (typeof sample.captured_at !== 'number' || now - sample.captured_at > freshnessMs) return false;
  // resets_at is unix SECONDS; now is ms. Past the reset boundary the percentage
  // belongs to a previous window, so it must not drive a trigger.
  if (typeof sample.resets_at === 'number' && now >= sample.resets_at * 1000) return false;
  return true;
}

// The five-hour limit is account-global, but Claude Code hands the status line
// and the Stop hook DIFFERENT session ids, so a sample keyed by the writer's
// session is usually invisible to the reader's session — which left the Stop
// hook reading nothing and never triggering. Pick the freshest still-valid
// sample across all sessions instead of requiring an exact session match.
export function readClaudeRateLimit({ sessionId, freshnessMs = 10_000, now = Date.now() } = {}) {
  let best = null;
  let files;
  try { files = readdirSync(claudeRateLimitDir()); } catch { files = []; }
  for (const file of files) {
    if (!file.endsWith('.json')) continue;
    let sample;
    try { sample = JSON.parse(readFileSync(join(claudeRateLimitDir(), file), 'utf8')); } catch { continue; }
    if (!sampleIsUsable(sample, freshnessMs, now)) continue;
    if (!best || sample.captured_at > best.captured_at) best = sample;
  }
  if (!best) return null;
  const result = {
    usedPercent: best.used_percent,
    windowMinutes: 300,
    resetsAt: best.resets_at,
    source: 'claude-statusline',
    capturedAt: best.captured_at,
  };
  if (sampleIsUsable(best.weekly, freshnessMs, now)) {
    result.weekly = {
      usedPercent: best.weekly.used_percent,
      windowMinutes: best.weekly.window_minutes ?? 10_080,
      resetsAt: best.weekly.resets_at,
    };
  }
  return result;
}
