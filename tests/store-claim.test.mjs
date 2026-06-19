import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  publishCapsule, findPendingCapsule, readState,
  claimCapsule, consumeCapsule, releaseClaim,
} from '../core/capsule/store.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-claim-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

const cap = {
  schema_version: '1.0.0', capsule_id: 'c1', task_id: 't-x-bbbbbbbbbbbb',
  created_at: 'z', source: { agent: 'codex' }, target: { agent: 'claude-code' },
  checkpoint: { status: 'in_progress' }, task: { goal: 'g' }, integrity: { payload_sha256: 'sha256:x' },
};

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
