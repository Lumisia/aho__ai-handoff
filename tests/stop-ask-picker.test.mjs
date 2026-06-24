import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const cli = join(root, 'core', 'cli.mjs');

function run(args, input, env) {
  return execFileSync(process.execPath, [cli, ...args], {
    input: JSON.stringify(input), encoding: 'utf8', env: { ...process.env, ...env },
  });
}

function askEnv() {
  const data = mkdtempSync(join(tmpdir(), 'ah-ask-'));
  writeFileSync(join(data, 'config.json'), JSON.stringify({
    triggers: { five_hour: { enabled: true, threshold_percent: 80, mode: 'ask' } },
    notification: { method: 'off' },
  }));
  return { AI_HANDOFF_ROOT: data, AH_NO_APPSERVER: '1' };
}

test('codex ask branch injects request_user_input before Stop', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-askp-'));
  const codexHome = mkdtempSync(join(tmpdir(), 'ah-askc-'));
  const sessions = join(codexHome, 'sessions', '2026', '06', '22');
  mkdirSync(sessions, { recursive: true });
  writeFileSync(join(sessions, 'rollout-ask.jsonl'), JSON.stringify({
    type: 'event_msg', payload: { type: 'token_count', rate_limits: {
      primary: { used_percent: 90, window_minutes: 300, resets_at: 9999999999 },
    } },
  }) + '\n');
  const env = { ...askEnv(), CODEX_HOME: codexHome };

  const out = run(['hook:user-prompt', '--agent', 'codex'], { cwd, session_id: 'codex-s', turn_id: 't1' }, env);
  assert.match(out, /request_user_input/);
  assert.match(out, /\/handoff create/);

  const stop = JSON.parse(run(['hook:stop', '--agent', 'codex'], { cwd, session_id: 'codex-s', turn_id: 't1' }, env));
  assert.deepEqual(stop, { continue: true });
});

test('claude-code ask branch tells the model to use AskUserQuestion', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-askp2-'));
  const env = { ...askEnv(), CODEX_HOME: join(tmpdir(), '__none__') };

  // Record a fresh Claude rate-limit reading the stop sensor will read back.
  run(['sensor:claude-statusline'], {
    cwd, session_id: 'claude-s',
    rate_limits: { five_hour: { used_percentage: 91, resets_at: 9999999999 } },
  }, env);

  // Claude Stop continuation is non-error feedback via additionalContext, not a
  // decision:block — see core/lib/hook-output.mjs.
  const out = JSON.parse(run(['hook:stop', '--agent', 'claude-code'], { cwd, session_id: 'claude-s' }, env));
  assert.equal(out.decision, undefined);
  assert.equal(out.hookSpecificOutput.hookEventName, 'Stop');
  assert.match(out.hookSpecificOutput.additionalContext, /AskUserQuestion/);
});
