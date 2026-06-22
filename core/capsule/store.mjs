import { join, basename, dirname } from 'node:path';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { handoffDir } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock, ownsLock } from '../lib/fsx.mjs';
import { sha256Hex } from '../lib/hash.mjs';
import { transition } from './lifecycle.mjs';
import { validateCapsule, capsulePayloadHash } from './create.mjs';
import { refreshProjectIndex } from '../project/index-store.mjs';
import { appendHistory } from './history.mjs';

function taskDir(fingerprint, taskId) { return join(handoffDir(fingerprint), taskId); }
const PENDING = new Set(['AVAILABLE', 'DEGRADED_AVAILABLE']);

// The capsule's own TTL (expires_at), in epoch ms, or null when absent/unparsable.
function capsuleExpiry(hd, name) {
  try {
    const cap = JSON.parse(readFileSync(join(hd, name, 'capsule.json'), 'utf8'));
    const t = cap && cap.expires_at ? Date.parse(cap.expires_at) : NaN;
    return Number.isFinite(t) ? t : null;
  } catch { return null; }
}

export function readState(statePath) {
  // A malformed state.json that parses to a non-object (literal `null`, an array,
  // a primitive) must not reach callers as-is: `state.status` would throw and
  // crash the read paths (findPendingCapsule, doctor). Collapse anything that is
  // not a plain object to {} so it is simply treated as "no usable status".
  try {
    const v = JSON.parse(readFileSync(statePath, 'utf8'));
    return v && typeof v === 'object' && !Array.isArray(v) ? v : {};
  } catch { return {}; }
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
    if (existsSync(capsulePath) && existsSync(statePath)) {
      // A FINALIZED capsule (both files present) is immutable: idempotent for
      // identical bytes, rejected otherwise.
      if (readFileSync(capsulePath, 'utf8') !== text) {
        throw new Error(`capsule already published: ${capsule.task_id}`);
      }
      return { dir, capsulePath, statePath };
    }
    // No state.json → either a brand-new publish or an ORPHANED partial one
    // (capsule.json written but the publish crashed before finalizing). Each
    // retry builds a fresh capsule_id so the bytes never match a byte-equality
    // check — (re)write all artifacts to complete the publish instead of wedging
    // every future retry on "already published".
    writeFileAtomic(capsulePath, text);
    writeFileAtomic(shaPath, sha256Hex(text) + '\n');
    refreshProjectIndex(fingerprint, capsule.task_id, { now });
    writeState(statePath, { status, task_id: capsule.task_id, updated_at: now });
    appendHistory(fingerprint, {
      event: 'created', taskId: capsule.task_id,
      agent: capsule.source?.agent, source: capsule.source?.agent, target: capsule.target?.agent,
      trigger: capsule.trigger?.type, observed_percent: capsule.trigger?.observed_percent ?? null,
    }, { now });
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

// Recover a CLAIMED capsule whose lease has expired, restoring its prior
// pending quality. The recovery runs UNDER the claim lock: acquiring it proves
// no live claimant holds the lease (acquireLock refuses a non-expired lock and
// atomically reclaims an expired one). Rewriting the state + deleting the
// lockfile WITHOUT the lock is a TOCTOU — a fresh claimant could acquire the
// lock between our read and unlink and have its live lock destroyed.
function recoverExpiredClaim(hd, name, statePath, now) {
  const lock = acquireLock(join(hd, name, '.claim.lock'), { now });
  if (!lock) return; // a live claimant holds it — leave it CLAIMED
  try {
    const fresh = readState(statePath);
    if (fresh.status !== 'CLAIMED' || typeof fresh.claim_expires_at !== 'number' || fresh.claim_expires_at > now) {
      return; // consumed or already recovered before we got the lock
    }
    const target = fresh.previous_status === 'DEGRADED_AVAILABLE' ? 'DEGRADED_AVAILABLE' : 'AVAILABLE';
    const recovered = { ...fresh, status: transition('CLAIMED', target), recovered_at: now };
    delete recovered.claim_expires_at;
    delete recovered.previous_status;
    writeState(statePath, recovered);
  } finally {
    releaseLock(lock);
  }
}

// Expire a past-TTL pending capsule UNDER ITS CLAIM LOCK, mirroring
// recoverExpiredClaim. Writing EXPIRED with only the publish lock held could
// clobber a concurrent claimCapsule (which writes CLAIMED under the claim lock),
// leaving a claimant about to consume an EXPIRED state. Acquiring the claim lock
// first proves no live claim is in flight; if one is, we skip and let it finish.
function expireTtlCapsule(hd, name, statePath, now) {
  const lock = acquireLock(join(hd, name, '.claim.lock'), { now });
  if (!lock) return;
  try {
    const fresh = readState(statePath);
    if (!PENDING.has(fresh.status)) return; // claimed/consumed/expired since the scan
    writeState(statePath, {
      ...fresh, status: transition(fresh.status, 'EXPIRED'), expired_at: now, expiration_reason: 'ttl',
    });
  } finally { releaseLock(lock); }
}

export function findPendingCapsule(fingerprint, { now = Date.now() } = {}) {
  const hd = handoffDir(fingerprint);
  if (!existsSync(hd)) return null;

  // Phase 1: lazily recover lease-expired CLAIMED capsules, each under its own
  // claim lock (see recoverExpiredClaim).
  for (const name of readdirSync(hd)) {
    const statePath = join(hd, name, 'state.json');
    if (!existsSync(statePath)) continue;
    const state = readState(statePath);
    if (state.status === 'CLAIMED' && typeof state.claim_expires_at === 'number' && state.claim_expires_at <= now) {
      recoverExpiredClaim(hd, name, statePath, now);
    }
  }

  // Phase 2: select the newest pending capsule AND expire its superseded
  // siblings UNDER the publish lock. publishCapsule expires siblings under the
  // same lock, so selecting + expiring here can never race a concurrent publish
  // (which would otherwise let this read scan expire a freshly published capsule
  // and silently drop the handoff). If the lock is held — a publish is in
  // flight — we still select read-only but skip expiration: selection prefers
  // the newest by `order`, so leaving extra pendings is harmless and loses
  // nothing.
  const publishLock = acquireLock(join(handoffDir(fingerprint), '.publish.lock'), { now });
  let best = null;
  try {
    let bestOrder = -Infinity;
    for (const name of readdirSync(hd)) {
      const statePath = join(hd, name, 'state.json');
      if (!existsSync(statePath)) continue;
      const state = readState(statePath);
      if (!PENDING.has(state.status)) continue;
      // TTL: a pending capsule past its expires_at is dead even though state
      // still says AVAILABLE. Persist the EXPIRED transition under the publish
      // lock so status/recent/preview agree, then skip it. If a publish holds the
      // lock we only skip selecting it (the owner won't pick an expired one).
      const expiry = capsuleExpiry(hd, name);
      if (expiry != null && expiry <= now) {
        if (publishLock) expireTtlCapsule(hd, name, statePath, now);
        continue;
      }
      const order = typeof state.updated_at === 'number' ? state.updated_at : statSync(statePath).mtimeMs;
      if (order > bestOrder) { bestOrder = order; best = { taskId: name, statePath, state }; }
    }
    if (best && publishLock) expireOtherPending(fingerprint, best.taskId, { now });
  } finally {
    if (publishLock) releaseLock(publishLock);
  }
  if (!best) return null;
  let capsule = null;
  try { capsule = JSON.parse(readFileSync(join(hd, best.taskId, 'capsule.json'), 'utf8')); } catch {}
  return { ...best, capsule };
}

export function claimCapsule(fingerprint, taskId, { leaseMs = 30000, now = Date.now(), claimedBy = null } = {}) {
  const dir = taskDir(fingerprint, taskId);
  const statePath = join(dir, 'state.json');
  const lock = acquireLock(join(dir, '.claim.lock'), { leaseMs, now });
  if (!lock) return null;
  try {
    const st = readState(statePath);
    const next = transition(st.status, 'CLAIMED');
    // Remember whether the capsule was AVAILABLE or DEGRADED_AVAILABLE so a
    // release or lease-expiry restores the original quality, not always AVAILABLE.
    const state = {
      ...st, status: next, previous_status: st.status, claimed_at: now, claim_expires_at: now + leaseMs,
    };
    // Attribute the claim so state alone shows which agent/session took it.
    if (claimedBy) state.claimed_by = claimedBy;
    writeState(statePath, state);
    return { lock, statePath };
  } catch {
    releaseLock(lock);
    return null;
  }
}

export function consumeCapsule(claim, { now = Date.now(), consumedBy = null } = {}) {
  if (!ownsLock(claim?.lock)) throw new Error('stale claim cannot consume capsule');
  const st = readState(claim.statePath);
  const next = { ...st, status: transition(st.status, 'CONSUMED'), consumed_at: now };
  // Record which agent/session actually consumed the capsule so "who read it" is
  // provable from state, not merely inferred from timing.
  if (consumedBy) next.consumed_by = consumedBy;
  delete next.claim_expires_at;
  writeState(claim.statePath, next);
  const fp = basename(dirname(dirname(dirname(claim.statePath))));
  appendHistory(fp, {
    event: 'resumed', taskId: next.task_id ?? st.task_id,
    agent: consumedBy?.agent, session_id: consumedBy?.session_id,
  }, { now });
  releaseLock(claim.lock);
}

export function releaseClaim(claim) {
  if (!ownsLock(claim?.lock)) throw new Error('stale claim cannot release capsule');
  const st = readState(claim.statePath);
  const target = st.previous_status === 'DEGRADED_AVAILABLE' ? 'DEGRADED_AVAILABLE' : 'AVAILABLE';
  const next = { ...st, status: transition(st.status, target) };
  delete next.claim_expires_at;
  delete next.previous_status;
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
