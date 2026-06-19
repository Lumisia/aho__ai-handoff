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

test('hook:session-start with no pending prints empty context', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cli-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const out = run(['hook:session-start'], JSON.stringify({ cwd }), { AI_HANDOFF_ROOT: root });
  assert.equal(out.trim(), '');
});

test('hook:stop off mode is a no-op and exits 0', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cli-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const out = run(['hook:stop', '--agent', 'codex', '--mode', 'off'], JSON.stringify({ session_id: 's', cwd }), {
    AI_HANDOFF_ROOT: root, AH_NO_APPSERVER: '1', CODEX_HOME: join(root, '__none__'),
  });
  assert.equal(out.trim(), '');
});
