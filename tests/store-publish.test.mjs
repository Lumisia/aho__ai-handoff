import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { publishCapsule, findPendingCapsule, readState, writeState } from '../core/capsule/store.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-store-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

const capsule = {
  schema_version: '1.0.0', capsule_id: 'c1', task_id: 't-x-aaaaaaaaaaaa',
  created_at: '2026-06-19T00:00:00Z', source: { agent: 'codex' }, target: { agent: 'claude-code' },
  checkpoint: { status: 'in_progress' }, task: { goal: 'g' }, integrity: { payload_sha256: 'sha256:x' },
};

test('publishCapsule writes capsule, sha, and AVAILABLE state', () => withRoot(() => {
  const { capsulePath, statePath } = publishCapsule('fp', capsule, { status: 'AVAILABLE', now: 1 });
  assert.ok(readFileSync(capsulePath, 'utf8').includes('t-x-aaaaaaaaaaaa'));
  assert.equal(readState(statePath).status, 'AVAILABLE');
}));

test('findPendingCapsule returns the published capsule', () => withRoot(() => {
  publishCapsule('fp', capsule, { status: 'AVAILABLE', now: 1 });
  const found = findPendingCapsule('fp');
  assert.equal(found.taskId, 't-x-aaaaaaaaaaaa');
  assert.equal(found.capsule.capsule_id, 'c1');
}));

test('findPendingCapsule ignores consumed capsules', () => withRoot(() => {
  const { statePath } = publishCapsule('fp', capsule, { status: 'AVAILABLE', now: 1 });
  writeState(statePath, { status: 'CONSUMED', task_id: capsule.task_id });
  assert.equal(findPendingCapsule('fp'), null);
}));
