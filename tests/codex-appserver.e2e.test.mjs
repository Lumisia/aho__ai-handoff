import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readAppServerRateLimit } from '../core/sensors/codex-appserver.mjs';

test('reads live rate limit from codex app-server', { skip: process.env.AH_E2E !== '1' }, async () => {
  const r = await readAppServerRateLimit({
    timeoutMs: 20000,
    onStderr: process.env.AH_DEBUG === '1' ? (text) => process.stderr.write(text) : undefined,
  });
  assert.ok(r, 'expected a result object');
  assert.equal(typeof r.usedPercent, 'number');
  assert.equal(r.windowMinutes, 300);
  assert.equal(r.source, 'app-server');
});
