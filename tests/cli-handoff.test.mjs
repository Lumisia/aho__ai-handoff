import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

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

test('handoff:resume injects then consumes', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({ cwd, session_id: 's', sentinel: { goal: 'do it' } }), env);
  const out = run(['handoff:resume'], JSON.stringify({ cwd }), env);
  assert.match(out, /do it/);
  const status = JSON.parse(run(['handoff:status'], JSON.stringify({ cwd }), env));
  assert.equal(status.pending, false);
});
