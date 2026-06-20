# ai-handoff v1.1 Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship six batched enhancements (doctor, status-line segment, history log, opt-in burn-rate trigger, token-report, i18n) as v1.1.0.

**Architecture:** Pure helpers in `core/lib` and `core/sensors` with thin CLI handlers in `core/cli.mjs`; capsule store under `dataRoot()`; TDD against `node --test`. Each feature is independent and testable on its own; i18n is a refactor phase that extracts strings already written by earlier features.

**Tech Stack:** Node ≥18, zero runtime dependencies, ES modules (`.mjs`), `node:test` + `node:assert/strict`.

## Global Constraints

- Node ≥18, **zero runtime dependencies** — stdlib only.
- All CLI JSON input goes through the existing `readInput(args)` (stdin or `--input <file>`, BOM-stripped, `--cwd` override). Do not reintroduce `JSON.parse(await readStdin())`.
- `node --test` must stay green; `npm run validate:package` must pass.
- Capsule/store data lives under `dataRoot()` (`%LOCALAPPDATA%/ai-handoff` | `~/Library/Application Support/ai-handoff` | `$XDG_STATE_HOME/ai-handoff`), never in the repo.
- New `config:set` keys must be added to `CONFIG_KEYS` in `core/lib/config-edit.mjs` with validation and to `config/defaults.json`.
- Skill `description:` frontmatter stays English (platform has no per-locale field).
- Final task bumps version `1.0.7` → `1.1.0` across `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, `package.json`.
- Conventional Commits; commit after every task. Co-author trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

## Phase 1 — `recover` → `doctor`

### Task 1.1: Expose fingerprint basis

**Files:**
- Modify: `core/lib/fingerprint.mjs`
- Test: `tests/fingerprint.test.mjs`

**Interfaces:**
- Produces: `projectFingerprintInfo(cwd) -> { fingerprint: string, basis: { type: 'remote'|'gitroot'|'path', value: string } }`. `projectFingerprint(cwd)` keeps returning the 24-char string (now via `projectFingerprintInfo`).

- [ ] **Step 1: Write the failing test**

```js
// append to tests/fingerprint.test.mjs
import { projectFingerprintInfo } from '../core/lib/fingerprint.mjs';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('projectFingerprintInfo reports a path basis for a non-repo dir', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ah-fp-'));
  const info = projectFingerprintInfo(dir);
  assert.equal(info.basis.type, 'path');
  assert.match(info.basis.value, /^path:/);
  assert.equal(info.fingerprint.length, 24);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/fingerprint.test.mjs`
Expected: FAIL — `projectFingerprintInfo` is not exported.

- [ ] **Step 3: Refactor `fingerprint.mjs` to expose basis**

```js
// core/lib/fingerprint.mjs — replace projectFingerprint with:
export function projectFingerprintInfo(cwd) {
  let basis = null;
  const url = git(cwd, ['config', '--get', 'remote.origin.url']);
  if (url) basis = { type: 'remote', value: 'remote:' + url };
  if (!basis) {
    const root = git(cwd, ['rev-parse', '--show-toplevel']);
    if (root) {
      let resolved = root;
      try { resolved = realpathSync(root); } catch {}
      basis = { type: 'gitroot', value: 'gitroot:' + resolved };
    }
  }
  if (!basis) {
    let resolved = cwd;
    try { resolved = realpathSync(cwd); } catch {}
    basis = { type: 'path', value: 'path:' + resolved };
  }
  return { fingerprint: sha256Hex(basis.value).slice(0, 24), basis };
}

export function projectFingerprint(cwd) {
  return projectFingerprintInfo(cwd).fingerprint;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test tests/fingerprint.test.mjs`
Expected: PASS (new test + any existing fingerprint tests).

- [ ] **Step 5: Commit**

```bash
git add core/lib/fingerprint.mjs tests/fingerprint.test.mjs
git commit -m "refactor: expose fingerprint basis via projectFingerprintInfo"
```

### Task 1.2: Rename recover → doctor and expand diagnosis

**Files:**
- Modify: `core/hooks/handoff.mjs` (rename `recoverFor` → `doctorFor`, expand)
- Modify: `core/cli.mjs` (dispatch `handoff:recover` → `handoff:doctor`, handler)
- Rename: `skills/handoff-recover/SKILL.md` → `skills/handoff-doctor/SKILL.md`
- Modify: `tests/skills-present.test.mjs`, `tests/cli-handoff.test.mjs` (if it references recover)
- Test: `tests/cli-doctor.test.mjs` (new)

**Interfaces:**
- Consumes: `projectFingerprintInfo` (Task 1.1), `findPendingCapsule`, `verifyStoredCapsule`, `findApproval`, `dataRoot`.
- Produces: `doctorFor(cwd, { now }) -> { fingerprint, basis, cwdResolved, dataRoot, healthy, issues, pending, approval, otherPending }`. CLI command `handoff:doctor`.

- [ ] **Step 1: Write the failing test**

```js
// tests/cli-doctor.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const cli = join(dirname(fileURLToPath(import.meta.url)), '..', 'core', 'cli.mjs');
const run = (args, input, env) =>
  execFileSync(process.execPath, [cli, ...args], { input, encoding: 'utf8', env: { ...process.env, ...env } });

test('handoff:doctor reports basis and cross-fingerprint pending capsules', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-doc-'));
  const projA = mkdtempSync(join(tmpdir(), 'ah-a-'));
  const projB = mkdtempSync(join(tmpdir(), 'ah-b-'));
  const env = { AI_HANDOFF_ROOT: root };
  // capsule authored under projA's fingerprint
  run(['handoff:checkpoint', '--agent', 'codex', '--cwd', projA],
    JSON.stringify({ session_id: 's', sentinel: { goal: 'find me', next_actions: ['x'] } }), env);
  // doctor run from projB (different fingerprint) must still surface it
  const out = JSON.parse(run(['handoff:doctor', '--cwd', projB], '', env));
  assert.equal(out.basis.type, 'path');
  assert.equal(out.pending, null);
  assert.equal(out.otherPending.length, 1);
  assert.equal(out.otherPending[0].goal, 'find me');
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/cli-doctor.test.mjs`
Expected: FAIL — `unknown command: handoff:doctor`.

- [ ] **Step 3: Expand and rename in `core/hooks/handoff.mjs`**

Add imports at top: `import { projectFingerprintInfo } from '../lib/fingerprint.mjs';` and `import { readdirSync, readFileSync, existsSync } from 'node:fs';` and `import { join } from 'node:path';` and `import { dataRoot } from '../lib/paths.mjs';`. Replace `recoverFor` with:

```js
function scanOtherPending(currentFp) {
  const projects = join(dataRoot(), 'projects');
  const out = [];
  let names = [];
  try { names = readdirSync(projects); } catch { return out; }
  for (const fp of names) {
    if (fp === currentFp) continue;
    const handoffDir = join(projects, fp, 'handoff');
    let tasks = [];
    try { tasks = readdirSync(handoffDir); } catch { continue; }
    for (const taskId of tasks) {
      const statePath = join(handoffDir, taskId, 'state.json');
      const capPath = join(handoffDir, taskId, 'capsule.json');
      if (!existsSync(statePath) || !existsSync(capPath)) continue;
      let state; let cap;
      try { state = JSON.parse(readFileSync(statePath, 'utf8')); cap = JSON.parse(readFileSync(capPath, 'utf8')); }
      catch { continue; }
      if (state.status !== 'AVAILABLE' && state.status !== 'DEGRADED_AVAILABLE') continue;
      out.push({
        fingerprint: fp, taskId,
        goal: cap.task && cap.task.goal,
        source: cap.source && cap.source.agent,
        branch: cap.project && cap.project.git_branch,
      });
    }
  }
  return out;
}

export function doctorFor(cwd, { now = Date.now() } = {}) {
  const { fingerprint, basis } = projectFingerprintInfo(cwd);
  let cwdResolved = cwd;
  try { cwdResolved = realpathSync(cwd); } catch {}
  const pending = findPendingCapsule(fingerprint, { now });
  const approval = findApproval(fingerprint);
  const issues = [];
  let verified = null;
  if (pending?.capsule) {
    verified = verifyStoredCapsule(fingerprint, pending.taskId, { now });
    issues.push(...verified.errors);
  }
  return {
    fingerprint,
    basis,
    cwdResolved,
    dataRoot: dataRoot(),
    healthy: issues.length === 0,
    issues,
    pending: pending ? {
      taskId: pending.taskId,
      status: pending.state.status,
      recoveredAt: pending.state.recovered_at || null,
      verified: verified?.valid ?? false,
    } : null,
    approval: approval ? { key: approval.key, status: approval.status } : null,
    otherPending: scanOtherPending(fingerprint),
  };
}
```

Add `import { realpathSync } from 'node:fs';` (merge with the fs import). Remove the old `recoverFor` export.

- [ ] **Step 4: Rename command in `core/cli.mjs`**

Replace the import `recoverFor` → `doctorFor`, rename `handoffRecover` → `handoffDoctor` using `doctorFor`, and in the dispatch map replace `'handoff:recover': handoffRecover` with `'handoff:doctor': handoffDoctor`:

```js
async function handoffDoctor(args) {
  const input = await readInput(args);
  await writeStdout(JSON.stringify(doctorFor(input.cwd || process.cwd()), null, 2) + '\n');
}
```

- [ ] **Step 5: Rename the skill**

```bash
git mv skills/handoff-recover skills/handoff-doctor
```

Rewrite `skills/handoff-doctor/SKILL.md`:

```markdown
---
name: handoff-doctor
description: Diagnose why a handoff is not appearing — fingerprint/basis, store location, capsule integrity, stale claims, approval state, and capsules pending under a different directory/fingerprint.
---

# handoff-doctor

Run `handoff:doctor --cwd "<project dir>"`. Report `basis` (how the project
fingerprint was derived: git remote / git root / path), `dataRoot` (where
capsules live), `cwdResolved`, current `pending`/`issues`, and `approval`.

If `otherPending` is non-empty, a capsule exists under a DIFFERENT fingerprint —
tell the user which directory/remote it belongs to and that both agents must run
from the same project (a git repo gives a path-independent remote-based
fingerprint). Do not consume, rewrite, or delete a capsule during diagnosis.
```

- [ ] **Step 6: Update references**

In `tests/skills-present.test.mjs`, change `'skills/handoff-recover/SKILL.md'` → `'skills/handoff-doctor/SKILL.md'`. Grep for `handoff:recover` / `recoverFor` across `tests/` and `README*.md` and update to doctor.

Run: `git grep -n "handoff:recover\|recoverFor\|handoff-recover"` — expected: no matches after edits.

- [ ] **Step 7: Run tests**

Run: `node --test tests/cli-doctor.test.mjs tests/skills-present.test.mjs`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: rename recover to doctor and add basis + cross-fingerprint scan"
```

---

## Phase 2 — `handoff:history` audit log

### Task 2.1: History store helper

**Files:**
- Create: `core/capsule/history.mjs`
- Test: `tests/history.test.mjs`

**Interfaces:**
- Consumes: `projectDir(fingerprint)` from `core/lib/paths.mjs`.
- Produces: `appendHistory(fingerprint, entry, { now, max }) -> void` (writes JSONL line `{ ts, ...entry }`, caps file to last `max` lines, default 500); `readHistory(fingerprint, { limit }) -> entry[]` (newest last, default 20).

- [ ] **Step 1: Write the failing test**

```js
// tests/history.test.mjs
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
  assert.equal(all.length, 3);                 // capped at max=3
  assert.equal(all[all.length - 1].taskId, 't4'); // newest last
  assert.equal(typeof all[0].ts, 'number');
  delete process.env.AI_HANDOFF_ROOT;
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/history.test.mjs`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `core/capsule/history.mjs`**

```js
import { join } from 'node:path';
import { existsSync, readFileSync, mkdirSync } from 'node:fs';
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
  const capped = lines.slice(-max);
  writeFileAtomic(path, capped.join('\n') + '\n');
}

export function readHistory(fingerprint, { limit = 20 } = {}) {
  const lines = readLines(historyPath(fingerprint));
  return lines.slice(-limit).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
}
```

- [ ] **Step 4: Run test**

Run: `node --test tests/history.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/capsule/history.mjs tests/history.test.mjs
git commit -m "feat: add per-project handoff history store"
```

### Task 2.2: Record lifecycle events and add `handoff:history` command

**Files:**
- Modify: `core/capsule/store.mjs` (`publishCapsule`, `consumeCapsule`)
- Modify: `core/hooks/handoff.mjs` (`skipApproval`, `createFromApproval`)
- Modify: `core/cli.mjs` (new `handoff:history` command)
- Test: `tests/cli-handoff.test.mjs` (extend)

**Interfaces:**
- Consumes: `appendHistory` (Task 2.1).
- Produces: CLI command `handoff:history` → prints `readHistory(fp, { limit })` as JSON; `--limit N` override.

- [ ] **Step 1: Write the failing test**

```js
// append to tests/cli-handoff.test.mjs
test('handoff:history records created then resumed', () => {
  const root = mkdtempSync(join(tmpdir(), 'ah-cliH-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-proj-'));
  const env = { AI_HANDOFF_ROOT: root };
  run(['handoff:checkpoint', '--agent', 'codex'], JSON.stringify({ cwd, session_id: 's', sentinel: { goal: 'g' } }), env);
  run(['handoff:resume', '--agent', 'claude-code'], JSON.stringify({ cwd }), env);
  const hist = JSON.parse(run(['handoff:history', '--cwd', cwd], '', env));
  assert.deepEqual(hist.map((h) => h.event), ['created', 'resumed']);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/cli-handoff.test.mjs`
Expected: FAIL — `unknown command: handoff:history`.

- [ ] **Step 3: Record events in the store**

In `core/capsule/store.mjs` add `import { appendHistory } from './history.mjs';`. In `publishCapsule`, immediately after the capsule file is first written (the `writeFileAtomic(capsulePath, text)` branch), add:

```js
appendHistory(fingerprint, {
  event: 'created', taskId: capsule.task_id,
  agent: capsule.source?.agent, source: capsule.source?.agent, target: capsule.target?.agent,
  trigger: capsule.trigger?.type, observed_percent: capsule.trigger?.observed_percent ?? null,
}, { now });
```

In `consumeCapsule`, after the state transition to CONSUMED, add (read task_id/fingerprint from state — `consumeCapsule` has `claim.statePath`; derive fingerprint from the path's parent-of-parent dir name):

```js
const fp = basename(dirname(dirname(claim.statePath)));
appendHistory(fp, { event: 'resumed', taskId: st.task_id }, { now });
```

Add `import { basename, dirname } from 'node:path';` to `store.mjs`.

- [ ] **Step 4: Record skip/create in `handoff.mjs`**

In `core/hooks/handoff.mjs` add `import { appendHistory } from '../capsule/history.mjs';`. In `skipApproval`, before `return { skipped: true, ... }`, add `appendHistory(fp, { event: 'skipped', key: approval.key }, { now });`. In `createFromApproval`, after `publishCapsule(...)`, add `appendHistory(fp, { event: 'created_from_approval', taskId: capsule.task_id, agent: context.agent }, { now });`.

- [ ] **Step 5: Add the CLI command**

In `core/cli.mjs` add `import { readHistory } from './capsule/history.mjs';`, a handler, and dispatch entry `'handoff:history': handoffHistory`:

```js
async function handoffHistory(args) {
  const input = await readInput(args);
  const limit = Number(argValue(args, '--limit', '20')) || 20;
  const fp = projectFingerprint(input.cwd || process.cwd());
  await writeStdout(JSON.stringify(readHistory(fp, { limit }), null, 2) + '\n');
}
```

- [ ] **Step 6: Run tests**

Run: `node --test tests/cli-handoff.test.mjs`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: record handoff lifecycle events and add handoff:history"
```

---

## Phase 3 — Claude status-line segment

### Task 3.1: `statuslineSegment` pure function + config key

**Files:**
- Create: `core/lib/statusline-segment.mjs`
- Modify: `core/lib/config-edit.mjs` (add `statusline.show_handoff`), `config/defaults.json`
- Modify: `core/cli.mjs` (`sensorClaudeStatusline` prepends the segment)
- Test: `tests/statusline-segment.test.mjs`

**Interfaces:**
- Consumes: `statusFor` from `core/hooks/handoff.mjs`.
- Produces: `statuslineSegment({ usedPercent, cwd, show }) -> string` (`"AH 82% · ⏳1"`, `"AH 82%"`, or `""`).

- [ ] **Step 1: Write the failing test**

```js
// tests/statusline-segment.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
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

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/statusline-segment.test.mjs`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `core/lib/statusline-segment.mjs`**

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

- [ ] **Step 4: Add config key**

In `core/lib/config-edit.mjs` `CONFIG_KEYS`, add `'statusline.show_handoff': { type: 'boolean' },`. In `config/defaults.json` add a `"statusline": { "show_handoff": true }` block.

- [ ] **Step 5: Wire into the status-line command**

In `core/cli.mjs` `sensorClaudeStatusline`, after `recordClaudeRateLimit(input)`, prepend the segment to the chained output:

```js
import { statuslineSegment } from './lib/statusline-segment.mjs';
import { loadConfig } from './lib/config.mjs'; // already imported
// inside sensorClaudeStatusline, replace the writeStdout block:
const cfg = loadConfig({ path: configPath() });
const seg = statuslineSegment({
  usedPercent: input?.rate_limits?.five_hour?.used_percentage,
  cwd: input.cwd || input.workspace?.current_dir,
  show: cfg.statusline?.show_handoff !== false,
});
let prev = '';
try { prev = runPreviousStatusline(raw); } catch (error) { process.stderr.write(`[handoff] previous statusLine failed: ${error.message}\n`); }
await writeStdout(seg ? (prev ? `${seg} | ${prev}` : seg + '\n') : prev);
```

- [ ] **Step 6: Run tests**

Run: `node --test tests/statusline-segment.test.mjs`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add Claude status-line handoff segment (AH <pct> + pending)"
```

---

## Phase 4 — Opt-in burn-rate trigger

### Task 4.1: Sample ring buffer

**Files:**
- Create: `core/sensors/samples.mjs`
- Test: `tests/samples.test.mjs`

**Interfaces:**
- Consumes: `projectDir(fingerprint)`.
- Produces: `appendSample(fingerprint, agent, { usedPercent, at }, { max }) -> void` (keeps last `max`, default 6); `readSamples(fingerprint, agent) -> [{ usedPercent, at }]` (oldest first).

- [ ] **Step 1: Write the failing test**

```js
// tests/samples.test.mjs
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
  assert.equal(s[0].usedPercent, 20);     // oldest kept
  assert.equal(s[s.length - 1].usedPercent, 70);
  delete process.env.AI_HANDOFF_ROOT;
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/samples.test.mjs`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `core/sensors/samples.mjs`**

```js
import { join } from 'node:path';
import { existsSync, readFileSync, mkdirSync } from 'node:fs';
import { writeFileAtomic } from '../lib/fsx.mjs';
import { projectDir } from '../lib/paths.mjs';

function samplesPath(fingerprint, agent) {
  return join(projectDir(fingerprint), `samples-${agent}.json`);
}

export function readSamples(fingerprint, agent) {
  try { const v = JSON.parse(readFileSync(samplesPath(fingerprint, agent), 'utf8')); return Array.isArray(v) ? v : []; }
  catch { return []; }
}

export function appendSample(fingerprint, agent, { usedPercent, at = Date.now() }, { max = 6 } = {}) {
  if (typeof usedPercent !== 'number' || !Number.isFinite(usedPercent)) return;
  try { mkdirSync(projectDir(fingerprint), { recursive: true }); } catch {}
  const next = [...readSamples(fingerprint, agent), { usedPercent, at }].slice(-max);
  writeFileAtomic(samplesPath(fingerprint, agent), JSON.stringify(next, null, 2) + '\n');
}
```

- [ ] **Step 4: Run test**

Run: `node --test tests/samples.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/sensors/samples.mjs tests/samples.test.mjs
git commit -m "feat: add rate-limit sample ring buffer"
```

### Task 4.2: Burn-rate logic in `evaluateTrigger` + config keys

**Files:**
- Modify: `core/hooks/trigger.mjs`
- Modify: `core/lib/config-edit.mjs`, `config/defaults.json`
- Test: `tests/trigger.test.mjs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `evaluateTrigger({ usedPercent, threshold, mode, deduped, samples, burnRate, now })`. `burnRate = { enabled, runwayMinutes }`. Adds reasons `burn-rate` and `insufficient-samples`.

- [ ] **Step 1: Write the failing test**

```js
// tests/trigger.test.mjs
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
  const samples = [{ usedPercent: 60, at: now - 60 * 60000 }, { usedPercent: 62, at: now }]; // +2%/60min, slow
  assert.equal(evaluateTrigger({ ...base, usedPercent: 62, samples, burnRate: { enabled: true, runwayMinutes: 30 }, now }).action, 'none');
});

test('burn-rate disabled => static only', () => {
  const now = 100 * 60000;
  const samples = [{ usedPercent: 60, at: now - 10 * 60000 }, { usedPercent: 80, at: now }];
  assert.equal(evaluateTrigger({ ...base, usedPercent: 80, samples, burnRate: { enabled: false }, now }).action, 'none');
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/trigger.test.mjs`
Expected: FAIL on burn-rate cases.

- [ ] **Step 3: Implement burn-rate in `core/hooks/trigger.mjs`**

```js
function projectMinutesTo100(samples, usedPercent, now) {
  if (!Array.isArray(samples) || samples.length < 2) return null;
  const sorted = [...samples].sort((a, b) => a.at - b.at);
  const first = sorted[0];
  const last = sorted[sorted.length - 1];
  const dPct = last.usedPercent - first.usedPercent;
  const dMin = (last.at - first.at) / 60000;
  if (dMin <= 0 || dPct <= 0) return null;
  const slope = dPct / dMin;                 // % per minute
  const remaining = 100 - (typeof usedPercent === 'number' ? usedPercent : last.usedPercent);
  if (remaining <= 0) return 0;
  return remaining / slope;
}

export function evaluateTrigger({ usedPercent, threshold, mode, deduped, samples, burnRate, now = Date.now() }) {
  if (mode === 'off') return { action: 'none', reason: 'off' };
  if (typeof usedPercent !== 'number') return { action: 'none', reason: 'unknown' };
  const fire = (reason) => deduped ? { action: 'none', reason: 'deduped' } : { action: mode === 'auto' ? 'create' : 'ask', reason };
  if (usedPercent >= threshold) return fire('threshold');
  if (burnRate && burnRate.enabled) {
    const eta = projectMinutesTo100(samples, usedPercent, now);
    if (eta == null) return { action: 'none', reason: 'insufficient-samples' };
    if (eta <= (burnRate.runwayMinutes ?? 30)) return fire('burn-rate');
  }
  return { action: 'none', reason: 'below' };
}
```

- [ ] **Step 4: Add config keys**

In `CONFIG_KEYS`: add `'triggers.five_hour.burn_rate.enabled': { type: 'boolean' },` and `'triggers.five_hour.burn_rate.runway_minutes': { type: 'number', min: 5, max: 120 },`. In `config/defaults.json`, set `triggers.five_hour.burn_rate` to `{ "enabled": false, "runway_minutes": 30 }`.

- [ ] **Step 5: Run tests**

Run: `node --test tests/trigger.test.mjs`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add opt-in burn-rate projection to evaluateTrigger"
```

### Task 4.3: Wire samples + burn-rate into `handleStop`

**Files:**
- Modify: `core/hooks/stop.mjs`
- Modify: `core/sensors/claude-statusline.mjs` (append sample when cwd present)
- Test: `tests/cli-hooks.test.mjs` (extend) or `tests/stop.test.mjs` if present

**Interfaces:**
- Consumes: `appendSample`/`readSamples` (Task 4.1), `evaluateTrigger` burn-rate params (Task 4.2).

- [ ] **Step 1: Write the failing test**

```js
// tests/burn-rate-stop.test.mjs
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

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/burn-rate-stop.test.mjs`
Expected: FAIL — burn-rate not wired into `handleStop`.

- [ ] **Step 3: Wire into `core/hooks/stop.mjs`**

Add `import { appendSample, readSamples } from '../sensors/samples.mjs';`. After `const reading = await readSensor();` (line ~76) and before the `evaluateTrigger` call, append the reading and load history; pass burn-rate config into `evaluateTrigger`:

```js
if (reading && typeof reading.usedPercent === 'number') {
  appendSample(fp, agent, { usedPercent: reading.usedPercent, at: now });
}
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

- [ ] **Step 4: Append samples from the Claude status line**

In `core/sensors/claude-statusline.mjs` `recordClaudeRateLimit`, when `input.cwd` (or `input.workspace?.current_dir`) is present, also append a sample. Add `import { appendSample } from './samples.mjs';` and `import { projectFingerprint } from '../lib/fingerprint.mjs';`, then before `return true;`:

```js
const cwd = input.cwd || input.workspace?.current_dir;
if (cwd) { try { appendSample(projectFingerprint(cwd), 'claude-code', { usedPercent: used, at: now }); } catch {} }
```

- [ ] **Step 5: Run tests**

Run: `node --test tests/burn-rate-stop.test.mjs tests/cli-hooks.test.mjs`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: feed samples and burn-rate config into handleStop"
```

---

## Phase 5 — i18n for human-facing output

### Task 5.1: Message catalog + `t()` + completeness test

**Files:**
- Create: `core/lib/i18n.mjs`
- Modify: `core/lib/config-edit.mjs`, `config/defaults.json` (add `locale`)
- Test: `tests/i18n.test.mjs`

**Interfaces:**
- Produces: `t(key, vars = {}, locale = 'en') -> string` (interpolates `{name}` from `vars`, falls back to `en` for missing key/locale); `MESSAGES` (catalog object keyed by locale).

- [ ] **Step 1: Write the failing test**

```js
// tests/i18n.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { t, MESSAGES } from '../core/lib/i18n.mjs';

test('t interpolates and falls back to en', () => {
  assert.equal(t('ask.create_or_skip', {}, 'ko'), MESSAGES.ko['ask.create_or_skip']);
  assert.equal(t('ask.create_or_skip', {}, 'xx'), MESSAGES.en['ask.create_or_skip']); // unknown locale
});

test('every locale defines the same keys as en (completeness)', () => {
  const enKeys = Object.keys(MESSAGES.en).sort();
  for (const loc of ['ko', 'ja', 'zh']) {
    assert.deepEqual(Object.keys(MESSAGES[loc]).sort(), enKeys, `${loc} key set must match en`);
  }
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/i18n.test.mjs`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `core/lib/i18n.mjs`**

Define the catalog with the keys used by the codebase (`ask.create_or_skip`, `notify.capsule_ready`, `summary.instruction`). Provide en/ko/ja/zh for each, plus `t()`:

```js
export const MESSAGES = {
  en: {
    'ask.create_or_skip': 'Create a capsule? /handoff create | /handoff skip',
    'notify.capsule_ready': 'Capsule ready for {agent}',
    'summary.instruction': 'Create the handoff capsule now. Reply with exactly one semantic summary wrapped in <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule>. Do not include secrets, hidden reasoning, or transcript text.',
  },
  ko: {
    'ask.create_or_skip': '캡슐을 생성할까요? /handoff create | /handoff skip',
    'notify.capsule_ready': '{agent}에게 전달할 캡슐이 준비됨',
    'summary.instruction': '지금 핸드오프 캡슐을 만드세요. <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 형식의 의미 요약 하나만 답하세요. 비밀·숨은 추론·대화 원문은 포함하지 마세요.',
  },
  ja: {
    'ask.create_or_skip': 'カプセルを作成しますか？ /handoff create | /handoff skip',
    'notify.capsule_ready': '{agent} 向けのカプセルが準備できました',
    'summary.instruction': '今すぐハンドオフ・カプセルを作成してください。<handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 形式の意味要約を1つだけ返してください。秘密・隠れた推論・会話本文は含めないでください。',
  },
  zh: {
    'ask.create_or_skip': '创建胶囊吗？ /handoff create | /handoff skip',
    'notify.capsule_ready': '已为 {agent} 准备好胶囊',
    'summary.instruction': '现在创建交接胶囊。仅回复一个用 <handoff-capsule>{"goal":"...","next_actions":["..."],"completed":[],"open_issues":[],"status":"in_progress"}</handoff-capsule> 包裹的语义摘要。不要包含密钥、隐藏推理或对话原文。',
  },
};

export function t(key, vars = {}, locale = 'en') {
  const table = MESSAGES[locale] || MESSAGES.en;
  const template = table[key] ?? MESSAGES.en[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (_, k) => (k in vars ? String(vars[k]) : `{${k}}`));
}
```

- [ ] **Step 4: Add the `locale` config key**

In `CONFIG_KEYS`: `'locale': { type: 'enum', values: ['en', 'ko', 'ja', 'zh'] },`. In `config/defaults.json`, add top-level `"locale": "en"`.

- [ ] **Step 5: Run tests**

Run: `node --test tests/i18n.test.mjs`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add i18n message catalog and t() with en fallback"
```

### Task 5.2: Localize stop-hook strings

**Files:**
- Modify: `core/hooks/stop.mjs` (use `t()` with `pcfg.locale`)
- Test: `tests/stop-i18n.test.mjs`

**Interfaces:**
- Consumes: `t` (Task 5.1).

- [ ] **Step 1: Write the failing test**

```js
// tests/stop-i18n.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { handleStop } from '../core/hooks/stop.mjs';

test('ask notification body is localized to ko', async () => {
  process.env.AI_HANDOFF_ROOT = mkdtempSync(join(tmpdir(), 'ah-i18n-'));
  const cwd = mkdtempSync(join(tmpdir(), 'ah-i18np-'));
  const config = {
    locale: 'ko',
    triggers: { five_hour: { enabled: true, threshold_percent: 50, mode: 'ask' } },
    notification: { method: 'terminal' },
  };
  let captured = '';
  const readSensor = async () => ({ usedPercent: 90, windowMinutes: 300, resetsAt: null });
  await handleStop({ input: { cwd, session_id: 's' }, config, readSensor, agent: 'codex', now: 1, notifyFn: (t2, b) => { captured = b; } });
  assert.match(captured, /캡슐을 생성할까요/);
  delete process.env.AI_HANDOFF_ROOT;
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test tests/stop-i18n.test.mjs`
Expected: FAIL — body is still the hardcoded English+Korean mix.

- [ ] **Step 3: Localize `core/hooks/stop.mjs`**

Add `import { t } from '../lib/i18n.mjs';`. Read `const locale = pcfg.locale || 'en';` (after `pcfg` is resolved). Replace:
- `summaryInstruction()` usage with `t('summary.instruction', {}, locale)`.
- `sendNotice('AI handoff', 'Capsule을 생성할까요? /handoff create | /handoff skip')` → `sendNotice('AI handoff', t('ask.create_or_skip', {}, locale))`.
- `sendNotice('AI handoff', \`Capsule ready for ${capsule.target.agent}\`)` → `sendNotice('AI handoff', t('notify.capsule_ready', { agent: capsule.target.agent }, locale))`.

Note: the `stop_hook_active` branch resolves `pcfg`/`locale` too (it runs before that block today — move the `pcfg`/`locale` resolution above the `if (input.stop_hook_active)` block so both branches see it). Also update `core/cli.mjs` `hookStop` ask-branch string (`'Ask the user once: ...'`) to use `t('ask.create_or_skip', {}, config-derived locale)` — load locale via `loadConfig` there.

- [ ] **Step 4: Run tests**

Run: `node --test tests/stop-i18n.test.mjs tests/cli-hooks.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: localize stop-hook notifications and prompts via t()"
```

---

## Phase 6 — Token-optimization report (investigation only)

### Task 6.1: Measure and write the report

**Files:**
- Create: `docs/superpowers/notes/2026-06-20-token-budget-report.md`

- [ ] **Step 1: Measure injection sizes**

Run a temporary script to capture sizes (do not commit the script):

```bash
node -e '
import("./core/hooks/session-start.mjs").then(async (m) => {
  // build a representative capsule via the CLI store path, then measure prepareSessionStart output length
  console.log("measure prepareSessionStart output, renderMemoryRecall output, and SKILL.md sizes");
});
' 2>/dev/null
wc -c skills/handoff-session/SKILL.md
```

Record: character counts of (a) a representative resume injection (`prepareSessionStart` context string), (b) a representative `renderMemoryRecall` output at the default 800 budget, (c) `skills/handoff-session/SKILL.md`. Convert to approx tokens (chars ÷ 4).

- [ ] **Step 2: Write the report**

Create `docs/superpowers/notes/2026-06-20-token-budget-report.md` with: the measured numbers; the levers (progressive disclosure of the resume injection; capping/summarizing long capsule fields; terser injection format; tuning `memory.auto_recall_token_budget`); each with estimated savings and risk; and a recommendation of which to implement in a v1.2 batch.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/notes/2026-06-20-token-budget-report.md
git commit -m "docs: token budget measurement and optimization levers (v1.2 candidate)"
```

---

## Phase 7 — Release v1.1.0

### Task 7.1: Version bump, READMEs, full verification

**Files:**
- Modify: `package.json`, `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`
- Modify: `README.md`, `README.ko.md`, `README.ja.md`, `README.zh.md`

- [ ] **Step 1: Bump version**

Set `"version"` to `1.1.0` in all three manifests.

- [ ] **Step 2: Update READMEs (4 languages)**

In each README: rename `/handoff recover` → `/handoff doctor` (+ note the expanded diagnosis), add `/handoff history`, document the new config keys (`locale`, `triggers.five_hour.burn_rate.enabled`, `triggers.five_hour.burn_rate.runway_minutes`, `statusline.show_handoff`), and add a one-line i18n/`locale` note. State the status-line segment is Claude-only.

- [ ] **Step 3: Full verification**

Run: `node --test`
Expected: all tests pass (0 fail).
Run: `npm run validate:package`
Expected: `package valid: ai-handoff@1.1.0`.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: release v1.1.0 (doctor, statusline, history, burn-rate, i18n)"
```

- [ ] **Step 5: Push (only on explicit user approval)**

```bash
git push origin master
```

---

## Self-Review

- **Spec coverage:** doctor (Phase 1) ✓; statusline (Phase 3) ✓; history (Phase 2) ✓; burn-rate (Phase 4) ✓; token report (Phase 6) ✓; i18n (Phase 5) ✓; config keys (folded into 3.1/4.2/5.1) ✓; version bump + READMEs (Phase 7) ✓.
- **Placeholder scan:** every code/step contains real code and exact commands; no TBD/TODO.
- **Type consistency:** `projectFingerprintInfo` (1.1) consumed by `doctorFor` (1.2); `appendHistory`/`readHistory` (2.1) consumed by 2.2; `appendSample`/`readSamples` (4.1) consumed by 4.2/4.3; `evaluateTrigger` burn-rate params (4.2) consumed by 4.3; `t()` (5.1) consumed by 5.2; `statuslineSegment` (3.1) consumed by the status-line command.
- **Note for implementer:** Tasks within a phase are ordered; Phases 1–4 are independent and may be reordered, but Phase 5 (i18n) must follow the features whose strings it localizes, and Phase 7 is last.
