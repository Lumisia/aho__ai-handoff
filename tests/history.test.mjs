import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('appendHistory writes entries and readHistory returns newest last, capped', async () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-hist-'));
  process.env.AI_HANDOFF_ROOT = root;
  const { appendHistory, readHistory } = await import('../core/capsule/history.mjs');
  for (let i = 0; i < 5; i++) appendHistory('fp1', { event: 'created', taskId: `t${i}` }, { max: 3 });
  const all = readHistory('fp1', { limit: 10 });
  assert.equal(all.length, 3);                    // capped at max=3
  assert.equal(all[all.length - 1].taskId, 't4'); // newest last
  assert.equal(typeof all[0].ts, 'number');
  delete process.env.AI_HANDOFF_ROOT;
});
