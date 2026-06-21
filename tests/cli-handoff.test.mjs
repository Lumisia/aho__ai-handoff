import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { saveApproval } from '../core/capsule/approval.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const cli = join(here, '..', 'core', 'cli.mjs');

function run(args, input, env) {
  return execFileSync(process.execPath, [cli, ...args], { input, encoding: 'utf8', env: { ...process.env, ...env } });
}

test('handoff:checkpoint then status/preview shows pending', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  const sentinel = { goal: 'wire it up', next_actions: ['ship'] };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({ cwd, session_id: 's', sentinel }), env);
  const status = JSON.parse(run(['handoff:status'], JSON.stringify({ cwd }), env));
  assert.equal(status.pending, true);
  const preview = JSON.parse(run(['handoff:preview'], JSON.stringify({ cwd }), env));
  assert.equal(preview.goal, 'wire it up');
});

test('repeated manual checkpoints in one session supersede instead of colliding', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  const first = JSON.parse(run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({
    cwd, session_id: 'same', sentinel: { goal: 'same task', next_actions: ['first'] },
  }), env));
  const second = JSON.parse(run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({
    cwd, session_id: 'same', sentinel: { goal: 'same task', next_actions: ['second'] },
  }), env));
  assert.notEqual(first.taskId, second.taskId);
  assert.deepEqual(JSON.parse(run(['handoff:preview'], JSON.stringify({ cwd }), env)).next_actions, ['second']);
  run(['handoff:resume', '--agent', 'claude-code'], JSON.stringify({ cwd }), env);
  assert.equal(JSON.parse(run(['handoff:status'], JSON.stringify({ cwd }), env)).pending, false);
});

test('handoff:resume injects then consumes', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({ cwd, session_id: 's', sentinel: { goal: 'do it' } }), env);
  const out = run(['handoff:resume', '--agent', 'claude-code'], JSON.stringify({ cwd }), env);
  assert.match(out, /do it/);
  const status = JSON.parse(run(['handoff:status'], JSON.stringify({ cwd }), env));
  assert.equal(status.pending, false);
});

test('handoff:create resolves persisted ask state and publishes capsule', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = root;
  try {
    const fp = projectFingerprint(cwd);
    saveApproval({
      fingerprint: fp, key: 'k1', now: 1,
      context: {
        cwd, agent: 'codex', sessionId: 's1', threshold: 80,
        reading: { usedPercent: 82, source: 'app-server' },
      },
    });
  } finally {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  }
  const env = { AI_HANDOFF_ROOT: root };
  const output = JSON.parse(run(['handoff:create'], JSON.stringify({
    cwd, sentinel: { goal: 'approved cli capsule', next_actions: ['continue'] },
  }), env));
  assert.equal(output.created, true);
  assert.equal(JSON.parse(run(['handoff:status'], JSON.stringify({ cwd }), env)).pending, true);
});

test('handoff:history records created then resumed', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({ cwd, session_id: 's', sentinel: { goal: 'g' } }), env);
  run(['handoff:resume', '--agent', 'claude-code'], JSON.stringify({ cwd, session_id: 'r' }), env);
  const hist = JSON.parse(run(['handoff:history', '--cwd', cwd], '', env));
  // resume now injects read-only (logged) before consuming, so the trail shows both.
  assert.deepEqual(hist.map((h) => h.event), ['created', 'injected', 'resumed']);
  const resumed = hist.find((h) => h.event === 'resumed');
  assert.equal(resumed.session_id, 'r');
  assert.equal(resumed.agent, 'claude-code');
});

test('memory:remember stores evidence and memory:recall returns only relevant memory', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  const remembered = JSON.parse(run(['memory:remember'], JSON.stringify({
    cwd, fact: 'OAuth refresh tokens rotate', tags: ['oauth'],
    evidence: [{ type: 'test', value: 'tests/auth passed' }],
  }), env));
  assert.equal(remembered.stored, true);
  assert.match(run(['memory:recall'], JSON.stringify({ cwd, prompt: 'fix oauth' }), env), /OAuth refresh/);
  assert.equal(run(['memory:recall'], JSON.stringify({ cwd, prompt: 'bananas' }), env), '');
});
