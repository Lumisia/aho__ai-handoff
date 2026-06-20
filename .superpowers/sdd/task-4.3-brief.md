# Task 4.3 — Wire samples + burn-rate into handleStop

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Modify: `core/hooks/stop.mjs`
- Modify: `core/sensors/claude-statusline.mjs`
- Test: create `tests/burn-rate-stop.test.mjs`

## Consumes (already exists, committed)
- `appendSample(fingerprint, agent, { usedPercent, at }, { max })`, `readSamples(fingerprint, agent)` from `core/sensors/samples.mjs`.
- `evaluateTrigger` now accepts `{ usedPercent, threshold, mode, deduped, samples, burnRate, now }`.
- In `core/hooks/stop.mjs`: `fp` (project fingerprint) and `agent` are already in scope inside `handleStop`; `tcfg = pcfg.triggers.five_hour`; the live trigger path computes `const reading = await readSensor();` then calls `evaluateTrigger({ usedPercent, threshold, mode, deduped })`.
- In `core/sensors/claude-statusline.mjs`: `recordClaudeRateLimit(input, { now })` computes `used` (the percent) and writes a single sample file; it has `input.session_id` and config.

## Step 1 — failing test: create `tests/burn-rate-stop.test.mjs`
```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';
import { appendSample } from '../core/sensors/samples.mjs';
import { projectFingerprint } from '../core/lib/fingerprint.mjs';

test('handleStop fires on burn-rate below threshold when enabled', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-br-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-brp-'));
  const fp = projectFingerprint(cwd);
  const now = 1000 * 60000;
  appendSample(fp, 'codex', { usedPercent: 60, at: now - 10 * 60000 });
  const config = {
    triggers: { five_hour: { enabled: true, threshold_percent: 95, mode: 'ask', burn_rate: { enabled: true, runway_minutes: 30 } } },
    notification: { method: 'off' },
  };
  const readSensor = async () => ({ usedPercent: 80, windowMinutes: 300, resetsAt: null });
  const res = await handleStop({ input: { cwd, session_id: 's' }, config, readSensor, agent: 'codex', now, notifyFn: () => {} });
  assert.equal(res.action, 'ask');
  assert.equal(res.reason, 'burn-rate');
  delete process.env.AI_HANDOFF_ROOT;
});
```

## Step 2 — run, expect FAIL
`node --test tests/burn-rate-stop.test.mjs` → fails (handleStop does not yet consider burn-rate; with usedPercent 80 < threshold 95 it returns action 'none').

## Step 3 — wire into `core/hooks/stop.mjs`
Add import near the other imports: `import { appendSample, readSamples } from '../sensors/samples.mjs';`

In the LIVE trigger path (NOT the `stop_hook_active` branch), right after `const reading = await readSensor();`, add:
```js
  if (reading && typeof reading.usedPercent === 'number') {
    appendSample(fp, agent, { usedPercent: reading.usedPercent, at: now });
  }
```
Then change the `evaluateTrigger({ ... })` call to pass samples + burn-rate config + now:
```js
  const ev = evaluateTrigger({
    usedPercent: reading && reading.usedPercent,
    threshold: tcfg.threshold_percent,
    mode: tcfg.mode,
    deduped: hasSeen(gstate, dkey),
    samples: readSamples(fp, agent),
    burnRate: tcfg.burn_rate && { enabled: tcfg.burn_rate.enabled, runwayMinutes: tcfg.burn_rate.runway_minutes },
    now,
  });
```
(Leave everything else in `handleStop` unchanged.)

## Step 4 — append samples from the Claude status line in `core/sensors/claude-statusline.mjs`
Add imports: `import { appendSample } from './samples.mjs';` and `import { projectFingerprint } from '../lib/fingerprint.mjs';`. In `recordClaudeRateLimit`, just before the final `return true;` (after the sample file is written), add:
```js
  const cwd = input.cwd || input.workspace?.current_dir;
  if (cwd) { try { appendSample(projectFingerprint(cwd), 'claude-code', { usedPercent: used, at: now }); } catch {} }
```

## Step 5 — run tests
`node --test tests/burn-rate-stop.test.mjs` → PASS. Then full `node --test` → no regressions (existing stop tests must still pass; burn_rate may be absent in some test configs — the `tcfg.burn_rate && ...` guard makes burnRate `undefined`, so evaluateTrigger ignores it).

## Step 6 — commit
```
git add -A
git commit -m "feat: feed samples and burn-rate config into handleStop"
```

## Global constraints
Node ≥18, zero deps. `node --test` green. Do not alter the `stop_hook_active` capsule-building branch. The `tcfg.burn_rate &&` guard must keep behavior identical when burn_rate config is absent.
