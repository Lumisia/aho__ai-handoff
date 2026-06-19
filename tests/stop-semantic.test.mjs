import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { loadConfig } from '../core/lib/config.mjs';
import { findPendingCapsule } from '../core/capsule/store.mjs';

function withRoot(fn) {
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-semantic-'));
  return Promise.resolve(fn()).finally(() => {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  });
}

const reading = { usedPercent: 84, windowMinutes: 300, resetsAt: 999, source: 'app-server' };

test('second Stop uses stored trigger context and publishes rich AVAILABLE capsule', async () => withRoot(async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const config = loadConfig({});
  config.triggers.five_hour.mode = 'auto';
  const first = await handleStop({
    input: { cwd, session_id: 's1' }, config, readSensor: async () => reading, agent: 'codex', now: 1000,
  });
  assert.equal(first.action, 'request-summary');

  const second = await handleStop({
    input: {
      cwd, session_id: 's1', stop_hook_active: true,
      last_assistant_message: '<handoff-capsule>{"goal":"finish packaging","next_actions":["add hooks"],"completed":["core"]}</handoff-capsule>',
    },
    config,
    readSensor: async () => ({ source: 'unknown' }),
    agent: 'codex',
    now: 2000,
  });
  assert.equal(second.action, 'create');
  const pending = findPendingCapsule(second.fingerprint);
  assert.equal(pending.state.status, 'AVAILABLE');
  assert.equal(pending.capsule.task.goal, 'finish packaging');
  assert.deepEqual(pending.capsule.task.next_actions, ['add hooks']);
}));

test('invalid summary publishes DEGRADED_AVAILABLE and never requests another turn', async () => withRoot(async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const config = loadConfig({});
  config.triggers.five_hour.mode = 'auto';
  await handleStop({ input: { cwd, session_id: 's2' }, config, readSensor: async () => reading, agent: 'codex', now: 1000 });
  const result = await handleStop({
    input: { cwd, session_id: 's2', stop_hook_active: true, last_assistant_message: 'not json' },
    config, readSensor: async () => null, agent: 'codex', now: 2000,
  });
  assert.equal(result.action, 'create');
  assert.equal(result.degraded, true);
  assert.equal(findPendingCapsule(result.fingerprint).state.status, 'DEGRADED_AVAILABLE');
}));
