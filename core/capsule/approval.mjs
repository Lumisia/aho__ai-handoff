import { join } from 'node:path';
import { readFileSync } from 'node:fs';
import { handoffDir } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock } from '../lib/fsx.mjs';

function approvalPath(fingerprint) { return join(handoffDir(fingerprint), 'approval-state.json'); }

function readApprovals(fingerprint) {
  try { return JSON.parse(readFileSync(approvalPath(fingerprint), 'utf8')); }
  catch { return { approvals: {} }; }
}

function mutate(fingerprint, fn, now) {
  const path = approvalPath(fingerprint);
  const lock = acquireLock(`${path}.lock`, { now });
  if (!lock) throw new Error('approval state is locked');
  try {
    const state = readApprovals(fingerprint);
    const result = fn(state);
    writeFileAtomic(path, JSON.stringify(state, null, 2) + '\n');
    return result;
  } finally {
    releaseLock(lock);
  }
}

export function saveApproval({ fingerprint, key, context, now = Date.now() }) {
  return mutate(fingerprint, (state) => {
    const entry = { key, status: 'AWAITING_USER', context, updated_at: now };
    state.approvals[key] = entry;
    return entry;
  }, now);
}

export function findApproval(fingerprint, { key } = {}) {
  const entries = Object.values(readApprovals(fingerprint).approvals || {})
    .filter((entry) => entry.status === 'AWAITING_USER' && (!key || entry.key === key))
    .sort((a, b) => b.updated_at - a.updated_at);
  return entries[0] || null;
}

export function resolveApproval(fingerprint, { key, decision, now = Date.now() }) {
  const status = decision === 'create' ? 'GENERATING' : decision === 'skip' ? 'SKIPPED' : null;
  if (!status) throw new Error(`invalid approval decision: ${decision}`);
  return mutate(fingerprint, (state) => {
    const current = state.approvals?.[key];
    if (!current || current.status !== 'AWAITING_USER') throw new Error('approval is not awaiting user');
    const resolved = { ...current, status, updated_at: now };
    state.approvals[key] = resolved;
    return resolved;
  }, now);
}
