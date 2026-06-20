import { statusFor } from '../hooks/handoff.mjs';

export function statuslineSegment({ usedPercent, cwd, show = true } = {}) {
  if (!show) return '';
  const pct = typeof usedPercent === 'number' ? `${Math.round(usedPercent)}%` : null;
  let pending = 0;
  if (cwd) { try { if (statusFor(cwd).pending) pending = 1; } catch {} }
  const head = pct ? `AH ${pct}` : 'AH';
  return pending ? `${head} · ⏳${pending}` : head;
}
