import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildCapsule } from '../core/capsule/create.mjs';
import { publishCapsule, verifyStoredCapsule } from '../core/capsule/store.mjs';

function withRoot(fn) {
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-verify-'));
  try { return fn(); } finally {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  }
}

function capsule(fp = 'fp1') {
  return buildCapsule({
    taskId: 't-verify-aaaaaaaaaaaa', now: '2026-06-19T00:00:00.000Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_head: 'old-head' },
    checkpoint: { status: 'in_progress' }, task: { goal: 'verify me' },
  });
}

test('verifies schema, payload hash, external sha, project, and target', () => withRoot(() => {
  const p = publishCapsule('fp1', capsule());
  const result = verifyStoredCapsule('fp1', 't-verify-aaaaaaaaaaaa', {
    expectedAgent: 'claude-code', currentGitHead: 'new-head', now: Date.parse('2026-06-19T01:00:00Z'),
  });
  assert.equal(result.valid, true);
  assert.deepEqual(result.errors, []);
  assert.ok(result.warnings.includes('git-head-mismatch'));
  assert.equal(result.capsulePath, p.capsulePath);
}));

test('rejects a capsule for the wrong target agent', () => withRoot(() => {
  publishCapsule('fp1', capsule());
  const result = verifyStoredCapsule('fp1', 't-verify-aaaaaaaaaaaa', { expectedAgent: 'codex' });
  assert.equal(result.valid, false);
  assert.ok(result.errors.includes('target-agent-mismatch'));
}));

test('rejects tampered capsule bytes even when JSON remains valid', () => withRoot(() => {
  const p = publishCapsule('fp1', capsule());
  const value = JSON.parse(readFileSync(p.capsulePath, 'utf8'));
  value.task.goal = 'tampered';
  writeFileSync(p.capsulePath, JSON.stringify(value, null, 2) + '\n');
  const result = verifyStoredCapsule('fp1', value.task_id, { expectedAgent: 'claude-code' });
  assert.equal(result.valid, false);
  assert.ok(result.errors.includes('external-sha-mismatch'));
  assert.ok(result.errors.includes('payload-integrity-mismatch'));
}));
