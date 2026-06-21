import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareSessionStart, finalizeSessionStart } from '../core/hooks/session-start.mjs';
import { consumeOnPrompt } from '../core/capsule/inject-track.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule, readState, findPendingCapsule } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { readHistory } from '../core/capsule/history.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-consume-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function cap(fp) {
  return buildCapsule({
    taskId: 't-x-cccccccccccc', now: '2026-06-19T00:00:00.000Z',
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal: 'fix the thing', next_actions: ['run tests'] },
  });
}

function inject(cwd, sessionId, agent = 'claude-code', now = 10) {
  const r = prepareSessionStart({ input: { cwd, session_id: sessionId }, agent, now });
  if (r.injected) finalizeSessionStart(r.delivery, { now });
  return r;
}

test('SessionStart injects read-only: the capsule stays pending and is not consumed', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  const r = inject(cwd, 's1', 'claude-code', 10);
  assert.equal(r.injected, true);
  assert.match(r.context, /fix the thing/);
  assert.equal(readState(published.statePath).status, 'AVAILABLE', 'inject must not change capsule status');
  assert.ok(findPendingCapsule(fp), 'capsule is still pending after a read-only inject');
}));

test('a promptless ephemeral SessionStart leaves the capsule for the next session', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  inject(cwd, 'probe', 'claude-code', 10); // ephemeral: injected, never prompts
  assert.equal(readState(published.statePath).status, 'AVAILABLE');
  const real = prepareSessionStart({ input: { cwd, session_id: 'real' }, agent: 'claude-code', now: 20 });
  assert.equal(real.injected, true, 'a later real session still sees the handoff');
}));

test('first UserPromptSubmit of an injected session consumes and records consumed_by', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  inject(cwd, 's1', 'claude-code', 10);
  const res = consumeOnPrompt({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 30 });
  assert.equal(res.consumed, true);
  const st = readState(published.statePath);
  assert.equal(st.status, 'CONSUMED');
  assert.equal(st.consumed_by.agent, 'claude-code');
  assert.equal(st.consumed_by.session_id, 's1');
}));

test('a non-target agent skips the capsule without rejecting it', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  // Capsule addressed to codex (handed off by claude-code).
  const c = buildCapsule({
    taskId: 't-x-dddddddddddd', now: '2026-06-19T00:00:00.000Z',
    source: { agent: 'claude-code' }, target: { agent: 'codex' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal: 'codex should do this', next_actions: ['x'] },
  });
  const published = publishCapsule(fp, c, { now: 1 });
  // A claude-code session must NOT touch a capsule meant for codex.
  const r = prepareSessionStart({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 10 });
  assert.equal(r.injected, false);
  assert.equal(r.reason, 'not-target-agent');
  assert.equal(readState(published.statePath).status, 'AVAILABLE', 'peer capsule must survive');
  // The intended target still receives it.
  const r2 = prepareSessionStart({ input: { cwd, session_id: 's2' }, agent: 'codex', now: 11 });
  assert.equal(r2.injected, true);
}));

test('UserPromptSubmit for a session that was never injected does not consume', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp), { now: 1 });
  const res = consumeOnPrompt({ input: { cwd, session_id: 's2' }, agent: 'claude-code', now: 30 });
  assert.equal(res.consumed, false);
  assert.equal(readState(published.statePath).status, 'AVAILABLE');
}));

test('handoff consume is once per session', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp), { now: 1 });
  inject(cwd, 's1', 'claude-code', 10);
  assert.equal(consumeOnPrompt({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 30 }).consumed, true);
  assert.equal(consumeOnPrompt({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 31 }).consumed, false);
}));

test('history records who resumed the capsule', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp), { now: 1 });
  inject(cwd, 's1', 'claude-code', 10);
  consumeOnPrompt({ input: { cwd, session_id: 's1' }, agent: 'claude-code', now: 30 });
  const resumed = readHistory(fp, { limit: 20 }).find((h) => h.event === 'resumed');
  assert.ok(resumed, 'a resumed event is recorded');
  assert.equal(resumed.agent, 'claude-code');
  assert.equal(resumed.session_id, 's1');
}));
