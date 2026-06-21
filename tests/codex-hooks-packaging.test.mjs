import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

function codexStopCommand() {
  const manifest = JSON.parse(readFileSync(join(root, '.codex-plugin', 'plugin.json'), 'utf8'));
  assert.equal(typeof manifest.hooks, 'string', '.codex-plugin/plugin.json must declare a hooks file');
  const hooks = JSON.parse(readFileSync(join(root, manifest.hooks), 'utf8'));
  const cmd = hooks.hooks.Stop[0].hooks[0].command;
  return cmd;
}

test('Codex hooks are declared and use the Codex-native ${PLUGIN_ROOT}', () => {
  const cmd = codexStopCommand();
  assert.match(cmd, /\$\{PLUGIN_ROOT\}/, 'Codex hook command must use ${PLUGIN_ROOT}');
  assert.doesNotMatch(cmd, /CLAUDE_PLUGIN_ROOT/, 'Codex hook command must not depend on the Claude var');
});

test('the declared Codex Stop command fires with only PLUGIN_ROOT in the environment', () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-cxpkg-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-cxproj-'));
  const codexHome = mkdtempSync(join(tmpdir(), 'ah-cxhome-'));
  const sessions = join(codexHome, 'sessions', '2026', '06', '19');
  mkdirSync(sessions, { recursive: true });
  writeFileSync(join(sessions, 'rollout-test.jsonl'), JSON.stringify({
    type: 'event_msg',
    payload: { type: 'token_count', rate_limits: { primary: { used_percent: 90, window_minutes: 300, resets_at: 9999999999 } } },
  }) + '\n');
  writeFileSync(join(data, 'config.json'), JSON.stringify({
    triggers: { five_hour: { enabled: true, threshold_percent: 80, mode: 'auto' } },
  }));

  // Build the command exactly as Codex would: substitute ${PLUGIN_ROOT}, and
  // provide ONLY PLUGIN_ROOT (no CLAUDE_PLUGIN_ROOT) so this is a faithful
  // Codex-runtime simulation of the packaged hook path.
  const cmd = codexStopCommand().replaceAll('${PLUGIN_ROOT}', root);
  const out = execSync(cmd, {
    input: JSON.stringify({ cwd, session_id: 'codex-pkg' }),
    encoding: 'utf8',
    env: { ...process.env, PLUGIN_ROOT: root, AI_HANDOFF_ROOT: data, AH_NO_APPSERVER: '1', CODEX_HOME: codexHome, CLAUDE_PLUGIN_ROOT: '' },
  });
  assert.equal(JSON.parse(out).decision, 'block', 'Stop hook should request a summary at 90% usage');
});
