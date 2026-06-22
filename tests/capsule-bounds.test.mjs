import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildCapsule, validateCapsule } from '../core/capsule/create.mjs';

function cap(task, mutate) {
  const c = buildCapsule({
    taskId: 't-x-aaaaaaaaaaaa', now: '2026-06-19T00:00:00.000Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: 'fp', git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task,
  });
  if (mutate) mutate(c);
  return c;
}

test('a normal small capsule passes the bounds', () => {
  const r = validateCapsule(cap({ goal: 'ship it', next_actions: ['a', 'b'] }));
  assert.equal(r.valid, true, r.errors.join('; '));
});

test('a goal longer than maxLength is rejected', () => {
  const r = validateCapsule(cap({ goal: 'x'.repeat(5000), next_actions: [] }));
  assert.equal(r.valid, false);
  assert.ok(r.errors.some((e) => /maxLength/.test(e)));
});

test('too many list items is rejected', () => {
  const r = validateCapsule(cap({ goal: 'g', next_actions: Array(101).fill('a') }));
  assert.equal(r.valid, false);
  assert.ok(r.errors.some((e) => /maxItems/.test(e)));
});

test('an over-long list item is rejected', () => {
  const r = validateCapsule(cap({ goal: 'g', next_actions: ['y'.repeat(3000)] }));
  assert.equal(r.valid, false);
  assert.ok(r.errors.some((e) => /maxLength/.test(e)));
});

test('an unsupported schema_version is rejected', () => {
  const r = validateCapsule(cap({ goal: 'g', next_actions: [] }, (c) => { c.schema_version = '2.0.0'; }));
  assert.equal(r.valid, false);
  assert.ok(r.errors.some((e) => /enum/.test(e)));
});

test('a capsule over the total byte ceiling is rejected even within per-field bounds', () => {
  const big = 'z'.repeat(2048);
  const list = Array(100).fill(big);
  const r = validateCapsule(cap({
    goal: 'g', next_actions: list, completed: list, open_issues: list, changed_files: list,
  }));
  assert.equal(r.valid, false);
  assert.ok(r.errors.some((e) => /exceeds 131072 bytes/.test(e)));
});
