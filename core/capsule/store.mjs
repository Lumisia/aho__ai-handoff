import { join } from 'node:path';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { handoffDir } from '../lib/paths.mjs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { sha256Hex } from '../lib/hash.mjs';

function taskDir(fingerprint, taskId) { return join(handoffDir(fingerprint), taskId); }

export function readState(statePath) {
  try { return JSON.parse(readFileSync(statePath, 'utf8')); } catch { return {}; }
}

export function writeState(statePath, obj) {
  writeFileAtomic(statePath, JSON.stringify(obj, null, 2) + '\n');
}

export function publishCapsule(fingerprint, capsule, { status = 'AVAILABLE', now = Date.now() } = {}) {
  const dir = taskDir(fingerprint, capsule.task_id);
  const capsulePath = join(dir, 'capsule.json');
  const shaPath = join(dir, 'capsule.sha256');
  const statePath = join(dir, 'state.json');
  const text = JSON.stringify(capsule, null, 2) + '\n';
  writeFileAtomic(capsulePath, text);
  writeFileAtomic(shaPath, sha256Hex(text) + '\n');
  writeState(statePath, { status, task_id: capsule.task_id, updated_at: now });
  return { dir, capsulePath, statePath };
}

const PENDING = new Set(['AVAILABLE', 'DEGRADED_AVAILABLE']);

export function findPendingCapsule(fingerprint) {
  const hd = handoffDir(fingerprint);
  if (!existsSync(hd)) return null;
  let best = null;
  let bestMtime = -Infinity;
  for (const name of readdirSync(hd)) {
    const statePath = join(hd, name, 'state.json');
    if (!existsSync(statePath)) continue;
    const state = readState(statePath);
    if (!PENDING.has(state.status)) continue;
    const m = statSync(statePath).mtimeMs;
    if (m > bestMtime) { bestMtime = m; best = { taskId: name, statePath, state }; }
  }
  if (!best) return null;
  let capsule = null;
  try { capsule = JSON.parse(readFileSync(join(hd, best.taskId, 'capsule.json'), 'utf8')); } catch {}
  return { ...best, capsule };
}
