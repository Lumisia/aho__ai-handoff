import { execFileSync } from 'node:child_process';

function git(cwd, args) {
  try {
    return execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
}

export function gitContext(cwd) {
  const head = git(cwd, ['rev-parse', 'HEAD']);
  if (!head) return { is_git: false, branch: null, head: null, dirty: null };
  const branch = git(cwd, ['rev-parse', '--abbrev-ref', 'HEAD']);
  const status = git(cwd, ['status', '--porcelain']);
  return { is_git: true, branch, head: head.slice(0, 12), dirty: !!(status && status.length) };
}

// Working-tree files changed versus HEAD (tracked modifications, staged or not),
// so a handoff tells the next agent which files are mid-edit instead of making it
// re-discover them. Empty outside a git repo or on a clean tree.
export function changedFiles(cwd, { max = 50 } = {}) {
  const out = git(cwd, ['diff', '--name-only', 'HEAD']);
  if (!out) return [];
  return out.split('\n').map((s) => s.trim()).filter(Boolean).slice(0, max);
}
