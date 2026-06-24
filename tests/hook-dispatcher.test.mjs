import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolveHookInvocation, claimHookEvent } from '../scripts/run-hook.mjs';

test('dispatcher maps shared hook events for Codex', () => {
  assert.deepEqual(resolveHookInvocation('session-start', { PLUGIN_ROOT: 'C:/p' }), {
    pluginRoot: 'C:/p', agent: 'codex', command: 'hook:session-start',
  });
  assert.deepEqual(resolveHookInvocation('post-tool-use', { PLUGIN_ROOT: 'C:/p' }), {
    pluginRoot: 'C:/p', agent: 'codex', command: 'hook:post-tool-use',
  });
});

test('dispatcher maps shared hook events for Claude Code', () => {
  assert.deepEqual(resolveHookInvocation('stop', { CLAUDE_PLUGIN_ROOT: '/p' }), {
    pluginRoot: '/p', agent: 'claude-code', command: 'hook:stop',
  });
  assert.deepEqual(resolveHookInvocation('stop', { PLUGIN_ROOT: '/codex', CLAUDE_PLUGIN_ROOT: '/claude' }), {
    pluginRoot: '/claude', agent: 'claude-code', command: 'hook:stop',
  });
});

test('dispatcher rejects unknown events and missing roots', () => {
  assert.throws(() => resolveHookInvocation('wat', { PLUGIN_ROOT: '/p' }), /unknown hook event/);
  assert.throws(() => resolveHookInvocation('stop', {}), /plugin root/);
});

test('claimHookEvent admits the first firing and rejects an identical duplicate in-window', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-guard-'));
  const args = { event: 'session-start', agent: 'codex', raw: '{"session_id":"s1"}', dir, now: 1000, windowMs: 5000 };
  assert.equal(claimHookEvent(args), true, 'first firing runs');
  assert.equal(claimHookEvent(args), false, 'duplicate from double registration is skipped');
});

test('claimHookEvent admits a different payload as a distinct occurrence', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-guard-'));
  const base = { event: 'user-prompt', agent: 'codex', dir, now: 1000, windowMs: 5000 };
  assert.equal(claimHookEvent({ ...base, raw: '{"prompt":"a"}' }), true);
  assert.equal(claimHookEvent({ ...base, raw: '{"prompt":"b"}' }), true, 'a different prompt is a real new event');
});

test('claimHookEvent re-admits the same payload after the window expires', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-guard-'));
  const a = { event: 'stop', agent: 'codex', raw: '{"x":1}', dir, windowMs: 5000 };
  assert.equal(claimHookEvent({ ...a, now: 1000 }), true);
  assert.equal(claimHookEvent({ ...a, now: 1000 }), false);
  assert.equal(claimHookEvent({ ...a, now: 7000 }), true, 'a later identical event runs once the window passed');
});
