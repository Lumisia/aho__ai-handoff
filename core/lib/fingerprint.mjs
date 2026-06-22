import { execFileSync } from 'node:child_process';
import { realpathSync, statSync, readFileSync } from 'node:fs';
import { resolve, isAbsolute, join, dirname } from 'node:path';
import { sha256Hex } from './hash.mjs';

// Run git and distinguish "git ran but the command failed" (e.g. not a repo,
// or safe.directory refusal — exit status) from "git could not run at all"
// (EPERM/ENOENT/EACCES — sandbox block or missing binary). Only the latter
// justifies the filesystem fallback: when git merely declined to associate the
// cwd with a repo, mimicking that with a raw .git walk would wrongly attach an
// ancestor repo git itself refused. Returns { ok, value } | { ok:false, blocked }.
function defaultGitRunner(cwd, args) {
  try {
    return { ok: true, value: execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() };
  } catch (err) {
    const code = err && err.code;
    return { ok: false, blocked: code === 'EPERM' || code === 'ENOENT' || code === 'EACCES' };
  }
}

// Locate the repo by walking up from cwd, reading the filesystem only. A
// sandbox that blocks the `git` binary (e.g. Codex returns `spawnSync git
// EPERM`) would otherwise fall back to a path-based fingerprint while the peer
// with working Git uses a remote-based one — the two agents then look in
// different capsule buckets and miss each other's handoffs. This mirrors `git
// rev-parse --show-toplevel` for the common cases. Worktree `.git` files (a
// "gitdir: <path>" pointer) and their shared common dir are resolved so remotes,
// which live in the common config, are still found. Returns
// { root, gitDir, commonDir } or null.
function findGitDirFs(cwd) {
  let dir;
  try { dir = realpathSync(cwd); } catch { dir = resolve(cwd); }
  for (;;) {
    const dotgit = join(dir, '.git');
    let st = null;
    try { st = statSync(dotgit); } catch {}
    if (st) {
      let gitDir = dotgit;
      if (st.isFile()) {
        let txt = '';
        try { txt = readFileSync(dotgit, 'utf8'); } catch {}
        const m = txt.match(/gitdir:\s*(.+?)\s*$/m);
        if (!m) return null;
        gitDir = isAbsolute(m[1]) ? m[1] : resolve(dir, m[1]);
      }
      let commonDir = gitDir;
      try {
        const cd = readFileSync(join(gitDir, 'commondir'), 'utf8').trim();
        if (cd) commonDir = isAbsolute(cd) ? cd : resolve(gitDir, cd);
      } catch {}
      return { root: dir, gitDir, commonDir };
    }
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

// Parse `remote.origin.url` out of a git config file's text. Git writes
// `[remote "origin"]` then an indented `url = ...`; the last value wins for a
// single-valued key. Returns null if origin has no url.
function parseRemoteOriginUrl(text) {
  let inOrigin = false;
  let url = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith('#') || line.startsWith(';')) continue;
    const sec = line.match(/^\[([\w.-]+)(?:\s+"(.*)")?\]$/);
    if (sec) { inOrigin = sec[1].toLowerCase() === 'remote' && sec[2] === 'origin'; continue; }
    if (inOrigin) {
      const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.*)$/);
      if (kv && kv[1].toLowerCase() === 'url') url = kv[2].trim();
    }
  }
  return url;
}

// Filesystem fallback for `git config --get remote.origin.url`. The shared
// (common dir) config carries remotes; a linked worktree's own config does not.
function readRemoteOriginUrlFs(gitInfo) {
  if (!gitInfo) return null;
  for (const base of [gitInfo.commonDir, gitInfo.gitDir]) {
    let txt = null;
    try { txt = readFileSync(join(base, 'config'), 'utf8'); } catch { continue; }
    const url = parseRemoteOriginUrl(txt);
    if (url) return url;
  }
  return null;
}

// Strip credentials embedded in the remote URL so a token never reaches the
// fingerprint hash or doctor output. Two carriers are removed for scheme://
// URLs: userinfo (https://user:TOKEN@host) and the query/fragment
// (https://host/repo.git?access_token=TOKEN#frag). scp-style SSH
// ("git@host:path") has no "://" and is left untouched — git@ is a conventional
// username, not a secret, and it has no query/fragment grammar.
//
// The userinfo class is [^/?#] (everything up to the authority terminator), not
// [^/@]: git/curl treat the LAST "@" before the path as the userinfo<->host
// delimiter, so a password may itself contain "@" (e.g. user:p@ss). Matching
// only up to the first "@" would leak the password tail. The class must also
// exclude "?" and "#" — the authority ends at the first "/", "?" or "#", so a
// "@" inside a query/fragment (e.g. host?token=ab@cd, no path) is not userinfo;
// matching across it would eat the real host and leak the query/fragment tail.
function sanitizeRemoteUrl(url) {
  let out = url.replace(/^([a-zA-Z][a-zA-Z0-9+.-]*:\/\/)[^/?#]*@/, '$1');
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(out)) {
    out = out.replace(/[?#].*$/, '');
  }
  return out;
}

function isSchemeUrl(u) { return /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(u); }
function isWindowsDrive(u) { return /^[A-Za-z]:[\\/]/.test(u); }
// git scp syntax: [user@]host:path — a colon before any slash, and not a scheme
// URL or a Windows drive path. The user part is OPTIONAL ("host:path" with an
// ssh-config host alias is valid), so we must not require an "@".
function isScpLike(u) {
  if (isSchemeUrl(u) || isWindowsDrive(u)) return false;
  return /^[^/:]+:/.test(u);
}

export function projectFingerprintInfo(cwd, { gitRunner = defaultGitRunner } = {}) {
  let basis = null;
  let blocked = false;
  const git = (args) => {
    const r = gitRunner(cwd, args);
    if (r.ok) return r.value;
    if (r.blocked) blocked = true;
    return null;
  };
  // Resolve the .git location from the filesystem at most once, and only when
  // the git binary is blocked — so the working-Git path is byte-for-byte
  // unchanged (existing fingerprints stay stable) and a non-repo cwd is not
  // wrongly attached to an ancestor repo that git declined.
  let gitInfo;
  const fsInfo = () => (gitInfo === undefined ? (gitInfo = findGitDirFs(cwd)) : gitInfo);

  let url = git(['config', '--get', 'remote.origin.url']);
  if (!url && blocked) url = readRemoteOriginUrlFs(fsInfo());
  if (url) {
    const cleaned = sanitizeRemoteUrl(url);
    let value = cleaned;
    // A RELATIVE local remote (e.g. "../upstream.git") hashes identically across
    // unrelated repos that happen to share the spelling, so they would share one
    // capsule store. Anchor it to an absolute path against the repo root so two
    // different checkouts get distinct fingerprints. Scheme URLs, scp-style SSH
    // remotes, and already-absolute paths are global identifiers and left as-is.
    if (!isSchemeUrl(cleaned) && !isScpLike(cleaned) && !isAbsolute(cleaned) && !isWindowsDrive(cleaned)) {
      // Resolve LEXICALLY against the repo root — never realpathSync. The remote
      // target may not exist locally (it is a git URL, not a checkout), and
      // resolving symlinks would make the fingerprint depend on filesystem state
      // (target presence / mount), orphaning capsules when that changes.
      const root = git(['rev-parse', '--show-toplevel']) || (blocked && fsInfo() && fsInfo().root) || cwd;
      value = resolve(root, cleaned);
    }
    basis = { type: 'remote', value: 'remote:' + value };
  }
  if (!basis) {
    let root = git(['rev-parse', '--show-toplevel']);
    if (!root && blocked && fsInfo()) root = fsInfo().root;
    if (root) {
      let resolved = root;
      try { resolved = realpathSync(root); } catch {}
      basis = { type: 'gitroot', value: 'gitroot:' + resolved };
    }
  }
  if (!basis) {
    let resolved = cwd;
    try { resolved = realpathSync(cwd); } catch {}
    basis = { type: 'path', value: 'path:' + resolved };
  }
  return { fingerprint: sha256Hex(basis.value).slice(0, 24), basis };
}

export function projectFingerprint(cwd) {
  return projectFingerprintInfo(cwd).fingerprint;
}
