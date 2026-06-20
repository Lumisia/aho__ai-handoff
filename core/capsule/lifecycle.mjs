export const STATES = {
  IDLE: 'IDLE',
  AWAITING_USER: 'AWAITING_USER',
  GENERATING: 'GENERATING',
  AVAILABLE: 'AVAILABLE',
  DEGRADED_AVAILABLE: 'DEGRADED_AVAILABLE',
  FAILED: 'FAILED',
  CLAIMED: 'CLAIMED',
  CONSUMED: 'CONSUMED',
  REJECTED: 'REJECTED',
  SKIPPED: 'SKIPPED',
  EXPIRED: 'EXPIRED',
};

const T = {
  IDLE: ['GENERATING', 'AWAITING_USER', 'IDLE'],
  AWAITING_USER: ['GENERATING', 'SKIPPED', 'EXPIRED'],
  GENERATING: ['AVAILABLE', 'DEGRADED_AVAILABLE', 'FAILED'],
  AVAILABLE: ['CLAIMED', 'EXPIRED'],
  DEGRADED_AVAILABLE: ['CLAIMED', 'EXPIRED'],
  CLAIMED: ['CONSUMED', 'AVAILABLE', 'DEGRADED_AVAILABLE', 'REJECTED'],
  CONSUMED: [],
  FAILED: [],
  REJECTED: [],
  SKIPPED: [],
  EXPIRED: [],
};

export function canTransition(from, to) {
  return (T[from] || []).includes(to);
}

export function transition(from, to) {
  if (!canTransition(from, to)) throw new Error(`illegal transition ${from} -> ${to}`);
  return to;
}
