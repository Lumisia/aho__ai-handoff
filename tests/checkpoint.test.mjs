import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildCheckpointCapsule } from '../core/capsule/checkpoint.mjs';
import { validateCapsule, capsulePayloadHash } from '../core/capsule/create.mjs';

test('builds a valid rich capsule from a model sentinel', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-cp-'));
  const sentinel = { goal: 'fix oauth redirect', next_actions: ['run e2e'], completed: ['found nonce reuse'], status: 'in_progress' };
  const { capsule } = buildCheckpointCapsule({ sentinel, cwd, agent: 'claude-code', sessionId: 's1', now: 1000 });
  assert.equal(capsule.task.goal, 'fix oauth redirect');
  assert.deepEqual(capsule.task.next_actions, ['run e2e']);
  assert.equal(capsule.trigger.type, 'manual_checkpoint');
  assert.deepEqual(validateCapsule(capsule), { valid: true, errors: [] });
  assert.equal('sha256:' + capsulePayloadHash(capsule), capsule.integrity.payload_sha256);
});

test('redacts secrets in model-provided fields', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-cp-'));
  const sentinel = { goal: 'use key sk-abcdef012345678901234567890', next_actions: [] };
  const { capsule } = buildCheckpointCapsule({ sentinel, cwd, agent: 'codex', sessionId: 's', now: 1 });
  assert.match(capsule.task.goal, /\[REDACTED\]/);
  assert.ok(capsule.security.redactions_applied >= 1);
});
