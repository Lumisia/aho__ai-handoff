import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { sha256Hex } from './hash.mjs';

function git(cwd, args) {
  try {
    return execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
}

// Strip credentials embedded in the remote URL so a token never reaches the
// fingerprint hash or doctor output. Two carriers are removed for scheme://
// URLs: userinfo (https://user:TOKEN@host) and the query/fragment
// (https://host/repo.git?access_token=TOKEN#frag). scp-style SSH
// ("git@host:path") has no "://" and is left untouched — git@ is a conventional
// username, not a secret, and it has no query/fragment grammar.
function sanitizeRemoteUrl(url) {
  let out = url.replace(/^([a-zA-Z][a-zA-Z0-9+.-]*:\/\/)[^/@]*@/, '$1');
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(out)) {
    out = out.replace(/[?#].*$/, '');
  }
  return out;
}

export function projectFingerprintInfo(cwd) {
  let basis = null;
  const url = git(cwd, ['config', '--get', 'remote.origin.url']);
  if (url) basis = { type: 'remote', value: 'remote:' + sanitizeRemoteUrl(url) };
  if (!basis) {
    const root = git(cwd, ['rev-parse', '--show-toplevel']);
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
