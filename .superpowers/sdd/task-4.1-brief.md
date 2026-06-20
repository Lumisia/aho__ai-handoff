# Task 4.1 — Rate-limit sample ring buffer

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Create: `core/sensors/samples.mjs`
- Test: `tests/samples.test.mjs`

## Consumes (already exists)
- `projectDir(fingerprint)` from `core/lib/paths.mjs`.
- `writeFileAtomic(path, text)` from `core/lib/fsx.mjs`.

## Produces
- `appendSample(fingerprint, agent, { usedPercent, at = Date.now() }, { max = 6 }) -> void` — keeps the last `max` samples; ignores non-finite `usedPercent`.
- `readSamples(fingerprint, agent) -> [{ usedPercent, at }]` — oldest first.
Stored at `dataRoot/projects/<fp>/samples-<agent>.json`.

## Step 1 — failing test: create `tests/samples.test.mjs`
```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('appendSample keeps the last N samples in order', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-smp-'));
  const { appendSample, readSamples } = await import('../core/sensors/samples.mjs');
  for (let i = 0; i < 8; i++) appendSample('fp', 'codex', { usedPercent: i * 10, at: i * 1000 }, { max: 6 });
  const s = readSamples('fp', 'codex');
  assert.equal(s.length, 6);
  assert.equal(s[0].usedPercent, 20);                 // oldest kept
  assert.equal(s[s.length - 1].usedPercent, 70);      // newest
  delete process.env.AI_HANDOFF_ROOT;
});
```

## Step 2 — run, expect FAIL
`node --test tests/samples.test.mjs` → module not found.

## Step 3 — implement `core/sensors/samples.mjs`
```js
import { join } from 'node:path';
import { readFileSync, mkdirSync } from 'node:fs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { projectDir } from '../lib/paths.mjs';

function samplesPath(fingerprint, agent) {
  return join(projectDir(fingerprint), `samples-${agent}.json`);
}

export function readSamples(fingerprint, agent) {
  try {
    const v = JSON.parse(readFileSync(samplesPath(fingerprint, agent), 'utf8'));
    return Array.isArray(v) ? v : [];
  } catch { return []; }
}

export function appendSample(fingerprint, agent, { usedPercent, at = Date.now() }, { max = 6 } = {}) {
  if (typeof usedPercent !== 'number' || !Number.isFinite(usedPercent)) return;
  try { mkdirSync(projectDir(fingerprint), { recursive: true }); } catch {}
  const next = [...readSamples(fingerprint, agent), { usedPercent, at }].slice(-max);
  writeFileAtomic(samplesPath(fingerprint, agent), JSON.stringify(next, null, 2) + '\n');
}
```

## Step 4 — run, expect PASS
`node --test tests/samples.test.mjs` → pass. Then full `node --test` → no regressions.

## Step 5 — commit
```
git add core/sensors/samples.mjs tests/samples.test.mjs
git commit -m "feat: add rate-limit sample ring buffer"
```

## Global constraints
Node ≥18, zero deps. Only the two files. `node --test` green.
