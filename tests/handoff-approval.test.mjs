import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { saveApproval, findApproval } from '../core/capsule/approval.mjs';
import { createFromApproval, skipApproval, statusFor } from '../core/hooks/handoff.mjs';
import { findPendingCapsule } from '../core/capsule/store.mjs';
import { handoffDir } from '../core/lib/paths.mjs';
import { acquireLock, releaseLock } from '../core/lib/fsx.mjs';

function withRoot(fn) {
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-handoff-approval-'));
  try { return fn(); } finally {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  }
}

function seed(cwd, key = 'k1') {
  const fingerprint = projectFingerprint(cwd);
  saveApproval({
    fingerprint, key, now: 10,
    context: {
      cwd, agent: 'codex', sessionId: 's1', threshold: 80,
      reading: { usedPercent: 82, source: 'app-server', resetsAt: 123, windowMinutes: 300 },
    },
  });
  return fingerprint;
}

test('create resolves awaiting approval and publishes user-authored capsule', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const fp = seed(cwd);
  const result = createFromApproval({ cwd, sentinel: { goal: 'approved goal', next_actions: ['continue'] }, now: 20 });
  assert.equal(result.created, true);
  assert.equal(findApproval(fp), null);
  assert.equal(findPendingCapsule(fp, { now: 20 }).capsule.task.goal, 'approved goal');
}));

test('skip resolves awaiting approval without publishing', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const fp = seed(cwd);
  assert.equal(skipApproval({ cwd, now: 20 }).skipped, true);
  assert.equal(findApproval(fp), null);
  assert.equal(findPendingCapsule(fp), null);
}));

test('status reports awaiting-user independently from pending capsule', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  seed(cwd);
  assert.equal(statusFor(cwd).awaitingUser, true);
}));

test('create restores approval to AWAITING_USER when publish fails (retryable)', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const fp = seed(cwd);
  // Hold the publish lock so publishCapsule throws after the approval has been
  // moved to GENERATING — exercising the failure-recovery path.
  const blocker = acquireLock(join(handoffDir(fp), '.publish.lock'), {});
  assert.ok(blocker, 'publish lock acquired by test');
  assert.throws(() => createFromApproval({ cwd, sentinel: { goal: 'g', next_actions: ['x'] }, now: 20 }));
  releaseLock(blocker);
  // The approval is back to AWAITING_USER, so the user can retry; nothing was
  // published.
  const restored = findApproval(fp);
  assert.ok(restored, 'approval restored for retry');
  assert.equal(restored.status, 'AWAITING_USER');
  assert.equal(findPendingCapsule(fp), null);
}));
