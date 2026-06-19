import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';

test('fingerprint is deterministic and 24 hex chars', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const a = projectFingerprint(dir);
  const b = projectFingerprint(dir);
  assert.equal(a, b);
  assert.match(a, /^[0-9a-f]{24}$/);
});

test('different dirs give different fingerprints', () => {
  const d1 = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const d2 = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  assert.notEqual(projectFingerprint(d1), projectFingerprint(d2));
});
