import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
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

// P1 regression: a sandbox where the git binary is blocked (Codex returns
// `spawnSync git EPERM`) must NOT fall back to a path-based bucket while the
// peer with working Git uses a remote-based one — that split is exactly why a
// Codex session reported pending=false for a capsule Claude had published.
const BLOCKED = () => ({ ok: false, blocked: true });

test('git failure (sandbox EPERM) yields the same remote fingerprint via .git/config', () => {
  const dir = repoWithRemote('https://github.com/Lumisia/claude-codex-auto-handoff.git');
  const withGit = projectFingerprintInfo(dir);
  const noGit = projectFingerprintInfo(dir, { gitRunner: BLOCKED });
  assert.equal(withGit.basis.type, 'remote');
  assert.equal(noGit.basis.type, 'remote', 'fs fallback must still resolve the remote');
  assert.equal(noGit.basis.value, withGit.basis.value);
  assert.equal(noGit.fingerprint, withGit.fingerprint);
});

test('git that ran but found no repo does NOT trigger the fs fallback (non-blocked)', () => {
  // A non-repo cwd whose git command fails by exit status (not EPERM) must keep
  // the path basis even when an unrelated ancestor repo exists on disk.
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const info = projectFingerprintInfo(dir, { gitRunner: () => ({ ok: false, blocked: false }) });
  assert.equal(info.basis.type, 'path');
  assert.match(info.fingerprint, /^[0-9a-f]{24}$/);
});

test('git failure decodes a quoted remote URL in .git/config to match working git', () => {
  const dir = repoWithRemote('https://example.invalid/org/repo.git');
  const cfgPath = join(dir, '.git', 'config');
  // Re-quote the url value the way git tolerates; working `git config --get`
  // returns it unquoted, so the fs fallback must decode the quotes to match.
  const cfg = readFileSync(cfgPath, 'utf8').replace(
    'url = https://example.invalid/org/repo.git',
    'url = "https://example.invalid/org/repo.git"',
  );
  writeFileSync(cfgPath, cfg);
  const withGit = projectFingerprintInfo(dir);
  const noGit = projectFingerprintInfo(dir, { gitRunner: BLOCKED });
  assert.equal(noGit.basis.value, 'remote:https://example.invalid/org/repo.git');
  assert.equal(noGit.fingerprint, withGit.fingerprint);
});

test('git failure in a linked worktree resolves the shared remote config', () => {
  const main = mkdtempSync(join(tmpdir(), 'ah-fp-main-'));
  const env = {
    ...process.env, GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@e',
    GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@e',
  };
  const run = (args) => execFileSync('git', ['-C', main, ...args], { stdio: 'ignore', env });
  run(['init']);
  run(['remote', 'add', 'origin', 'https://github.com/Lumisia/claude-codex-auto-handoff.git']);
  run(['commit', '--allow-empty', '-m', 'init']);
  const wt = join(mkdtempSync(join(tmpdir(), 'ah-fp-wtp-')), 'wt');
  run(['worktree', 'add', wt, 'HEAD']);

  // The worktree's `.git` is a file pointer; with git blocked we must still read
  // the remote from the common config and match the main checkout's fingerprint.
  const noGitWt = projectFingerprintInfo(wt, { gitRunner: BLOCKED });
  assert.equal(noGitWt.basis.type, 'remote');
  assert.equal(noGitWt.fingerprint, projectFingerprintInfo(main).fingerprint);
});
