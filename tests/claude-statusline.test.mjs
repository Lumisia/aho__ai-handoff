import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  recordClaudeRateLimit,
  readClaudeRateLimit,
} from '../core/sensors/claude-statusline.mjs';

function withRoot(fn) {
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-claude-rate-'));
  try { return fn(); } finally {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  }
}

test('records and reads a fresh Claude five-hour rate limit for the same session', () => withRoot(() => {
  const recorded = recordClaudeRateLimit({
    session_id: 'claude-s1',
    rate_limits: { five_hour: { used_percentage: 81.5, resets_at: 12345 } },
  }, { now: 1_000 });

  assert.equal(recorded, true);
  assert.deepEqual(readClaudeRateLimit({ sessionId: 'claude-s1', now: 1_500, freshnessMs: 1_000 }), {
    usedPercent: 81.5,
    windowMinutes: 300,
    resetsAt: 12345,
    source: 'claude-statusline',
    capturedAt: 1_000,
  });
}));

test('records and reads Claude seven-day rate limit when statusline provides it', () => withRoot(() => {
  const recorded = recordClaudeRateLimit({
    session_id: 'claude-s1',
    rate_limits: {
      five_hour: { used_percentage: 12 },
      seven_day: { used_percentage: 34, resets_at: 98765 },
    },
  }, { now: 1_000 });

  assert.equal(recorded, true);
  assert.deepEqual(readClaudeRateLimit({ sessionId: 'claude-s1', now: 1_500, freshnessMs: 1_000 })?.weekly, {
    usedPercent: 34,
    windowMinutes: 10_080,
    resetsAt: 98765,
  });
}));

test('does not record status-line input without a usable five-hour rate limit', () => withRoot(() => {
  assert.equal(recordClaudeRateLimit({ session_id: 's1' }, { now: 1 }), false);
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 1, freshnessMs: 100 }), null);
}));

test('rejects a sample older than the default freshness window', () => withRoot(() => {
  recordClaudeRateLimit({
    session_id: 's1',
    rate_limits: { five_hour: { used_percentage: 66, resets_at: 2_000 } },
  }, { now: 1_000 });
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 301_000 }), null);
}));

test('accepts an older sample when the caller explicitly widens freshness', () => withRoot(() => {
  recordClaudeRateLimit({
    session_id: 's1',
    rate_limits: { five_hour: { used_percentage: 66, resets_at: 2_000 } },
  }, { now: 1_000 });
  const reading = readClaudeRateLimit({ sessionId: 's1', now: 301_000, freshnessMs: 900_000 });
  assert.equal(reading?.usedPercent, 66);
}));

test('rejects a reading whose five-hour window has already reset', () => withRoot(() => {
  recordClaudeRateLimit({
    session_id: 's1',
    rate_limits: { five_hour: { used_percentage: 96, resets_at: 100 } },
  }, { now: 1_000 });
  // now (150000ms) is past resets_at (100s -> 100000ms): the 96% belongs to a
  // window that already reset, so it must not drive a trigger.
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 150_000 }), null);
}));

test('reads a sample across sessions (account-global) and drops it once stale', () => withRoot(() => {
  recordClaudeRateLimit({
    session_id: 's1',
    rate_limits: { five_hour: { used_percentage: 50, resets_at: 9 } },
  }, { now: 1_000 });

  // Claude hands the status line and the Stop hook different session ids, so a
  // reader on session s2 must still see s1's account-global reading.
  assert.equal(readClaudeRateLimit({ sessionId: 's2', now: 1_001, freshnessMs: 1_000 })?.usedPercent, 50);
  // ...but only while it is fresh.
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 2_001, freshnessMs: 1_000 }), null);
}));

test('picks the most recent valid sample when several sessions have written', () => withRoot(() => {
  recordClaudeRateLimit({ session_id: 'old', rate_limits: { five_hour: { used_percentage: 20, resets_at: 9_999 } } }, { now: 1_000 });
  recordClaudeRateLimit({ session_id: 'new', rate_limits: { five_hour: { used_percentage: 75, resets_at: 9_999 } } }, { now: 5_000 });
  assert.equal(readClaudeRateLimit({ sessionId: 'unrelated', now: 6_000, freshnessMs: 100_000 })?.usedPercent, 75);
}));
