import { join } from 'node:path';
import { readFileSync } from 'node:fs';
import { claudeRateLimitDir } from '../lib/paths.mjs';
import { sha256Hex } from '../lib/hash.mjs';
import { writeFileAtomic } from '../lib/fsx.mjs';

function samplePath(sessionId) {
  if (!sessionId) return null;
  return join(claudeRateLimitDir(), `${sha256Hex(String(sessionId))}.json`);
}

export function recordClaudeRateLimit(input, { now = Date.now() } = {}) {
  const fiveHour = input?.rate_limits?.five_hour;
  const used = fiveHour?.used_percentage;
  const path = samplePath(input?.session_id);
  if (!path || typeof used !== 'number' || !Number.isFinite(used) || used < 0 || used > 100) return false;

  writeFileAtomic(path, JSON.stringify({
    session_id: input.session_id,
    used_percent: used,
    resets_at: fiveHour.resets_at ?? null,
    captured_at: now,
  }, null, 2) + '\n');
  return true;
}

export function readClaudeRateLimit({ sessionId, freshnessMs = 120_000, now = Date.now() } = {}) {
  const path = samplePath(sessionId);
  if (!path) return null;
  let sample;
  try { sample = JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
  if (sample.session_id !== sessionId) return null;
  if (typeof sample.captured_at !== 'number' || now - sample.captured_at > freshnessMs) return null;
  if (typeof sample.used_percent !== 'number') return null;
  return {
    usedPercent: sample.used_percent,
    windowMinutes: 300,
    resetsAt: sample.resets_at,
    source: 'claude-statusline',
    capturedAt: sample.captured_at,
  };
}
