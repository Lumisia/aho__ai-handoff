import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprint, projectFingerprintInfo } from '../core/lib/fingerprint.mjs';

function repoWithRemote(url) {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const run = (args) => execFileSync('git', ['-C', dir, ...args], { stdio: 'ignore' });
  run(['init']);
  run(['remote', 'add', 'origin', url]);
  return dir;
}

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

test('projectFingerprintInfo reports a path basis for a non-repo dir', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const info = projectFingerprintInfo(dir);
  assert.equal(info.basis.type, 'path');
  assert.match(info.basis.value, /^path:/);
  assert.equal(info.fingerprint.length, 24);
});

test('fingerprint basis strips userinfo credentials from the remote URL', () => {
  const info = projectFingerprintInfo(
    repoWithRemote('https://user:USERINFO_SECRET@example.invalid/org/repo.git'),
  );
  assert.equal(info.basis.type, 'remote');
  assert.doesNotMatch(info.basis.value, /USERINFO_SECRET/);
});

test('fingerprint basis strips query-string and fragment credentials from the remote URL', () => {
  const info = projectFingerprintInfo(
    repoWithRemote('https://example.invalid/org/repo.git?access_token=QUERY_SECRET#FRAG_SECRET'),
  );
  assert.equal(info.basis.type, 'remote');
  assert.doesNotMatch(info.basis.value, /QUERY_SECRET/);
  assert.doesNotMatch(info.basis.value, /FRAG_SECRET/);
});

test('fingerprint leaves scp-style SSH remotes untouched', () => {
  const info = projectFingerprintInfo(repoWithRemote('git@example.invalid:org/repo.git'));
  assert.equal(info.basis.value, 'remote:git@example.invalid:org/repo.git');
});
