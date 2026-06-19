import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareSessionStart, finalizeSessionStart, abortSessionStart } from '../core/hooks/session-start.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule, readState } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-ss-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function cap(fp) {
  return buildCapsule({
    taskId: 't-x-cccccccccccc', now: '2026-06-19T00:00:00.000Z', source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal: 'fix the thing', next_actions: ['run tests'] },
  });
}

test('prepares injection without consuming, then consumes only after finalize', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  const r = prepareSessionStart({ input: { cwd }, agent: 'claude-code', now: 10 });
  assert.equal(r.injected, true);
  assert.match(r.context, /fix the thing/);
  assert.match(r.context, /CURRENT HANDOFF/);
  assert.match(r.context, /CURRENT TASK/);
  assert.equal(readState(published.statePath).status, 'CLAIMED');
  finalizeSessionStart(r.delivery, { now: 11 });
  assert.equal(readState(published.statePath).status, 'CONSUMED');
  assert.equal(prepareSessionStart({ input: { cwd }, agent: 'claude-code', now: 12 }).injected, false);
}));

test('no pending capsule → not injected', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  assert.equal(prepareSessionStart({ input: { cwd }, agent: 'claude-code', now: 1 }).injected, false);
}));

test('aborting output delivery releases the claim', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  const r = prepareSessionStart({ input: { cwd }, agent: 'claude-code', now: 10 });
  abortSessionStart(r.delivery);
  assert.equal(readState(published.statePath).status, 'AVAILABLE');
}));
