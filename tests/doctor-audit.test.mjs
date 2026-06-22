import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { handoffDir } from '../core/lib/paths.mjs';
import { publishCapsule } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { appendHistory } from '../core/capsule/history.mjs';
import { doctorFor } from '../core/hooks/handoff.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-audit-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function publish(cwd) {
  const fp = projectFingerprint(cwd);
  const cap = buildCapsule({
    taskId: 't-x-aaaaaaaaaaaa', now: '2026-06-19T00:00:00.000Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal: 'g', next_actions: ['x'] },
  });
  publishCapsule(fp, cap, { now: 1 });
  return { fp, dir: join(handoffDir(fp), 't-x-aaaaaaaaaaaa') };
}

function issues(cwd) { return doctorFor(cwd).audit.map((a) => a.issue); }

test('a clean bucket audits healthy', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  publish(cwd);
  const d = doctorFor(cwd);
  assert.deepEqual(d.audit, []);
  assert.equal(d.healthy, true);
}));

test('a corrupt state.json is reported and marks the bucket unhealthy', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const { dir } = publish(cwd);
  writeFileSync(join(dir, 'state.json'), 'not json {');
  const d = doctorFor(cwd);
  assert.ok(d.audit.some((a) => a.issue === 'invalid-state-json'));
  assert.equal(d.healthy, false);
}));

test('an orphaned capsule with no state is reported as missing-state', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const { dir } = publish(cwd);
  rmSync(join(dir, 'state.json'));
  assert.ok(issues(cwd).includes('missing-state'));
}));

test('a missing capsule.sha256 is reported', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const { dir } = publish(cwd);
  rmSync(join(dir, 'capsule.sha256'));
  assert.ok(issues(cwd).includes('missing-sha'));
}));

test('a state.json that parses to null is reported as invalid-state-shape without crashing', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const { dir } = publish(cwd);
  writeFileSync(join(dir, 'state.json'), 'null');
  const d = doctorFor(cwd); // must not throw despite the null state
  assert.ok(d.audit.some((a) => a.issue === 'invalid-state-shape'));
  assert.equal(d.healthy, false);
}));

test('history referencing a vanished task dir is reported as history-without-task', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  appendHistory(fp, { event: 'created', taskId: 't-x-ghostghostgh' }, { now: 1 });
  const d = doctorFor(cwd);
  assert.ok(d.audit.some((a) => a.issue === 'history-without-task' && a.taskId === 't-x-ghostghostgh'));
  assert.equal(d.healthy, false);
}));
