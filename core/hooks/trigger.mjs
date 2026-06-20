function projectMinutesTo100(samples, usedPercent) {
  if (!Array.isArray(samples) || samples.length < 2) return null;
  const sorted = [...samples].sort((a, b) => a.at - b.at);
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  const dPct = last.usedPercent - first.usedPercent;
  const dMin = (last.at - first.at) / 60000;
  if (dMin <= 0 || dPct <= 0) return null;
  const slope = dPct / dMin; // % per minute
  const remaining = 100 - (typeof usedPercent === 'number' ? usedPercent : last.usedPercent);
  if (remaining <= 0) return 0;
  return remaining / slope;
}

export function evaluateTrigger({ usedPercent, threshold, mode, deduped, samples, burnRate, now = Date.now() }) {
  if (mode === 'off') return { action: 'none', reason: 'off' };
  if (typeof usedPercent !== 'number') return { action: 'none', reason: 'unknown' };
  const fire = (reason) => (deduped ? { action: 'none', reason: 'deduped' } : { action: mode === 'auto' ? 'create' : 'ask', reason });
  if (usedPercent >= threshold) return fire('threshold');
  if (burnRate && burnRate.enabled) {
    const eta = projectMinutesTo100(samples, usedPercent);
    if (eta == null) return { action: 'none', reason: 'insufficient-samples' };
    if (eta <= (burnRate.runwayMinutes ?? 30)) return fire('burn-rate');
  }
  return { action: 'none', reason: 'below' };
}
