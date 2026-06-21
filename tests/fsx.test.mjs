import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeFileAtomic, acquireLock, releaseLock, withLock } from '../core/lib/fsx.mjs';

test('writeFileAtomic writes data and leaves no temp file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fsx-'));
  const f = join(dir, 'sub', 'out.json');
  writeFileAtomic(f, '{"x":1}');
  assert.equal(readFileSync(f, 'utf8'), '{"x":1}');
  assert.deepEqual(readdirSync(join(dir, 'sub')), ['out.json']);
});

test('acquireLock blocks a second holder until lease expires', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-lock-'));
  const lp = join(dir, '.lock');
  const a = acquireLock(lp, { leaseMs: 1000, now: 1000 });
  assert.ok(a);
  assert.equal(acquireLock(lp, { leaseMs: 1000, now: 1500 }), null);
  const c = acquireLock(lp, { leaseMs: 1000, now: 3000 });
  assert.ok(c);
  releaseLock(c);
  assert.ok(acquireLock(lp, { leaseMs: 1000, now: 4000 }));
});

test('withLock runs fn under the lock and reports success', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-wl-'));
  const lp = join(dir, '.lock');
  let ran = false;
  const ok = withLock(lp, () => { ran = true; });
  assert.equal(ran, true);
  assert.equal(ok, true);
});

test('withLock skips fn and reports failure when the lock is held', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-wl-'));
  const lp = join(dir, '.lock');
  const held = acquireLock(lp, { leaseMs: 60_000 });
  assert.ok(held);
  let ran = false;
  const ok = withLock(lp, () => { ran = true; }, { tries: 2, waitMs: 1, leaseMs: 60_000 });
  assert.equal(ran, false, 'fn must not run without holding the lock');
  assert.equal(ok, false);
  releaseLock(held);
});
