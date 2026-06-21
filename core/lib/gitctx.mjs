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

// Working-tree files the next agent should know are mid-edit: tracked
// modifications versus HEAD (staged or not) plus untracked files (honouring
// .gitignore). Empty outside a git repo or on a clean tree. When the list
// exceeds `max`, it is capped and a final marker reports how many were dropped
// so truncation is visible instead of silent.
export function changedFiles(cwd, { max = 50 } = {}) {
  const lines = (out) => (out ? out.split('\n').map((s) => s.trim()).filter(Boolean) : []);
  const all = [];
  const seen = new Set();
  for (const f of [
    ...lines(git(cwd, ['diff', '--name-only', 'HEAD'])),
    ...lines(git(cwd, ['ls-files', '--others', '--exclude-standard'])),
  ]) {
    if (!seen.has(f)) { seen.add(f); all.push(f); }
  }
  if (all.length <= max) return all;
  return [...all.slice(0, max), `… (+${all.length - max} more changed files)`];
}
