import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { findPendingCapsule } from '../core/capsule/store.mjs';
import { loadConfig } from '../core/lib/config.mjs';
import { findApproval } from '../core/capsule/approval.mjs';

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

test('auto over threshold requests exactly one semantic summary turn', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const r = await handleStop({ input: { session_id: 's1', cwd }, config: cfgAuto, readSensor: async () => reading, agent: 'codex', now: 1000 });
    assert.equal(r.action, 'request-summary');
    const fp = projectFingerprint(cwd);
    assert.equal(r.fingerprint, fp);
    assert.equal(findPendingCapsule(fp), null);
  });
});

test('completed auto generation makes later Stop in same window deduped', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const args = { input: { session_id: 's1', cwd }, config: cfgAuto, readSensor: async () => reading, agent: 'codex', now: 1000 };
    await handleStop(args);
    const summary = '<handoff-capsule>{"goal":"continue task","next_actions":["run tests"]}</handoff-capsule>';
    const created = await handleStop({ ...args, input: { ...args.input, stop_hook_active: true, last_assistant_message: summary }, now: 1500 });
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

test('ask mode persists AWAITING_USER once and notifies without creating a capsule', async () => {
  await withRoot(async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
    const config = loadConfig({});
    config.triggers.five_hour.mode = 'ask';
    const notifications = [];
    const args = {
      input: { session_id: 's-ask', cwd }, config, readSensor: async () => reading,
      agent: 'codex', now: 1000, notifyFn: (...values) => notifications.push(values),
    };
    const first = await handleStop(args);
    assert.equal(first.action, 'ask');
    assert.equal(findApproval(first.fingerprint).status, 'AWAITING_USER');
    assert.equal(findPendingCapsule(first.fingerprint), null);
    assert.equal(notifications.length, 1);

    const second = await handleStop({ ...args, now: 2000 });
    assert.equal(second.reason, 'deduped');
    assert.equal(findApproval(first.fingerprint).status, 'AWAITING_USER');
  });
});
