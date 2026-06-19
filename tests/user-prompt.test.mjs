import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { buildMemoryShard, storeMemoryShard } from '../core/memory/store.mjs';
import { prepareUserPrompt, finalizeUserPrompt } from '../core/hooks/user-prompt.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-prompt-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

test('injects relevant verified memory once per project session', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const fp = projectFingerprint(cwd);
  storeMemoryShard(fp, buildMemoryShard({
    shardId: 'm1', fingerprint: fp, fact: 'OAuth refresh tokens rotate', tags: ['oauth'],
    evidence: [{ type: 'test', value: 'tests/auth passed' }], now: 1,
  }));
  const first = prepareUserPrompt({ input: { cwd, session_id: 's1', prompt: 'fix oauth' }, agent: 'codex' });
  assert.equal(first.injected, true);
  assert.match(first.context, /OAuth refresh/);
  finalizeUserPrompt(first.delivery);
  assert.equal(prepareUserPrompt({ input: { cwd, session_id: 's1', prompt: 'oauth' }, agent: 'codex' }).injected, false);
  assert.equal(prepareUserPrompt({ input: { cwd, session_id: 's2', prompt: 'unrelated bananas' }, agent: 'codex' }).injected, false);
}));
