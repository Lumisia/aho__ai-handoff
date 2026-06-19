import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveHookInvocation } from '../scripts/run-hook.mjs';

test('dispatcher maps shared hook events for Codex', () => {
  assert.deepEqual(resolveHookInvocation('session-start', { PLUGIN_ROOT: 'C:/p' }), {
    pluginRoot: 'C:/p', agent: 'codex', command: 'hook:session-start',
  });
});

test('dispatcher maps shared hook events for Claude Code', () => {
  assert.deepEqual(resolveHookInvocation('stop', { CLAUDE_PLUGIN_ROOT: '/p' }), {
    pluginRoot: '/p', agent: 'claude-code', command: 'hook:stop',
  });
});

test('dispatcher rejects unknown events and missing roots', () => {
  assert.throws(() => resolveHookInvocation('wat', { PLUGIN_ROOT: '/p' }), /unknown hook event/);
  assert.throws(() => resolveHookInvocation('stop', {}), /plugin root/);
});
