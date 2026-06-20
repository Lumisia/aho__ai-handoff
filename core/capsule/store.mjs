import { join } from 'node:path';
import { existsSync, readFileSync, readdirSync, statSync, unlinkSync } from 'node:fs';
import { handoffDir } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock, ownsLock } from '../lib/fsx.mjs';
import { sha256Hex } from '../lib/hash.mjs';
import { transition } from './lifecycle.mjs';
import { validateCapsule, capsulePayloadHash } from './create.mjs';
import { refreshProjectIndex } from '../project/index-store.mjs';

function taskDir(fingerprint, taskId) { return join(handoffDir(fingerprint), taskId); }
const PENDING = new Set(['AVAILABLE', 'DEGRADED_AVAILABLE']);

export function readState(statePath) {
  try { return JSON.parse(readFileSync(statePath, 'utf8')); } catch { return {}; }
}

export function writeState(statePath, obj) {
  writeFileAtomic(statePath, JSON.stringify(obj, null, 2) + '\n');
}

export function publishCapsule(fingerprint, capsule, { status = 'AVAILABLE', now = Date.now() } = {}) {
  const validation = validateCapsule(capsule);
  const integrityOk = capsule.integrity?.payload_sha256 === `sha256:${capsulePayloadHash(capsule)}`;
  if (!validation.valid || !integrityOk || capsule.project?.fingerprint !== fingerprint) {
    const reasons = [...validation.errors];
    if (!integrityOk) reasons.push('integrity payload_sha256 mismatch');
    if (capsule.project?.fingerprint !== fingerprint) {
      reasons.push(`project fingerprint mismatch (capsule=${capsule.project?.fingerprint ?? 'none'}, expected=${fingerprint})`);
    }
    throw new Error(`invalid capsule: ${reasons.join('; ') || 'schema, integrity, or project fingerprint mismatch'}`);
  }
  const dir = taskDir(fingerprint, capsule.task_id);
  const capsulePath = join(dir, 'capsule.json');
  const shaPath = join(dir, 'capsule.sha256');
  const statePath = join(dir, 'state.json');
  const text = JSON.stringify(capsule, null, 2) + '\n';
  const publishLock = acquireLock(join(handoffDir(fingerprint), '.publish.lock'), { now });
  if (!publishLock) throw new Error(`capsule publish is locked: ${capsule.task_id}`);
  try {
    if (existsSync(capsulePath)) {
      const existing = readFileSync(capsulePath, 'utf8');
      if (existing !== text) throw new Error(`capsule already published: ${capsule.task_id}`);
      if (!existsSync(statePath)) {
        refreshProjectIndex(fingerprint, capsule.task_id, { now });
        writeState(statePath, { status, task_id: capsule.task_id, updated_at: now });
      }
      return { dir, capsulePath, statePath };
    }
    writeFileAtomic(capsulePath, text);
    writeFileAtomic(shaPath, sha256Hex(text) + '\n');
    refreshProjectIndex(fingerprint, capsule.task_id, { now });
    writeState(statePath, { status, task_id: capsule.task_id, updated_at: now });
    expireOtherPending(fingerprint, capsule.task_id, { now });
    return { dir, capsulePath, statePath };
  } finally {
    releaseLock(publishLock);
  }
}

function expireOtherPending(fingerprint, keepTaskId, { now = Date.now() } = {}) {
  const hd = handoffDir(fingerprint);
  if (!existsSync(hd)) return;
  for (const name of readdirSync(hd)) {
    if (name === keepTaskId) continue;
    const statePath = join(hd, name, 'state.json');
    if (!existsSync(statePath)) continue;
    const state = readState(statePath);
    if (!PENDING.has(state.status)) continue;
    writeState(statePath, {
      ...state, status: transition(state.status, 'EXPIRED'), expired_at: now,
      expiration_reason: 'superseded',
    });
  }
}

export function findPendingCapsule(fingerprint, { now = Date.now() } = {}) {
  const hd = handoffDir(fingerprint);
  if (!existsSync(hd)) return null;
  let best = null;
  let bestOrder = -Infinity;
  for (const name of readdirSync(hd)) {
    const statePath = join(hd, name, 'state.json');
    if (!existsSync(statePath)) continue;
    let state = readState(statePath);
    if (state.status === 'CLAIMED' && typeof state.claim_expires_at === 'number' && state.claim_expires_at <= now) {
      state = { ...state, status: transition('CLAIMED', 'AVAILABLE'), recovered_at: now };
      delete state.claim_expires_at;
      writeState(statePath, state);
      try { unlinkSync(join(hd, name, '.claim.lock')); } catch {}
    }
    if (!PENDING.has(state.status)) continue;
    const order = typeof state.updated_at === 'number' ? state.updated_at : statSync(statePath).mtimeMs;
    if (order > bestOrder) { bestOrder = order; best = { taskId: name, statePath, state }; }
  }
  if (!best) return null;
  expireOtherPending(fingerprint, best.taskId, { now });
  let capsule = null;
  try { capsule = JSON.parse(readFileSync(join(hd, best.taskId, 'capsule.json'), 'utf8')); } catch {}
  return { ...best, capsule };
}

export function claimCapsule(fingerprint, taskId, { leaseMs = 30000, now = Date.now() } = {}) {
  const dir = taskDir(fingerprint, taskId);
  const statePath = join(dir, 'state.json');
  const lock = acquireLock(join(dir, '.claim.lock'), { leaseMs, now });
  if (!lock) return null;
  try {
    const st = readState(statePath);
    const next = transition(st.status, 'CLAIMED');
    writeState(statePath, { ...st, status: next, claimed_at: now, claim_expires_at: now + leaseMs });
    return { lock, statePath };
  } catch {
    releaseLock(lock);
    return null;
  }
}

export function consumeCapsule(claim, { now = Date.now() } = {}) {
  if (!ownsLock(claim?.lock)) throw new Error('stale claim cannot consume capsule');
  const st = readState(claim.statePath);
  const next = { ...st, status: transition(st.status, 'CONSUMED'), consumed_at: now };
  delete next.claim_expires_at;
  writeState(claim.statePath, next);
  releaseLock(claim.lock);
}

export function releaseClaim(claim) {
  if (!ownsLock(claim?.lock)) throw new Error('stale claim cannot release capsule');
  const st = readState(claim.statePath);
  const next = { ...st, status: transition(st.status, 'AVAILABLE') };
  delete next.claim_expires_at;
  writeState(claim.statePath, next);
  releaseLock(claim.lock);
}

export function rejectCapsule(claim, { now = Date.now() } = {}) {
  if (!ownsLock(claim?.lock)) throw new Error('stale claim cannot reject capsule');
  const st = readState(claim.statePath);
  const next = { ...st, status: transition(st.status, 'REJECTED'), rejected_at: now };
  delete next.claim_expires_at;
  writeState(claim.statePath, next);
  releaseLock(claim.lock);
}

export function verifyStoredCapsule(fingerprint, taskId, {
  expectedAgent, currentGitHead, now = Date.now(),
} = {}) {
  const dir = taskDir(fingerprint, taskId);
  const capsulePath = join(dir, 'capsule.json');
  const shaPath = join(dir, 'capsule.sha256');
  const statePath = join(dir, 'state.json');
  const errors = [];
  const warnings = [];
  let text = '';
  let capsule = null;

  try { text = readFileSync(capsulePath, 'utf8'); capsule = JSON.parse(text); }
  catch { errors.push('capsule-unreadable'); }
  if (capsule) {
    if (!validateCapsule(capsule).valid) errors.push('schema-invalid');
    if (capsule.integrity?.payload_sha256 !== `sha256:${capsulePayloadHash(capsule)}`) {
      errors.push('payload-integrity-mismatch');
    }
    let storedSha = '';
    try { storedSha = readFileSync(shaPath, 'utf8').trim(); } catch {}
    if (!storedSha || storedSha !== sha256Hex(text)) errors.push('external-sha-mismatch');
    if (capsule.project?.fingerprint !== fingerprint) errors.push('project-fingerprint-mismatch');
    if (expectedAgent && capsule.target?.agent !== expectedAgent) errors.push('target-agent-mismatch');
    if (capsule.expires_at && Date.parse(capsule.expires_at) <= now) errors.push('capsule-expired');
    if (currentGitHead && capsule.project?.git_head && capsule.project.git_head !== currentGitHead) {
      warnings.push('git-head-mismatch');
    }
  }
  return { valid: errors.length === 0, errors, warnings, capsule, capsulePath, statePath };
}
