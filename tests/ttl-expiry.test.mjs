import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';
import { publishCapsule, findPendingCapsule, readState } from '../core/capsule/store.mjs';
import { buildCapsule } from '../core/capsule/create.mjs';
import { statusFor, recentCapsules } from '../core/hooks/handoff.mjs';

function withRoot(fn) {
  const prev = process.env.AI_HANDOFF_ROOT;
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-ttl-'));
  try { return fn(); } finally {
    if (prev === undefined) delete process.env.AI_HANDOFF_ROOT; else process.env.AI_HANDOFF_ROOT = prev;
  }
}

function cap(fp, taskId, { createdAt, expiresAt }) {
  return buildCapsule({
    taskId, now: createdAt, expiresAt,
    source: { agent: 'codex' }, target: { agent: 'claude-code' },
    trigger: { type: 'manual_checkpoint' },
    project: { fingerprint: fp, git_branch: 'main', git_head: 'abc123' },
    checkpoint: { status: 'in_progress' },
    task: { goal: 'g', next_actions: ['x'] },
  });
}

test('findPendingCapsule transitions a past-TTL capsule to EXPIRED and stops returning it', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  const published = publishCapsule(fp, cap(fp, 't-x-eeeeeeeeeeee', {
    createdAt: '2000-01-01T00:00:00.000Z', expiresAt: '2000-01-02T00:00:00.000Z',
  }), { now: 1 });
  assert.equal(readState(published.statePath).status, 'AVAILABLE');

  const found = findPendingCapsule(fp, { now: Date.parse('2026-01-01T00:00:00.000Z') });
  assert.equal(found, null, 'an expired capsule must not be returned as pending');
  const st = readState(published.statePath);
  assert.equal(st.status, 'EXPIRED');
  assert.equal(st.expiration_reason, 'ttl');
}));

test('statusFor reports pending=false once the capsule TTL has passed', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-ffffffffffff', {
    createdAt: '2000-01-01T00:00:00.000Z', expiresAt: '2000-01-02T00:00:00.000Z',
  }), { now: 1 });
  const status = statusFor(cwd);
  assert.equal(status.pending, false);
}));

test('a capsule still within TTL stays pending', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-aaaaaaaaaaaa', {
    createdAt: '2026-01-01T00:00:00.000Z', expiresAt: '2099-01-01T00:00:00.000Z',
  }), { now: 1 });
  assert.ok(findPendingCapsule(fp, { now: Date.parse('2026-06-01T00:00:00.000Z') }));
  assert.equal(statusFor(cwd).pending, true);
}));

test('recentCapsules reports a past-TTL pending capsule as EXPIRED (display-only)', () => withRoot(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const fp = projectFingerprint(cwd);
  publishCapsule(fp, cap(fp, 't-x-eeeeeeeeeeee', {
    createdAt: '2000-01-01T00:00:00.000Z', expiresAt: '2000-01-02T00:00:00.000Z',
  }), { now: 1 });
  const rows = recentCapsules({ limit: 10, now: Date.parse('2026-01-01T00:00:00.000Z') });
  const row = rows.find((r) => r.taskId === 't-x-eeeeeeeeeeee');
  assert.ok(row);
  assert.equal(row.status, 'EXPIRED');
}));
