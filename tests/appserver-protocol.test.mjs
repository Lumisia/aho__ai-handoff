import { test } from 'node:test';
import assert from 'node:assert/strict';
import { INIT_REQUEST, reduce, extractPrimary } from '../core/sensors/appserver-protocol.mjs';

test('INIT_REQUEST is the initialize call with id 0', () => {
  assert.equal(INIT_REQUEST.method, 'initialize');
  assert.equal(INIT_REQUEST.id, 0);
  assert.ok(INIT_REQUEST.params.clientInfo.name);
});

test('initialize response triggers initialized + rateLimits read', () => {
  const out = reduce({ id: 0, result: { userAgent: 'x' } });
  assert.equal(out.send.length, 2);
  assert.equal(out.send[0].method, 'initialized');
  assert.equal(out.send[1].method, 'account/rateLimits/read');
  assert.equal(out.send[1].id, 1);
});

test('id:1 result yields extracted primary', () => {
  const out = reduce({ id: 1, result: { rateLimits: { primary: { usedPercent: 57, windowDurationMins: 300, resetsAt: 1781851482 } } } });
  assert.deepEqual(out.result, { usedPercent: 57, windowMinutes: 300, resetsAt: 1781851482, source: 'app-server' });
});

test('id:1 error is surfaced', () => {
  const out = reduce({ id: 1, error: { code: 1, message: 'nope' } });
  assert.equal(out.error.message, 'nope');
});

test('notifications and other ids are ignored', () => {
  assert.deepEqual(reduce({ method: 'remoteControl/status/changed', params: {} }), {});
});

test('extractPrimary returns null when shape missing', () => {
  assert.equal(extractPrimary({}), null);
});
