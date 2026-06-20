# Task 4.2 — Burn-rate logic in evaluateTrigger + config keys

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Modify: `core/hooks/trigger.mjs`
- Modify: `core/lib/config-edit.mjs`, `config/defaults.json`
- Test: `tests/trigger.test.mjs` (create; if it already exists, ADD these tests, keep existing ones)

## Current `core/hooks/trigger.mjs`
```js
export function evaluateTrigger({ usedPercent, threshold, mode, deduped }) {
  if (mode === 'off') return { action: 'none', reason: 'off' };
  if (typeof usedPercent !== 'number') return { action: 'none', reason: 'unknown' };
  if (usedPercent < threshold) return { action: 'none', reason: 'below' };
  if (deduped) return { action: 'none', reason: 'deduped' };
  return { action: mode === 'auto' ? 'create' : 'ask', reason: 'threshold' };
}
```

## Produces
- `evaluateTrigger({ usedPercent, threshold, mode, deduped, samples, burnRate, now })`. `burnRate = { enabled, runwayMinutes }`. New reasons: `burn-rate`, `insufficient-samples`. MUST stay backward compatible: a call WITHOUT `samples`/`burnRate` behaves exactly as today.

## Step 1 — failing tests: `tests/trigger.test.mjs`
```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { evaluateTrigger } from '../core/hooks/trigger.mjs';

const base = { threshold: 95, mode: 'ask', deduped: false };

test('static threshold still fires (regression)', () => {
  assert.equal(evaluateTrigger({ ...base, usedPercent: 96 }).action, 'ask');
  assert.equal(evaluateTrigger({ ...base, usedPercent: 50 }).action, 'none');
});

test('burn-rate fires below threshold when exhaustion is within runway', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 10 * 60000 }, { usedPercent: 80, at: now }]; // +20%/10min => 100% in 10min
  const ev = evaluateTrigger({ ...base, usedPercent: 80, samples, burnRate: { enabled: true, runwayMinutes: 30 }, now });
  assert.equal(ev.action, 'ask');
  assert.equal(ev.reason, 'burn-rate');
});

test('burn-rate does not fire when projection is beyond runway', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 60 * 60000 }, { usedPercent: 62, at: now }]; // slow
  assert.equal(evaluateTrigger({ ...base, usedPercent: 62, samples, burnRate: { enabled: true, runwayMinutes: 30 }, now }).action, 'none');
});

test('burn-rate disabled => static only', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 10 * 60000 }, { usedPercent: 80, at: now }];
  assert.equal(evaluateTrigger({ ...base, usedPercent: 80, samples, burnRate: { enabled: false }, now }).action, 'none');
});

test('burn-rate enabled but too few samples => insufficient-samples', () => {
  assert.equal(evaluateTrigger({ ...base, usedPercent: 80, samples: [], burnRate: { enabled: true, runwayMinutes: 30 }, now: 1 }).reason, 'insufficient-samples');
});
```

## Step 2 — run, expect FAIL
`node --test tests/trigger.test.mjs` → burn-rate cases fail.

## Step 3 — implement `core/hooks/trigger.mjs`
```js
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
```
(`now` is accepted for interface symmetry even though the linear projection does not need it; keep it in the signature.)

## Step 4 — config keys
- In `core/lib/config-edit.mjs` `CONFIG_KEYS`, add:
  - `'triggers.five_hour.burn_rate.enabled': { type: 'boolean' },`
  - `'triggers.five_hour.burn_rate.runway_minutes': { type: 'number', min: 5, max: 120 },`
- In `config/defaults.json`, change the `five_hour` block to include `burn_rate`:
  `"five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask", "burn_rate": { "enabled": false, "runway_minutes": 30 } }`

## Step 5 — run tests
`node --test tests/trigger.test.mjs` → PASS. Then full `node --test` → no regressions (existing stop/cli tests that call evaluateTrigger with the old args must still pass — the new params are optional).

## Step 6 — commit
```
git add -A
git commit -m "feat: add opt-in burn-rate projection to evaluateTrigger"
```

## Global constraints
Node ≥18, zero deps. `node --test` green. Backward compatible signature (old callers unaffected). Keep `config/defaults.json` valid JSON.
