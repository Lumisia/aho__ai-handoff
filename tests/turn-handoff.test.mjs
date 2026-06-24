import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { findGeneration } from '../core/capsule/generation.mjs';
import { findApproval } from '../core/capsule/approval.mjs';
import { handleStop } from '../core/hooks/stop.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { loadConfig } from '../core/lib/config.mjs';
import { prepareTurnHandoff } from '../core/hooks/turn-handoff.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-turn-'));
  return Promise.resolve(fn()).finally(() => {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = prev;
  });
}

const reading = { usedPercent: 86, windowMinutes: 300, resetsAt: 123, source: 'app-server' };

test('auto mode injects inline final capsule instruction before Stop', async () => withRoot(async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const config = loadConfig({});
  config.triggers.five_hour.mode = 'auto';
  const result = await prepareTurnHandoff({
    input: { cwd, session_id: 's1', turn_id: 't1' },
    config,
    readSensor: async () => reading,
    agent: 'codex',
    now: 1000,
  });

  assert.equal(result.injected, true);
  assert.match(result.context, /```ai-handoff-capsule/);
  const generation = findGeneration(result.slotKey);
  assert.equal(generation.context.strategy, 'codex-inline-final');
  assert.equal(generation.context.turnId, 't1');
  assert.equal(result.fingerprint, projectFingerprint(cwd));
}));

test('ask mode injects question instruction and saves approval before Stop', async () => withRoot(async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const config = loadConfig({});
  config.triggers.five_hour.mode = 'ask';
  const result = await prepareTurnHandoff({
    input: { cwd, session_id: 's2', turn_id: 't2' },
    config,
    readSensor: async () => reading,
    agent: 'codex',
    now: 1000,
  });

  assert.equal(result.injected, true);
  assert.match(result.context, /request_user_input/);
  assert.equal(result.action, 'ask');
}));

test('ask deferred at Codex Stop is injected on the next prompt', async () => withRoot(async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-project-'));
  const config = loadConfig({});
  config.triggers.five_hour.mode = 'ask';
  const stopped = await handleStop({
    input: { cwd, session_id: 's3' },
    config,
    readSensor: async () => reading,
    agent: 'codex',
    now: 1000,
    notifyFn: () => {},
  });

  assert.equal(stopped.action, 'none');
  assert.equal(findApproval(stopped.fingerprint, { now: 1000 }).status, 'AWAITING_USER');

  const result = await prepareTurnHandoff({
    input: { cwd, session_id: 's3', turn_id: 't3' },
    config,
    readSensor: async () => reading,
    agent: 'codex',
    now: 2000,
  });

  assert.equal(result.injected, true);
  assert.match(result.context, /request_user_input/);
}));
