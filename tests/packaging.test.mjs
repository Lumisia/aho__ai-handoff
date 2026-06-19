import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

function json(path) { return JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8')); }

test('Claude and Codex manifests expose the same plugin version', () => {
  const claude = json('../.claude-plugin/plugin.json');
  const codex = json('../.codex-plugin/plugin.json');
  assert.equal(claude.name, 'ai-handoff');
  assert.equal(codex.name, claude.name);
  assert.equal(codex.version, claude.version);
});

test('shared hooks wire both automatic directions and memory recall', () => {
  const hooks = json('../hooks/hooks.json').hooks;
  assert.ok(hooks.SessionStart);
  assert.ok(hooks.Stop);
  assert.ok(hooks.UserPromptSubmit);
  const commands = JSON.stringify(hooks);
  assert.match(commands, /run-hook\.mjs/);
  assert.match(commands, /CLAUDE_PLUGIN_ROOT/);
});
