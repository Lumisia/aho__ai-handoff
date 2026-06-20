import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildCheckpointCapsule } from '../core/capsule/checkpoint.mjs';
import {
  publishCapsule, claimCapsule, releaseClaim, findPendingCapsule, readState,
} from '../core/capsule/store.mjs';

function setup(session) {
  const data = mkdtempSync(join(tmpdir(), 'ah-deg-'));
  process.env.AI_HANDOFF_ROOT = data;
  const cwd = mkdtempSync(join(tmpdir(), 'ah-degcwd-'));
  const { capsule, fingerprint } = buildCheckpointCapsule({ sentinel: { goal: 'g' }, cwd, agent: 'codex', sessionId: session });
  return { capsule, fingerprint };
}

test('releasing a claimed degraded capsule restores DEGRADED_AVAILABLE, not AVAILABLE', () => {
  const { capsule, fingerprint } = setup('s1');
  publishCapsule(fingerprint, capsule, { status: 'DEGRADED_AVAILABLE' });
  const claim = claimCapsule(fingerprint, capsule.task_id);
  assert.ok(claim, 'claim succeeds');
  releaseClaim(claim);
  assert.equal(readState(claim.statePath).status, 'DEGRADED_AVAILABLE');
});

test('releasing a claimed available capsule still restores AVAILABLE', () => {
  const { capsule, fingerprint } = setup('s2');
  publishCapsule(fingerprint, capsule, { status: 'AVAILABLE' });
  const claim = claimCapsule(fingerprint, capsule.task_id);
  releaseClaim(claim);
  assert.equal(readState(claim.statePath).status, 'AVAILABLE');
});

test('lease-expiry recovery preserves degraded status', () => {
  const { capsule, fingerprint } = setup('s3');
  publishCapsule(fingerprint, capsule, { status: 'DEGRADED_AVAILABLE' });
  claimCapsule(fingerprint, capsule.task_id, { leaseMs: 1, now: 1000 });
  const pending = findPendingCapsule(fingerprint, { now: 1000 + 60000 });
  assert.ok(pending, 'expired claim is recovered as pending');
  assert.equal(pending.state.status, 'DEGRADED_AVAILABLE');
});

test('lease-expiry recovery of a normal capsule yields AVAILABLE', () => {
  const { capsule, fingerprint } = setup('s4');
  publishCapsule(fingerprint, capsule, { status: 'AVAILABLE' });
  claimCapsule(fingerprint, capsule.task_id, { leaseMs: 1, now: 1000 });
  const pending = findPendingCapsule(fingerprint, { now: 1000 + 60000 });
  assert.ok(pending);
  assert.equal(pending.state.status, 'AVAILABLE');
});
