import { test } from 'node:test';
import assert from 'node:assert/strict';
import { evaluateTrigger } from '../core/hooks/trigger.mjs';

test('below threshold → none', () => {
  assert.equal(evaluateTrigger({ usedPercent: 50, threshold: 80, mode: 'auto', deduped: false }).action, 'none');
});
test('off mode → none', () => {
  assert.equal(evaluateTrigger({ usedPercent: 90, threshold: 80, mode: 'off', deduped: false }).action, 'none');
});
test('auto over threshold → create', () => {
  assert.equal(evaluateTrigger({ usedPercent: 85, threshold: 80, mode: 'auto', deduped: false }).action, 'create');
});
test('ask over threshold → ask', () => {
  assert.equal(evaluateTrigger({ usedPercent: 85, threshold: 80, mode: 'ask', deduped: false }).action, 'ask');
});
test('deduped → none', () => {
  assert.equal(evaluateTrigger({ usedPercent: 85, threshold: 80, mode: 'auto', deduped: true }).action, 'none');
});
test('unknown usedPercent → none', () => {
  assert.equal(evaluateTrigger({ usedPercent: undefined, threshold: 80, mode: 'auto', deduped: false }).action, 'none');
});

const base = { threshold: 95, mode: 'ask', deduped: false };

test('static threshold still fires (regression)', () => {
  assert.equal(evaluateTrigger({ ...base, usedPercent: 96 }).action, 'ask');
  assert.equal(evaluateTrigger({ ...base, usedPercent: 50 }).action, 'none');
});

test('burn-rate fires below threshold when exhaustion is within runway', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 10 * 60000 }, { usedPercent: 80, at: now }]; // +20%/10min => 100% in 10min
  const ev = evaluateTrigger({ ...base, usedPercent: 80, samples, burnRate: { enabled: true, runwayMinutes: 30 }, now });
  assert.equal(ev.action, 'ask');
  assert.equal(ev.reason, 'burn-rate');
});

test('burn-rate does not fire when projection is beyond runway', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 60 * 60000 }, { usedPercent: 62, at: now }]; // slow
  assert.equal(evaluateTrigger({ ...base, usedPercent: 62, samples, burnRate: { enabled: true, runwayMinutes: 30 }, now }).action, 'none');
});

test('burn-rate disabled => static only', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 10 * 60000 }, { usedPercent: 80, at: now }];
  assert.equal(evaluateTrigger({ ...base, usedPercent: 80, samples, burnRate: { enabled: false }, now }).action, 'none');
});

test('burn-rate enabled but too few samples => insufficient-samples', () => {
  assert.equal(evaluateTrigger({ ...base, usedPercent: 80, samples: [], burnRate: { enabled: true, runwayMinutes: 30 }, now: 1 }).reason, 'insufficient-samples');
});
