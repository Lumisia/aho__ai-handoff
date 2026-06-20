import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { projectFingerprintInfo } from '../core/lib/fingerprint.mjs';
import { sha256Hex } from '../core/lib/hash.mjs';

function makeRepo(remoteUrl) {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  execFileSync('git', ['-C', dir, 'init', '-q'], { stdio: 'ignore' });
  execFileSync('git', ['-C', dir, 'remote', 'add', 'origin', remoteUrl], { stdio: 'ignore' });
  return dir;
}

test('credential userinfo is stripped from the fingerprint basis', () => {
  const dir = makeRepo('https://user:SECRETTOKEN@github.com/owner/repo.git');
  const info = projectFingerprintInfo(dir);
  assert.equal(info.basis.type, 'remote');
  assert.ok(!info.basis.value.includes('SECRETTOKEN'), 'token must not appear in basis');
  assert.ok(!info.basis.value.includes('user:'), 'userinfo must not appear in basis');
  assert.equal(info.basis.value, 'remote:https://github.com/owner/repo.git');
});

test('fingerprint is derived from the sanitized basis (secret-free, normalized)', () => {
  const credInfo = projectFingerprintInfo(makeRepo('https://user:SECRETTOKEN@github.com/owner/repo.git'));
  const cleanInfo = projectFingerprintInfo(makeRepo('https://github.com/owner/repo.git'));
  // Same repo with or without embedded credentials => same fingerprint.
  assert.equal(credInfo.fingerprint, cleanInfo.fingerprint);
  assert.equal(credInfo.fingerprint, sha256Hex('remote:https://github.com/owner/repo.git').slice(0, 24));
});

test('clean https remote is unchanged (no fingerprint drift for the common case)', () => {
  const info = projectFingerprintInfo(makeRepo('https://github.com/owner/repo.git'));
  assert.equal(info.basis.value, 'remote:https://github.com/owner/repo.git');
  assert.equal(info.fingerprint, sha256Hex('remote:https://github.com/owner/repo.git').slice(0, 24));
});

test('scp-style ssh remote keeps its git@ user (not a credential)', () => {
  const info = projectFingerprintInfo(makeRepo('git@github.com:owner/repo.git'));
  assert.equal(info.basis.value, 'remote:git@github.com:owner/repo.git');
});
