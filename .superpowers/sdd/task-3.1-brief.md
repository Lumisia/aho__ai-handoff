# Task 3.1 — Claude status-line segment + config key

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Create: `core/lib/statusline-segment.mjs`
- Modify: `core/lib/config-edit.mjs` (add `statusline.show_handoff` to CONFIG_KEYS)
- Modify: `config/defaults.json` (add `statusline` block)
- Modify: `core/cli.mjs` (`sensorClaudeStatusline` prepends the segment)
- Test: create `tests/statusline-segment.test.mjs`

## Consumes (already exists)
- `statusFor(cwd) -> { pending, ... }` from `core/hooks/handoff.mjs`.
- `core/cli.mjs` already imports `loadConfig` from `./lib/config.mjs`, `configPath` from `./lib/paths.mjs`, `runPreviousStatusline` from `./setup/claude-statusline.mjs`, `recordClaudeRateLimit`, `readStdin`, `writeStdout`.

## Produces
- `statuslineSegment({ usedPercent, cwd, show = true }) -> string` → `"AH 82% · ⏳1"` (pending), `"AH 82%"` (idle), or `""` (show=false).

## Step 1 — failing test: create `tests/statusline-segment.test.mjs`
```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { statuslineSegment } from '../core/lib/statusline-segment.mjs';

const cli = join(dirname(fileURLToPath(import.meta.url)), '..', 'core', 'cli.mjs');

test('statuslineSegment shows usage only when no pending capsule', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'ah-sl-'));
  assert.equal(statuslineSegment({ usedPercent: 82, cwd, show: true }), 'AH 82%');
});

test('statuslineSegment adds pending marker when a capsule is pending', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-slr-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-slp-'));
  process.env.AI_HANDOFF_ROOT = root;
  execFileSync(process.execPath, [cli, 'handoff:checkpoint', '--agent', 'codex', '--cwd', cwd],
    { input: JSON.stringify({ session_id: 's', sentinel: { goal: 'g' } }), encoding: 'utf8', env: process.env });
  assert.equal(statuslineSegment({ usedPercent: 82, cwd, show: true }), 'AH 82% · ⏳1');
  delete process.env.AI_HANDOFF_ROOT;
});

test('statuslineSegment returns empty when show is false', () => {
  assert.equal(statuslineSegment({ usedPercent: 82, cwd: undefined, show: false }), '');
});
```

## Step 2 — run, expect FAIL
`node --test tests/statusline-segment.test.mjs` → module not found.

## Step 3 — implement `core/lib/statusline-segment.mjs`
```js
import { statusFor } from '../hooks/handoff.mjs';

export function statuslineSegment({ usedPercent, cwd, show = true } = {}) {
  if (!show) return '';
  const pct = typeof usedPercent === 'number' ? `${Math.round(usedPercent)}%` : null;
  let pending = 0;
  if (cwd) { try { if (statusFor(cwd).pending) pending = 1; } catch {} }
  const head = pct ? `AH ${pct}` : 'AH';
  return pending ? `${head} · ⏳${pending}` : head;
}
```

## Step 4 — config key
- In `core/lib/config-edit.mjs` `CONFIG_KEYS`, add: `'statusline.show_handoff': { type: 'boolean' },`
- In `config/defaults.json`, add a top-level block `"statusline": { "show_handoff": true }` (valid JSON — mind the commas).

## Step 5 — wire into `core/cli.mjs`
Add import: `import { statuslineSegment } from './lib/statusline-segment.mjs';`. Replace the body of `sensorClaudeStatusline` with:
```js
async function sensorClaudeStatusline() {
  const raw = (await readStdin()) || '{}';
  const input = JSON.parse(raw);
  recordClaudeRateLimit(input);
  const cfg = loadConfig({ path: configPath() });
  const seg = statuslineSegment({
    usedPercent: input?.rate_limits?.five_hour?.used_percentage,
    cwd: input.cwd || input.workspace?.current_dir,
    show: cfg.statusline?.show_handoff !== false,
  });
  let prev = '';
  try { prev = runPreviousStatusline(raw); }
  catch (error) { process.stderr.write(`[handoff] previous statusLine failed: ${error.message}\n`); }
  await writeStdout(seg ? (prev ? `${seg} | ${prev}` : seg + '\n') : prev);
}
```

## Step 6 — run tests
`node --test tests/statusline-segment.test.mjs` → PASS. Then full `node --test` → no regressions (config defaults test, if any, must still pass with the new key).

## Step 7 — commit
```
git add -A
git commit -m "feat: add Claude status-line handoff segment (AH <pct> + pending)"
```

## Global constraints
Node ≥18, zero deps. `node --test` green. The segment is Claude-only by design; do not add Codex wiring. Keep `config/defaults.json` valid JSON.
