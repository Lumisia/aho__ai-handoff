import { test } from 'node:test';
import assert from 'node:assert/strict';
import { slugify, instanceKey, deriveTaskId } from '../core/lib/taskid.mjs';

test('slugify lowercases and dasherizes', () => {
  assert.equal(slugify('Fix OAuth Redirect!'), 'fix-oauth-redirect');
  assert.equal(slugify(''), 'task');
});

test('instanceKey prefers explicit key, else session', () => {
  assert.equal(instanceKey({ explicitTaskKey: 'ISSUE-12' }), 'key:issue-12');
  assert.equal(instanceKey({ agent: 'codex', sessionId: 's1' }), 'session:codex:s1');
});

test('deriveTaskId is deterministic and instanceKey-sensitive', () => {
  const a = deriveTaskId({ projectFingerprint: 'fp', instanceKey: 'session:codex:s1', goalSlug: 'fix auth' });
  const b = deriveTaskId({ projectFingerprint: 'fp', instanceKey: 'session:codex:s1', goalSlug: 'fix auth' });
  const c = deriveTaskId({ projectFingerprint: 'fp', instanceKey: 'session:codex:s2', goalSlug: 'fix auth' });
  assert.equal(a, b);
  assert.notEqual(a, c);
  assert.match(a, /^t-fix-auth-[a-z2-7]{12}$/);
});
