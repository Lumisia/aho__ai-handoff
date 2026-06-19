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

export function projectFingerprint(cwd) {
  let basis = null;
  const url = git(cwd, ['config', '--get', 'remote.origin.url']);
  if (url) basis = 'remote:' + url;
  if (!basis) {
    const root = git(cwd, ['rev-parse', '--show-toplevel']);
    if (root) { try { basis = 'gitroot:' + realpathSync(root); } catch { basis = 'gitroot:' + root; } }
  }
  if (!basis) {
    try { basis = 'path:' + realpathSync(cwd); } catch { basis = 'path:' + cwd; }
  }
  return sha256Hex(basis).slice(0, 24);
}
