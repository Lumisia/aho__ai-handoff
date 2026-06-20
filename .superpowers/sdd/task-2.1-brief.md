# Task 2.1 — History store helper

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Create: `core/capsule/history.mjs`
- Test: `tests/history.test.mjs`

## Consumes (already exists)
- `projectDir(fingerprint)` from `core/lib/paths.mjs` → returns `join(dataRoot(), 'projects', fingerprint)`.
- `writeFileAtomic(path, text)` from `core/lib/fsx.mjs`.

## Produces
- `appendHistory(fingerprint, entry, { now = Date.now(), max = 500 }) -> void` — appends one JSON line `{ ts: now, ...entry }` to `dataRoot/projects/<fp>/history.jsonl`, capping the file to the last `max` lines.
- `readHistory(fingerprint, { limit = 20 }) -> entry[]` — returns the last `limit` entries, oldest-first (newest last).

## Step 1 — failing test: create `tests/history.test.mjs`
```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('appendHistory writes entries and readHistory returns newest last, capped', async () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-hist-'));
  process.env.AI_HANDOFF_ROOT = root;
  const { appendHistory, readHistory } = await import('../core/capsule/history.mjs');
  for (let i = 0; i < 5; i++) appendHistory('fp1', { event: 'created', taskId: `t${i}` }, { max: 3 });
  const all = readHistory('fp1', { limit: 10 });
  assert.equal(all.length, 3);                    // capped at max=3
  assert.equal(all[all.length - 1].taskId, 't4'); // newest last
  assert.equal(typeof all[0].ts, 'number');
  delete process.env.AI_HANDOFF_ROOT;
});
```

## Step 2 — run, expect FAIL
`node --test tests/history.test.mjs` → module not found.

## Step 3 — implement `core/capsule/history.mjs`
```js
import { join } from 'node:path';
import { readFileSync, mkdirSync } from 'node:fs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { projectDir } from '../lib/paths.mjs';

function historyPath(fingerprint) { return join(projectDir(fingerprint), 'history.jsonl'); }

function readLines(path) {
  try { return readFileSync(path, 'utf8').split('\n').filter((l) => l.trim()); }
  catch { return []; }
}

export function appendHistory(fingerprint, entry, { now = Date.now(), max = 500 } = {}) {
  const path = historyPath(fingerprint);
  try { mkdirSync(projectDir(fingerprint), { recursive: true }); } catch {}
  const lines = readLines(path);
  lines.push(JSON.stringify({ ts: now, ...entry }));
  writeFileAtomic(path, lines.slice(-max).join('\n') + '\n');
}

export function readHistory(fingerprint, { limit = 20 } = {}) {
  return readLines(historyPath(fingerprint))
    .slice(-limit)
    .map((l) => { try { return JSON.parse(l); } catch { return null; } })
    .filter(Boolean);
}
```

## Step 4 — run, expect PASS
`node --test tests/history.test.mjs` → pass. Then full `node --test` → no regressions.

## Step 5 — commit
```
git add core/capsule/history.mjs tests/history.test.mjs
git commit -m "feat: add per-project handoff history store"
```

## Global constraints
Node ≥18, zero deps. Only the two files above. `node --test` green.
