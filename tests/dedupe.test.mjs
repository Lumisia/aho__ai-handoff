import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dedupeKey, hasSeen, markSeen } from '../core/lib/dedupe.mjs';

const parts = { source: 'codex', windowDuration: 300, resetsAt: 111, sessionId: 's1', projectFingerprint: 'fp', threshold: 80 };

test('dedupeKey is deterministic and order-insensitive', () => {
  const a = dedupeKey(parts);
  const b = dedupeKey({ threshold: 80, source: 'codex', sessionId: 's1', resetsAt: 111, projectFingerprint: 'fp', windowDuration: 300 });
  assert.equal(a, b);
  assert.match(a, /^[0-9a-f]{16}$/);
});

test('different parts give a different key', () => {
  assert.notEqual(dedupeKey(parts), dedupeKey({ ...parts, resetsAt: 222 }));
});

test('markSeen then hasSeen is true and is immutable', () => {
  const k = dedupeKey(parts);
  const s0 = {};
  const s1 = markSeen(s0, k, 5);
  assert.equal(hasSeen(s0, k), false);
  assert.equal(hasSeen(s1, k), true);
});
