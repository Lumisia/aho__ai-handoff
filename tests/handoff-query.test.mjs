import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { statusFor, previewFor, doctorFor } from '../core/hooks/handoff.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule, findPendingCapsule } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-hq-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function cap(fp) {
  return buildCapsule({
    capsuleId: 'c1', taskId: 't-x-dddddddddddd', now: '2026-06-19T00:00:00Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
    project: { fingerprint: fp }, checkpoint: { status: 'in_progress' },
    task: { goal: 'do X', next_actions: ['a'] },
  });
}

test('statusFor and previewFor report pending without consuming', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp), { now: 1 });
  const s = statusFor(cwd);
  assert.equal(s.pending, true);
  assert.equal(s.state, 'AVAILABLE');
  const p = previewFor(cwd);
  assert.equal(p.goal, 'do X');
  assert.ok(findPendingCapsule(fp));
}));

test('no pending → not pending', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  assert.equal(statusFor(cwd).pending, false);
  assert.equal(previewFor(cwd).pending, false);
}));

test('previewFor surfaces completed, open_issues and changed_files', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const c = buildCapsule({
    capsuleId: 'c2', taskId: 't-x-eeeeeeeeeeee', now: '2026-06-19T00:00:00Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' }, trigger: { type: 'test' },
    project: { fingerprint: fp }, checkpoint: { status: 'in_progress' },
    task: {
      goal: 'do X', next_actions: ['a'],
      completed: ['done thing'], open_issues: ['open thing'], changed_files: ['f.txt'],
    },
  });
  publishCapsule(fp, c, { now: 1 });
  const p = previewFor(cwd);
  assert.deepEqual(p.completed, ['done thing']);
  assert.deepEqual(p.open_issues, ['open thing']);
  assert.deepEqual(p.changed_files, ['f.txt']);
}));

test('doctor diagnoses pending capsule integrity without consuming it', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp), { now: 1 });
  const result = doctorFor(cwd, { now: 2 });
  assert.equal(result.healthy, true);
  assert.equal(result.pending.status, 'AVAILABLE');
  assert.deepEqual(result.issues, []);
  assert.ok(findPendingCapsule(fp));
}));

test('preview never exposes an unverified tampered capsule', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  const tampered = readFileSync(published.capsulePath, 'utf8').replace('do X', 'malicious instructions');
  writeFileSync(published.capsulePath, tampered);
  const preview = previewFor(cwd);
  assert.equal(preview.valid, false);
  assert.equal('goal' in preview, false);
  assert.ok(preview.errors.includes('payload-integrity-mismatch'));
}));
