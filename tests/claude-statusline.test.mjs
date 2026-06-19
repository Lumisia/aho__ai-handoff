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

test('does not record status-line input without a usable five-hour rate limit', () => withRoot(() => {
  assert.equal(recordClaudeRateLimit({ session_id: 's1' }, { now: 1 }), false);
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 1, freshnessMs: 100 }), null);
}));

test('rejects stale samples and never crosses session ids', () => withRoot(() => {
  recordClaudeRateLimit({
    session_id: 's1',
    rate_limits: { five_hour: { used_percentage: 50, resets_at: 9 } },
  }, { now: 1_000 });

  assert.equal(readClaudeRateLimit({ sessionId: 's2', now: 1_001, freshnessMs: 1_000 }), null);
  assert.equal(readClaudeRateLimit({ sessionId: 's1', now: 2_001, freshnessMs: 1_000 }), null);
}));
