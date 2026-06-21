import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { recentCapsules } from '../core/hooks/handoff.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-recent-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function publish(fp, taskId, createdAt, goal) {
  const cap = buildCapsule({
    taskId, now: createdAt,
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal, next_actions: ['x'] },
  });
  return publishCapsule(fp, cap, { now: Date.parse(createdAt) });
}

test('no buckets -> empty list', () => withRoot(() => {
  assert.deepEqual(recentCapsules({ limit: 10 }), []);
}));

test('lists capsules across all project buckets, newest first', () => withRoot(() => {
  const projA = mkdtempSync(join(tmpdir(), 'ah-A-'));
  const projB = mkdtempSync(join(tmpdir(), 'ah-B-'));
  const fpA = projectFingerprint(projA);
  const fpB = projectFingerprint(projB);
  publish(fpA, 't-x-aaaaaaaaaaaa', '2026-06-19T00:00:00.000Z', 'oldest');
  publish(fpB, 't-x-bbbbbbbbbbbb', '2026-06-20T00:00:00.000Z', 'middle');
  publish(fpA, 't-x-cccccccccccc', '2026-06-21T00:00:00.000Z', 'newest');

  const rows = recentCapsules({ limit: 10 });
  assert.equal(rows.length, 3);
  assert.deepEqual(rows.map((r) => r.goal), ['newest', 'middle', 'oldest']);
  // Both buckets are represented.
  assert.ok(rows.some((r) => r.fingerprint === fpA) && rows.some((r) => r.fingerprint === fpB));
  // Key info is surfaced.
  assert.equal(rows[0].taskId, 't-x-cccccccccccc');
  assert.equal(rows[0].status, 'AVAILABLE');
  assert.equal(rows[0].source, 'codex');
  assert.equal(rows[0].target, 'claude-code');
  assert.equal(rows[0].branch, 'main');
}));

test('limit caps the number of rows', () => withRoot(() => {
  const proj = mkdtempSync(join(tmpdir(), 'ah-L-'));
  const fp = projectFingerprint(proj);
  publish(fp, 't-x-aaaaaaaaaaaa', '2026-06-19T00:00:00.000Z', 'a');
  publish(fp, 't-x-bbbbbbbbbbbb', '2026-06-20T00:00:00.000Z', 'b');
  publish(fp, 't-x-cccccccccccc', '2026-06-21T00:00:00.000Z', 'c');
  const rows = recentCapsules({ limit: 2 });
  assert.equal(rows.length, 2);
  assert.deepEqual(rows.map((r) => r.goal), ['c', 'b']);
}));

test('current flag marks the caller bucket only', () => withRoot(() => {
  const projA = mkdtempSync(join(tmpdir(), 'ah-A-'));
  const projB = mkdtempSync(join(tmpdir(), 'ah-B-'));
  const fpA = projectFingerprint(projA);
  const fpB = projectFingerprint(projB);
  publish(fpA, 't-x-aaaaaaaaaaaa', '2026-06-19T00:00:00.000Z', 'a');
  publish(fpB, 't-x-bbbbbbbbbbbb', '2026-06-20T00:00:00.000Z', 'b');
  const rows = recentCapsules({ limit: 10, currentFingerprint: fpA });
  assert.equal(rows.find((r) => r.fingerprint === fpA).current, true);
  assert.equal(rows.find((r) => r.fingerprint === fpB).current, false);
}));
