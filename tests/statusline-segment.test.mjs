import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { statuslineSegment } from '../core/lib/statusline-segment.mjs';

const cli = join(dirname(fileURLToPath(import.meta.url)), '..', 'core', 'cli.mjs');

test('statuslineSegment shows usage only when no pending capsule', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-sl-'));
  assert.equal(statuslineSegment({ usedPercent: 82, cwd, show: true }), 'AH 82%');
});

test('statuslineSegment adds pending marker when a capsule is pending', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-slr-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-slp-'));
  process.env.AI_HANDOFF_ROOT = root;
  execFileSync(process.execPath, [cli, 'handoff:checkpoint', '--agent', 'codex', '--cwd', cwd],
    { input: JSON.stringify({ session_id: 's', sentinel: { goal: 'g' } }), encoding: 'utf8', env: process.env });
  assert.equal(statuslineSegment({ usedPercent: 82, cwd, show: true }), 'AH 82% · ⏳1');
  delete process.env.AI_HANDOFF_ROOT;
});

test('statuslineSegment returns empty when show is false', () => {
  assert.equal(statuslineSegment({ usedPercent: 82, cwd: undefined, show: false }), '');
});
