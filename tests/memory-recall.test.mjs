import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rankMemoryShards, renderMemoryRecall } from '../core/memory/recall.mjs';

const shards = [
  { shard_id: 'auth', fact: 'OAuth refresh tokens require rotation', tags: ['auth', 'oauth'], paths: ['core/auth.mjs'], branch: 'main', evidence: [{ type: 'test', value: 'auth test passed' }] },
  { shard_id: 'db', fact: 'SQLite uses WAL mode', tags: ['database'], paths: ['core/db.mjs'], branch: 'feature/db', evidence: [{ type: 'file', value: 'core/db.mjs' }] },
];

test('ranks lexical, path, and branch relevance and excludes unrelated shards', () => {
  const ranked = rankMemoryShards(shards, { prompt: 'fix oauth in core/auth.mjs', branch: 'main' });
  assert.equal(ranked[0].shard_id, 'auth');
  assert.equal(ranked.some((x) => x.shard_id === 'db'), false);
  assert.deepEqual(rankMemoryShards(shards, { prompt: 'bananas', branch: 'main' }), []);
});

test('render obeys a strict approximate token budget', () => {
  const text = renderMemoryRecall(shards, { tokenBudget: 30 });
  assert.ok(text.length <= 120);
  assert.match(text, /VERIFIED RELATED MEMORY/);
});
