import { readFileSync } from 'node:fs';
import { handoffInjectStatePath } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock } from '../lib/fsx.mjs';
import { projectFingerprint } from '../lib/fingerprint.mjs';
import { findPendingCapsule, claimCapsule, consumeCapsule } from './store.mjs';

// Injected markers older than a capsule's own lifetime can never match a live
// capsule, so they are pruned on every write to keep the map bounded.
const TTL_MS = 24 * 60 * 60 * 1000;

function key(fingerprint, sessionId) { return `${fingerprint}:${sessionId || 'unknown'}`; }

function readInjectState() {
  try { return JSON.parse(readFileSync(handoffInjectStatePath(), 'utf8')); }
  catch { return { injected: {} }; }
}

// Mutate the inject map under a lock. Best-effort: if the lock is contended we
// skip the write rather than throw on the fire-and-forget hook path.
function writeInjectState(mutate, { now = Date.now() } = {}) {
  const path = handoffInjectStatePath();
  const lock = acquireLock(`${path}.lock`, { now });
  if (!lock) return false;
  try {
    const state = readInjectState();
    if (!state.injected || typeof state.injected !== 'object') state.injected = {};
    for (const [k, v] of Object.entries(state.injected)) {
      if (!v || typeof v.at !== 'number' || now - v.at > TTL_MS) delete state.injected[k];
    }
    mutate(state.injected);
    writeFileAtomic(path, JSON.stringify(state, null, 2) + '\n');
    return true;
  } finally { releaseLock(lock); }
}

// Record that `taskId` was injected (read-only) into `sessionId` so the
// session's first prompt — proof the session is live — can consume it. The
// capsule status is untouched here: an ephemeral session that never prompts
// simply leaves the capsule pending for the next session.
export function recordInject({ fingerprint, sessionId, taskId, now = Date.now() }) {
  // Without a session id we cannot bind the injection to THIS session: a later
  // session that also lacks an id would collide on the 'unknown' key and consume
  // a capsule it never received. Refuse to record — the capsule stays pending and
  // is re-injected to the next identifiable session. Returns true only if the
  // marker was actually persisted (false on missing id or lock contention).
  if (!sessionId) return false;
  return writeInjectState((injected) => { injected[key(fingerprint, sessionId)] = { taskId, at: now }; }, { now });
}

// The taskId SessionStart injected (read-only) into this session, or undefined
// once it has been consumed/pruned. Lets a separate concern (the newer-pending
// nudge) tell "the capsule this session already saw" apart from a fresh one.
export function injectedTaskIdFor({ fingerprint, sessionId }) {
  if (!sessionId) return undefined;
  return readInjectState().injected?.[key(fingerprint, sessionId)]?.taskId;
}

// Consume the capsule this session was injected, on the session's first prompt.
// No marker (capsule never injected to this session) or a pending capsule that
// does not match the injected task → no-op, so a capsule the model never saw is
// never silently consumed. Returns { consumed, taskId? }.
export function consumeOnPrompt({ input = {}, agent, now = Date.now() }) {
  const cwd = input.cwd || process.cwd();
  const fingerprint = projectFingerprint(cwd);
  const sessionId = input.session_id;
  // A session with no id was never recordable (see recordInject), so it can have
  // no marker of its own — refuse rather than fall back to the shared 'unknown'
  // key and risk consuming another session's capsule.
  if (!sessionId) return { consumed: false, reason: 'no-session' };
  const k = key(fingerprint, sessionId);
  const entry = readInjectState().injected?.[k];
  if (!entry) return { consumed: false, reason: 'not-injected' };

  const pending = findPendingCapsule(fingerprint, { now });
  // Only consume a capsule that is still the one we injected AND is addressed to
  // this agent — never consume a peer's handoff.
  if (!pending || pending.taskId !== entry.taskId || pending.capsule?.target?.agent !== agent) {
    writeInjectState((injected) => { delete injected[k]; }, { now });
    return { consumed: false, reason: 'not-pending' };
  }

  const consumedBy = { agent, session_id: sessionId };
  const claim = claimCapsule(fingerprint, pending.taskId, { now, claimedBy: consumedBy });
  if (!claim) return { consumed: false, reason: 'claim-failed' };
  consumeCapsule(claim, { now, consumedBy });
  writeInjectState((injected) => { delete injected[k]; }, { now });
  return { consumed: true, taskId: pending.taskId };
}
