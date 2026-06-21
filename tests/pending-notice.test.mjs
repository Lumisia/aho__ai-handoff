import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { recordInject, consumeOnPrompt } from '../core/capsule/inject-track.mjs';
import { findNewerPending, recordNotified, renderPendingNotice } from '../core/capsule/pending-notice.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-notice-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function cap(fp, taskId, goal = 'fix the thing', target = 'claude-code', source = 'codex') {
  return buildCapsule({
    taskId, now: '2026-06-19T00:00:00.000Z',
    source: { agent: source }, target: { agent: target },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal, next_actions: ['run tests'] },
  });
}

test('no pending capsule -> no notice', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const r = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 10 });
  assert.equal(r.notify, false);
  assert.equal(r.reason, 'no-pending');
}));

test('a session with no id is never nudged', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-aaaaaaaaaaaa'), { now: 1 });
  const r = findNewerPending({ input: { cwd }, agent: 'claude-code', now: 10 });
  assert.equal(r.notify, false);
  assert.equal(r.reason, 'no-session');
}));

test('a capsule addressed to the peer agent is not nudged', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-bbbbbbbbbbbb', 'codex job', 'codex', 'claude-code'), { now: 1 });
  const r = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 10 });
  assert.equal(r.notify, false);
  assert.equal(r.reason, 'not-target-agent');
}));

test('the capsule already injected into THIS session is not re-nudged', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-cccccccccccc'), { now: 1 });
  recordInject({ fingerprint: fp, sessionId: 's1', taskId: 't-x-cccccccccccc', now: 5 });
  const r = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 10 });
  assert.equal(r.notify, false);
  assert.equal(r.reason, 'already-injected');
}));

test('a newer capsule created after the session consumed its own is nudged', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  // Session s1 receives and consumes capsule A.
  publishCapsule(fp, cap(fp, 't-x-aaaaaaaaaaaa', 'old goal'), { now: 1 });
  recordInject({ fingerprint: fp, sessionId: 's1', taskId: 't-x-aaaaaaaaaaaa', now: 5 });
  assert.equal(consumeOnPrompt({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 10 }).consumed, true);
  // The peer publishes a newer capsule B mid-session.
  publishCapsule(fp, cap(fp, 't-x-bbbbbbbbbbbb', 'new goal'), { now: 100 });
  const r = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 110 });
  assert.equal(r.notify, true);
  assert.equal(r.taskId, 't-x-bbbbbbbbbbbb');
  assert.equal(r.capsule.task.goal, 'new goal');
}));

test('recordNotified suppresses a second nudge for the same capsule in the same session', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-bbbbbbbbbbbb', 'new goal'), { now: 100 });
  const first = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 110 });
  assert.equal(first.notify, true);
  recordNotified({ fingerprint: fp, sessionId: 's1', taskId: 't-x-bbbbbbbbbbbb', now: 111 });
  const second = findNewerPending({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 112 });
  assert.equal(second.notify, false);
  assert.equal(second.reason, 'already-notified');
  // A different live session is still nudged about the same capsule.
  const other = findNewerPending({ input: { cwd, session_id: 's2' }, agent: 'claude-code', now: 113 });
  assert.equal(other.notify, true);
}));

test('renderPendingNotice surfaces the capsule key info', () => withRoot(() => {
  const fp = 'fp';
  const c = cap(fp, 't-x-bbbbbbbbbbbb', 'ship the calendar UI');
  const text = renderPendingNotice(c, 'ko');
  assert.match(text, /ship the calendar UI/);
  assert.match(text, /codex/);
  assert.match(text, /claude-code/);
  assert.match(text, /t-x-bbbbbbbbbbbb/);
  assert.match(text, /\/handoff/);
}));
