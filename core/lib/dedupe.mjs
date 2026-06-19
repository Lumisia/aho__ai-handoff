import { sha256OfJson } from './hash.mjs';

export function dedupeKey(parts) {
  const { source, windowDuration, resetsAt, sessionId, projectFingerprint, threshold } = parts || {};
  return sha256OfJson({ source, windowDuration, resetsAt, sessionId, projectFingerprint, threshold }).slice(0, 16);
}

export function hasSeen(state, key) {
  return !!(state && state.seen && state.seen[key]);
}

export function markSeen(state, key, now = Date.now()) {
  const base = state || {};
  return { ...base, seen: { ...(base.seen || {}), [key]: now } };
}
