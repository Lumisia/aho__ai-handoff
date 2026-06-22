import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { claimHookEvent } from '../scripts/run-hook.mjs';

test('claimHookEvent prunes lock files older than the lease window', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-hookguard-'));
  for (let i = 0; i < 5; i++) writeFileSync(join(dir, `stale-${i}.lock`), '');
  // Fire far enough in the future that the freshly-written files are well past
  // the lease cutoff and must be swept.
  const future = Date.now() + 600000;
  const claimed = claimHookEvent({ event: 'user-prompt', agent: 'codex', raw: 'unique-payload', dir, now: future });
  assert.equal(claimed, true, 'a fresh event still claims its lease');
  const stale = readdirSync(dir).filter((n) => n.startsWith('stale-'));
  assert.equal(stale.length, 0, 'stale lock files are pruned');
});

test('claimHookEvent keeps a lock that is still within the lease window', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-hookguard-'));
  const now = Date.now();
  // First firing creates the lock; a duplicate sibling within the window is
  // rejected and the lock must survive (not be pruned).
  assert.equal(claimHookEvent({ event: 'stop', agent: 'codex', raw: 'p', dir, now }), true);
  assert.equal(claimHookEvent({ event: 'stop', agent: 'codex', raw: 'p', dir, now: now + 100 }), false);
  assert.ok(readdirSync(dir).some((n) => n.endsWith('.lock')), 'the live lock is retained');
});
