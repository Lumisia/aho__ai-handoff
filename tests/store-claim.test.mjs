import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  publishCapsule, findPendingCapsule, readState,
  claimCapsule, consumeCapsule, releaseClaim,
} from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-claim-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

const cap = buildCapsule({
  capsuleId: 'c1', taskId: 't-x-bbbbbbbbbbbb', now: '2026-06-19T00:00:00Z',
  source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
  project: { fingerprint: 'fp' }, checkpoint: { status: 'in_progress' }, task: { goal: 'g' },
});

test('claim sets CLAIMED and blocks a second claim', () => withRoot(() => {
  publishCapsule('fp', cap, { now: 1 });
  const c = claimCapsule('fp', cap.task_id, { leaseMs: 1000, now: 1000 });
  assert.ok(c);
  assert.equal(readState(c.statePath).status, 'CLAIMED');
  assert.equal(claimCapsule('fp', cap.task_id, { leaseMs: 1000, now: 1100 }), null);
  consumeCapsule(c, { now: 1200 });
  assert.equal(readState(c.statePath).status, 'CONSUMED');
}));

test('releaseClaim returns capsule to AVAILABLE', () => withRoot(() => {
  publishCapsule('fp', cap, { now: 1 });
  const c = claimCapsule('fp', cap.task_id, { leaseMs: 1000, now: 1000 });
  releaseClaim(c);
  assert.equal(readState(c.statePath).status, 'AVAILABLE');
  assert.ok(findPendingCapsule('fp'));
}));

test('expired claim lease is recovered to AVAILABLE during pending lookup', () => withRoot(() => {
  publishCapsule('fp', cap, { now: 1 });
  const first = claimCapsule('fp', cap.task_id, { now: 10, leaseMs: 20 });
  assert.ok(first);
  const recovered = findPendingCapsule('fp', { now: 31 });
  assert.equal(recovered.state.status, 'AVAILABLE');
  assert.ok(claimCapsule('fp', cap.task_id, { now: 32 }));
}));

test('an expired claim cannot consume a capsule reclaimed by a new owner', () => withRoot(() => {
  publishCapsule('fp', cap, { now: 1 });
  const stale = claimCapsule('fp', cap.task_id, { now: 10, leaseMs: 20 });
  findPendingCapsule('fp', { now: 31 });
  const current = claimCapsule('fp', cap.task_id, { now: 32, leaseMs: 100 });
  assert.ok(current);
  assert.throws(() => consumeCapsule(stale, { now: 33 }), /stale claim/);
  assert.equal(readState(current.statePath).status, 'CLAIMED');
  consumeCapsule(current, { now: 34 });
}));
