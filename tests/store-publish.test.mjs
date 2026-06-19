import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { publishCapsule, findPendingCapsule, readState, writeState } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-store-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

const capsule = buildCapsule({
  capsuleId: 'c1', taskId: 't-x-aaaaaaaaaaaa', now: '2026-06-19T00:00:00Z',
  source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
  project: { fingerprint: 'fp' }, checkpoint: { status: 'in_progress' }, task: { goal: 'g' },
});

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

test('publishCapsule rejects a schema-invalid or integrity-invalid payload', () => withRoot(() => {
  assert.throws(() => publishCapsule('fp', { ...capsule, task: {} }), /invalid capsule/);
  assert.throws(() => publishCapsule('fp', { ...capsule, integrity: { payload_sha256: 'sha256:nope' } }), /invalid capsule/);
}));

test('publishCapsule never overwrites an existing task capsule', () => withRoot(() => {
  publishCapsule('fp', capsule, { now: 1 });
  const different = buildCapsule({
    capsuleId: 'c2', taskId: capsule.task_id, now: '2026-06-19T00:00:01Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
    project: { fingerprint: 'fp' }, checkpoint: { status: 'in_progress' }, task: { goal: 'different' },
  });
  assert.throws(() => publishCapsule('fp', different, { now: 2 }), /already published/);
}));

test('publishing a newer project capsule expires the previous pending capsule', () => withRoot(() => {
  const first = publishCapsule('fp', capsule, { now: 1 });
  const newer = buildCapsule({
    capsuleId: 'c2', taskId: 't-y-yyyyyyyyyyyy', now: '2026-06-19T00:00:01Z',
    source: { agent: 'claude-code' }, target: { agent: 'codex' }, trigger: { type: 'test' },
    project: { fingerprint: 'fp' }, checkpoint: { status: 'in_progress' }, task: { goal: 'newest' },
  });
  publishCapsule('fp', newer, { now: 2 });
  assert.equal(readState(first.statePath).status, 'EXPIRED');
  assert.equal(findPendingCapsule('fp').taskId, newer.task_id);
}));
