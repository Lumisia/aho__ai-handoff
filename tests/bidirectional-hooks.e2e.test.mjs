import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const cli = join(root, 'core', 'cli.mjs');
const dispatcher = join(root, 'scripts', 'run-hook.mjs');

function run(file, args, input, env) {
  return execFileSync(process.execPath, [file, ...args], {
    input: JSON.stringify(input), encoding: 'utf8', env: { ...process.env, ...env },
  });
}

function sentinel(goal) {
  return `<handoff-capsule>${JSON.stringify({ goal, next_actions: ['continue'], completed: [], open_issues: [], status: 'in_progress' })}</handoff-capsule>`;
}

function inlineSentinel(goal) {
  return `done\n\n\`\`\`ai-handoff-capsule\n${JSON.stringify({ goal, next_actions: ['continue'], completed: [], open_issues: [], status: 'in_progress' })}\n\`\`\``;
}

test('shared automatic hooks hand off Codex → Claude → Codex', () => {
  const data = mkdtempSync(join(tmpdir(), 'ah-bi-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const codexHome = mkdtempSync(join(tmpdir(), 'ah-codex-'));
  const sessions = join(codexHome, 'sessions', '2026', '06', '19');
  mkdirSync(sessions, { recursive: true });
  writeFileSync(join(sessions, 'rollout-test.jsonl'), JSON.stringify({
    type: 'event_msg', payload: { type: 'token_count', rate_limits: {
      primary: { used_percent: 90, window_minutes: 300, resets_at: 9999999999 },
    } },
  }) + '\n');
  writeFileSync(join(data, 'config.json'), JSON.stringify({
    triggers: { five_hour: { enabled: true, threshold_percent: 80, mode: 'auto' } },
    handoff: { session_start_auto_fetch: true },
  }));

  const common = { AI_HANDOFF_ROOT: data, AH_NO_APPSERVER: '1', CODEX_HOME: codexHome };
  // Codex sets PLUGIN_ROOT (not CLAUDE_PLUGIN_ROOT) — simulate that faithfully so
  // the dispatcher's agent detection is actually exercised.
  const codexEnv = { ...common, PLUGIN_ROOT: root, CLAUDE_PLUGIN_ROOT: '' };
  const claudeEnv = { ...common, CLAUDE_PLUGIN_ROOT: root };

  const first = run(dispatcher, ['user-prompt'], { cwd, session_id: 'codex-s', turn_id: 'codex-t1' }, codexEnv);
  assert.match(first, /ai-handoff-capsule/);
  run(dispatcher, ['stop'], {
    cwd, session_id: 'codex-s', turn_id: 'codex-t1',
    last_assistant_message: inlineSentinel('Codex to Claude automatic'),
  }, codexEnv);
  assert.match(run(dispatcher, ['session-start'], { cwd, session_id: 'claude-s' }, claudeEnv), /Codex to Claude automatic/);

  run(cli, ['sensor:claude-statusline'], {
    cwd, session_id: 'claude-s', rate_limits: {
      five_hour: { used_percentage: 91, resets_at: 9999999999 },
    },
  }, claudeEnv);
  // Claude keeps its Stop continuation behavior; Codex now injects before Stop.
  const second = JSON.parse(run(dispatcher, ['stop'], { cwd, session_id: 'claude-s' }, claudeEnv));
  assert.equal(second.decision, undefined);
  assert.match(second.hookSpecificOutput.additionalContext, /handoff-capsule/);
  run(dispatcher, ['stop'], {
    cwd, session_id: 'claude-s', stop_hook_active: true,
    last_assistant_message: sentinel('Claude to Codex automatic'),
  }, claudeEnv);
  assert.match(run(dispatcher, ['session-start'], { cwd, session_id: 'codex-next' }, codexEnv), /Claude to Codex automatic/);
});
