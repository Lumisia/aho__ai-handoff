import { readFileSync } from 'node:fs';
import { handoffNoticeStatePath } from '../lib/paths.mjs';
import { writeFileAtomic, acquireLock, releaseLock } from '../lib/fsx.mjs';
import { projectFingerprint } from '../lib/fingerprint.mjs';
import { t } from '../lib/i18n.mjs';
import { findPendingCapsule } from './store.mjs';
import { injectedTaskIdFor } from './inject-track.mjs';

// Notice markers older than a capsule's own lifetime can never match a live
// capsule, so they are pruned on every write to keep the map bounded.
const TTL_MS = 24 * 60 * 60 * 1000;

function key(fingerprint, sessionId) { return `${fingerprint}:${sessionId || 'unknown'}`; }

function readNoticeState() {
  try { return JSON.parse(readFileSync(handoffNoticeStatePath(), 'utf8')); }
  catch { return { notified: {} }; }
}

// Mutate the notice map under a lock. Best-effort: a contended lock skips the
// write rather than throwing on the fire-and-forget prompt hook path.
function writeNoticeState(mutate, { now = Date.now() } = {}) {
  const path = handoffNoticeStatePath();
  const lock = acquireLock(`${path}.lock`, { now });
  if (!lock) return false;
  try {
    const state = readNoticeState();
    if (!state.notified || typeof state.notified !== 'object') state.notified = {};
    for (const [k, v] of Object.entries(state.notified)) {
      if (!v || typeof v.at !== 'number' || now - v.at > TTL_MS) delete state.notified[k];
    }
    mutate(state.notified);
    writeFileAtomic(path, JSON.stringify(state, null, 2) + '\n');
    return true;
  } finally { releaseLock(lock); }
}

// Decide whether a live session should be told about a newer pending capsule it
// has not yet pulled. SessionStart injects a capsule only once, so a peer
// checkpoint created mid-session never reaches a running session on its own —
// this surfaces it on the next prompt. Pure read; never claims or consumes.
// Returns { notify, reason?, fingerprint?, sessionId?, taskId?, capsule? }.
export function findNewerPending({ input = {}, agent, now = Date.now() }) {
  const cwd = input.cwd || process.cwd();
  const sessionId = input.session_id;
  // No id means no per-session marker (see inject-track), so we can neither tell
  // what this session already saw nor dedupe the nudge — stay silent.
  if (!sessionId) return { notify: false, reason: 'no-session' };
  const fingerprint = projectFingerprint(cwd);
  const pending = findPendingCapsule(fingerprint, { now });
  if (!pending || !pending.capsule) return { notify: false, reason: 'no-pending' };
  // Never nudge about a capsule handed to the peer agent.
  if (pending.capsule.target?.agent !== agent) return { notify: false, reason: 'not-target-agent' };
  // The capsule SessionStart already put in this session's context is not "new".
  if (pending.taskId === injectedTaskIdFor({ fingerprint, sessionId })) {
    return { notify: false, reason: 'already-injected' };
  }
  const notified = readNoticeState().notified?.[key(fingerprint, sessionId)]?.taskIds || [];
  if (notified.includes(pending.taskId)) return { notify: false, reason: 'already-notified' };
  return { notify: true, fingerprint, sessionId, taskId: pending.taskId, capsule: pending.capsule };
}

// Remember that `taskId` was surfaced to `sessionId` so the same capsule is not
// re-announced on every subsequent prompt. Returns true only if persisted.
export function recordNotified({ fingerprint, sessionId, taskId, now = Date.now() }) {
  if (!sessionId || !taskId) return false;
  const k = key(fingerprint, sessionId);
  return writeNoticeState((notified) => {
    const entry = notified[k] && Array.isArray(notified[k].taskIds) ? notified[k] : { taskIds: [], at: now };
    if (!entry.taskIds.includes(taskId)) entry.taskIds.push(taskId);
    entry.at = now;
    notified[k] = entry;
  }, { now });
}

// A token-lean banner with the pending capsule's key info, so the session learns
// what is waiting without having to pull it first. Field labels stay fixed
// (machine-stable); only the heading and call to action are localized.
export function renderPendingNotice(cap, locale = 'en') {
  const task = cap.task || {};
  const project = cap.project || {};
  const lines = [
    `[${t('notice.newer_pending', {}, locale)}]`,
    `goal: ${task.goal || ''}`,
    `from: ${cap.source && cap.source.agent} → ${cap.target && cap.target.agent}`,
    `branch: ${project.git_branch || ''} @ ${project.git_head || ''}`,
  ];
  if ((task.next_actions || []).length) lines.push(`next_actions: ${task.next_actions.join('; ')}`);
  if ((task.open_issues || []).length) lines.push(`open_issues: ${task.open_issues.join('; ')}`);
  if (cap.created_at) lines.push(`created: ${cap.created_at}`);
  lines.push('', t('notice.newer_pending_action', { taskId: cap.task_id || '' }, locale));
  return lines.join('\n');
}
