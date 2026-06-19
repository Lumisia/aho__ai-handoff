import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildMemoryShard, storeMemoryShard, readVerifiedShards } from '../core/memory/store.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-memory-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

test('stores an immutable verified shard with redaction and integrity', () => withRoot(() => {
  const shard = buildMemoryShard({
    shardId: 'm1', fingerprint: 'fp', fact: 'token sk-abcdefghijklmnopqrstuvwxyz123456',
    evidence: [{ type: 'test', value: 'tests/auth.test.mjs passed' }], tags: ['auth'], now: 1,
  });
  const { path } = storeMemoryShard('fp', shard);
  const stored = JSON.parse(readFileSync(path, 'utf8'));
  assert.match(stored.fact, /\[REDACTED\]/);
  assert.equal(readVerifiedShards('fp').length, 1);
  assert.throws(() => storeMemoryShard('fp', { ...shard, fact: 'changed' }), /invalid|already stored/);
}));

test('tampered or unverified shards are excluded from recall evidence', () => withRoot(() => {
  const shard = buildMemoryShard({
    shardId: 'm2', fingerprint: 'fp', fact: 'verified fact',
    evidence: [{ type: 'command', value: 'npm test' }], verified: false, now: 1,
  });
  assert.throws(() => storeMemoryShard('fp', shard), /verified/);
}));
