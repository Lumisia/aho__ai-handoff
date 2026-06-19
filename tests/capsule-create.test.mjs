import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildCapsule, validateCapsule, capsulePayloadHash } from '../core/capsule/create.mjs';

const input = {
  taskId: 't-fix-auth-abcdef012345',
  source: { agent: 'codex' },
  target: { agent: 'claude-code' },
  checkpoint: { status: 'in_progress' },
  task: { goal: 'fix oauth redirect loop', next_actions: ['run e2e'] },
  now: '2026-06-19T10:30:00+09:00',
  capsuleId: 'fixed-id',
};

test('buildCapsule fills required metadata and integrity hash', () => {
  const c = buildCapsule(input);
  assert.equal(c.schema_version, '1.0.0');
  assert.equal(c.capsule_id, 'fixed-id');
  assert.equal(c.task_id, input.taskId);
  assert.equal(c.created_at, input.now);
  assert.match(c.integrity.payload_sha256, /^sha256:[0-9a-f]{64}$/);
});

test('integrity hash excludes the integrity field and is stable', () => {
  const c = buildCapsule(input);
  assert.equal('sha256:' + capsulePayloadHash(c), c.integrity.payload_sha256);
});

test('buildCapsule output passes schema validation', () => {
  assert.deepEqual(validateCapsule(buildCapsule(input)), { valid: true, errors: [] });
});

test('a capsule missing goal fails validation', () => {
  const c = buildCapsule(input);
  delete c.task.goal;
  assert.equal(validateCapsule(c).valid, false);
});

test('empty goals and same-agent handoffs fail semantic validation', () => {
  const emptyGoal = buildCapsule({ ...input, task: { goal: '   ' } });
  assert.equal(validateCapsule(emptyGoal).valid, false);
  const sameAgent = buildCapsule({ ...input, target: { agent: 'codex' } });
  assert.equal(validateCapsule(sameAgent).valid, false);
});
