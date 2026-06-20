# Task 4.2 Report — Burn-rate logic in evaluateTrigger + config keys

## Per-file changes

### `core/hooks/trigger.mjs`
- Added `projectMinutesTo100(samples, usedPercent)` helper: sorts samples by `at`, computes slope (%/min), returns minutes until 100% or `null` if < 2 samples / non-positive delta.
- Rewrote `evaluateTrigger` signature to accept `{ usedPercent, threshold, mode, deduped, samples, burnRate, now = Date.now() }`.
- New logic (only when `burnRate.enabled` is true and `usedPercent < threshold`):
  - If `projectMinutesTo100` returns `null` → `{ action: 'none', reason: 'insufficient-samples' }`
  - If eta <= `burnRate.runwayMinutes ?? 30` → fires via `fire('burn-rate')` (respects deduped flag and auto/ask mode)
- Deduped check unified into the `fire()` helper so it applies to both `threshold` and `burn-rate` reasons.
- `now` accepted in signature for interface symmetry but not used in linear projection.

### `core/lib/config-edit.mjs`
- Added two new `CONFIG_KEYS` entries after the existing `five_hour.mode` key:
  - `'triggers.five_hour.burn_rate.enabled': { type: 'boolean' }`
  - `'triggers.five_hour.burn_rate.runway_minutes': { type: 'number', min: 5, max: 120 }`

### `config/defaults.json`
- Extended `triggers.five_hour` block to include `"burn_rate": { "enabled": false, "runway_minutes": 30 }`.
- File remains valid JSON.

### `tests/trigger.test.mjs`
- Kept all 6 existing tests untouched.
- Added 5 new tests per the brief:
  1. `static threshold still fires (regression)` — verifies backward compat
  2. `burn-rate fires below threshold when exhaustion is within runway`
  3. `burn-rate does not fire when projection is beyond runway`
  4. `burn-rate disabled => static only`
  5. `burn-rate enabled but too few samples => insufficient-samples`

## Test commands + output

### Step 2 — failing tests (before implementation):
```
node --test tests/trigger.test.mjs
# pass 9, fail 2 (burn-rate fires... and insufficient-samples)
```

### Step 5 — trigger tests pass:
```
node --test tests/trigger.test.mjs
# tests 11, pass 11, fail 0
```

### Full suite:
```
node --test
# tests 166, pass 165, fail 0, skipped 1
# (the 1 skip is the pre-existing "reads live rate limit from codex app-server" SKIP)
```

## Concerns

None. All 165 tests pass, backward compatibility preserved (old callers without `samples`/`burnRate` behave identically to before). `config/defaults.json` is valid JSON. `now` is in the signature per the spec but unused in projection math (as noted in the brief).
