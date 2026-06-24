import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { prepareTurnHandoff } from '../core/hooks/turn-handoff.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { findPendingCapsule } from '../core/capsule/store.mjs';
import { loadConfig } from '../core/lib/config.mjs';
import { findApproval } from '../core/capsule/approval.mjs';
import { createFromApproval } from '../core/hooks/handoff.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-stop-'));
  return Promise.resolve(fn()).finally(() => {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  });
}

const cfgAuto = loadConfig({});
cfgAuto.triggers.five_hour.mode = 'auto';

const reading = { usedPercent: 85, windowMinutes: 300, resetsAt: 111, source: 'app-server' };

test('Codex Stop-only auto over threshold publishes degraded capsule without continuation', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const r = await handleStop({ input: { session_id: 's1', cwd }, config: cfgAuto, readSensor: async () => reading, agent: 'codex', now: 1000 });
    assert.equal(r.action, 'create');
    assert.equal(r.degraded, true);
    const fp = projectFingerprint(cwd);
    assert.equal(r.fingerprint, fp);
    assert.equal(findPendingCapsule(fp, { now: 1000 }).state.status, 'DEGRADED_AVAILABLE');
  });
});

test('inline auto generation makes later Stop in same window deduped', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const args = { input: { session_id: 's1', cwd }, config: cfgAuto, readSensor: async () => reading, agent: 'codex', now: 1000 };
    const prepared = await prepareTurnHandoff({ ...args, input: { ...args.input, turn_id: 't1' } });
    assert.equal(prepared.injected, true);
    const summary = 'done\n\n```ai-handoff-capsule\n{"goal":"continue task","next_actions":["run tests"]}\n```';
    const created = await handleStop({ ...args, input: { ...args.input, turn_id: 't1', last_assistant_message: summary }, now: 1500 });
    assert.equal(created.action, 'create');
    const r3 = await handleStop({ ...args, now: 2000 });
    assert.equal(r3.action, 'none');
    assert.equal(r3.reason, 'deduped');
  });
});

test('below threshold does nothing', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const r = await handleStop({ input: { session_id: 's2', cwd }, config: cfgAuto, readSensor: async () => ({ ...reading, usedPercent: 10 }), agent: 'codex', now: 1 });
    assert.equal(r.action, 'none');
  });
});

test('disabled trigger never reads the sensor or creates a capsule', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    let reads = 0;
    const config = JSON.parse(JSON.stringify(cfgAuto));
    config.triggers.five_hour.enabled = false;
    const result = await handleStop({
      input: { session_id: 'disabled', cwd }, config,
      readSensor: async () => { reads++; return reading; }, agent: 'codex', now: 1,
    });
    assert.equal(result.reason, 'disabled');
    assert.equal(reads, 0);
  });
});

test('Claude ask keeps re-asking until the user resolves it, then create marks the window seen', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const config = loadConfig({});
    config.triggers.five_hour.mode = 'ask';
    const notifications = [];
    const args = {
      input: { session_id: 's-ask', cwd }, config, readSensor: async () => reading,
      agent: 'claude-code', now: 1000, notifyFn: (...values) => notifications.push(values),
    };
    const first = await handleStop(args);
    assert.equal(first.action, 'ask');
    assert.equal(findApproval(first.fingerprint, { now: 1000 }).status, 'AWAITING_USER');
    assert.equal(findPendingCapsule(first.fingerprint), null);

    // Asking does not mark the window seen: an unresolved ask must be free to
    // surface again on a later Stop (the picker may have failed to render).
    const second = await handleStop({ ...args, now: 2000 });
    assert.equal(second.action, 'ask');
    assert.equal(findApproval(second.fingerprint, { now: 2000 }).status, 'AWAITING_USER');
    assert.equal(notifications.length, 2);

    // The user answers Yes → create resolves the approval AND marks the window
    // seen, so a later Stop in the same window is finally deduped.
    const created = createFromApproval({ cwd, sentinel: { goal: 'g', next_actions: [], status: 'in_progress' }, now: 2500 });
    assert.equal(created.created, true);

    const third = await handleStop({ ...args, now: 3000 });
    assert.equal(third.reason, 'deduped');
  });
});

test('resolved ask dedupes across Claude session changes in the same usage window', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const config = loadConfig({});
    config.triggers.five_hour.mode = 'ask';
    const args = {
      input: { session_id: 's-before-restart', cwd }, config, readSensor: async () => reading,
      agent: 'claude-code', now: 1000, notifyFn: () => {},
    };
    const first = await handleStop(args);
    assert.equal(first.action, 'ask');

    createFromApproval({ cwd, sentinel: { goal: 'g', next_actions: [], status: 'in_progress' }, now: 1500 });

    const afterRestart = await handleStop({
      ...args,
      input: { session_id: 's-after-restart', cwd },
      now: 2000,
    });
    assert.equal(afterRestart.reason, 'deduped');
  });
});

test('notification off still asks but sends no notification for Claude', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const config = loadConfig({});
    config.triggers.five_hour.mode = 'ask';
    config.notification = { method: 'off' };
    const notifications = [];
    const r = await handleStop({
      input: { session_id: 's-off', cwd }, config, readSensor: async () => reading,
      agent: 'claude-code', now: 1000, notifyFn: (...values) => notifications.push(values),
    });
    assert.equal(r.action, 'ask');
    assert.equal(findApproval(r.fingerprint, { now: 1000 }).status, 'AWAITING_USER');
    assert.equal(notifications.length, 0);
  });
});
