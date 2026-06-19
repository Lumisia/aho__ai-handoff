import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { saveApproval, findApproval, resolveApproval } from '../core/capsule/approval.mjs';

function withRoot(fn) {
  const previous = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-approval-'));
  try { return fn(); } finally {
    if (previous === undefined) delete process.env.AI_HANDOFF_ROOT;
    else process.env.AI_HANDOFF_ROOT = previous;
  }
}

test('persists one awaiting-user approval with trigger context', () => withRoot(() => {
  saveApproval({ fingerprint: 'fp1', key: 'k1', context: { agent: 'codex', usedPercent: 82 }, now: 10 });
  assert.deepEqual(findApproval('fp1'), {
    key: 'k1', status: 'AWAITING_USER', context: { agent: 'codex', usedPercent: 82 }, updated_at: 10,
  });
}));

test('skip resolves approval and removes it from awaiting lookup', () => withRoot(() => {
  saveApproval({ fingerprint: 'fp1', key: 'k1', context: {}, now: 10 });
  const resolved = resolveApproval('fp1', { key: 'k1', decision: 'skip', now: 20 });
  assert.equal(resolved.status, 'SKIPPED');
  assert.equal(findApproval('fp1'), null);
}));

test('create resolves approval to generating and returns its stored context', () => withRoot(() => {
  saveApproval({ fingerprint: 'fp1', key: 'k1', context: { sessionId: 's1' }, now: 10 });
  const resolved = resolveApproval('fp1', { key: 'k1', decision: 'create', now: 20 });
  assert.equal(resolved.status, 'GENERATING');
  assert.equal(resolved.context.sessionId, 's1');
}));
