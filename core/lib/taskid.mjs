import { createHash } from 'node:crypto';
import { canonicalJson } from './hash.mjs';

const B32 = 'abcdefghijklmnopqrstuvwxyz234567'; // RFC4648 lowercase

function base32(buf) {
  let bits = 0, value = 0, out = '';
  for (const b of buf) {
    value = (value << 8) | b;
    bits += 8;
    while (bits >= 5) { out += B32[(value >>> (bits - 5)) & 31]; bits -= 5; }
  }
  if (bits > 0) out += B32[(value << (5 - bits)) & 31];
  return out;
}

export function slugify(text, max = 32) {
  const s = String(text || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, max);
  return s || 'task';
}

export function instanceKey({ explicitTaskKey, agent, sessionId } = {}) {
  if (explicitTaskKey) return 'key:' + String(explicitTaskKey).trim().toLowerCase();
  return `session:${agent || 'unknown'}:${sessionId || 'nosession'}`;
}

export function deriveTaskId({ projectFingerprint, instanceKey: ik, goalSlug }) {
  const digest = createHash('sha256')
    .update(canonicalJson({ v: 1, projectFingerprint, instanceKey: ik }))
    .digest();
  return `t-${slugify(goalSlug)}-${base32(digest).slice(0, 12)}`;
}
