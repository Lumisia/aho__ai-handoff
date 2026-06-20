# Task 2.2 — Record lifecycle events + `handoff:history` command

ai-handoff repo, branch v1.1-enhancements. Node ≥18, zero deps, ES modules. TDD.

## Files
- Modify: `core/capsule/store.mjs` (`publishCapsule`, `consumeCapsule`)
- Modify: `core/hooks/handoff.mjs` (`skipApproval`, `createFromApproval`)
- Modify: `core/cli.mjs` (new `handoff:history` command + dispatch entry)
- Test: extend `tests/cli-handoff.test.mjs`

## Consumes (already exists, committed)
- `appendHistory(fingerprint, entry, { now, max })` and `readHistory(fingerprint, { limit })` from `core/capsule/history.mjs`.
- `core/cli.mjs` has `readInput(args)`, `argValue(args, name, fallback)`, `writeStdout`, and imports `projectFingerprint` from `./lib/fingerprint.mjs`.
- `core/capsule/store.mjs` already imports from `node:path` — check the existing import line and extend it.

## Produces
- History entries appended at: capsule publish (`created`), consume/resume (`resumed`), approval skip (`skipped`), create-from-approval (`created_from_approval`).
- CLI command `handoff:history` → prints `readHistory(fp, { limit })` JSON; `--limit N` (default 20).

## Step 1 — failing test: append to `tests/cli-handoff.test.mjs`
(The file already has a `run(args, input, env)` helper and imports `mkdtempSync`, `tmpdir`, `join`. Reuse them — do not redefine.)
```js
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

## Step 2 — run, expect FAIL
`node --test tests/cli-handoff.test.mjs` → `unknown command: handoff:history`.

## Step 3 — record events in `core/capsule/store.mjs`
Add import: `import { appendHistory } from './history.mjs';`. Ensure `basename, dirname` are imported from `node:path` (extend the existing `node:path` import line).

In `publishCapsule`, in the branch that FIRST writes the capsule (right after the `writeState(statePath, { status, task_id: capsule.task_id, updated_at: now });` that follows `writeFileAtomic(capsulePath, text)` / `refreshProjectIndex(...)`), add:
```js
appendHistory(fingerprint, {
  event: 'created', taskId: capsule.task_id,
  agent: capsule.source?.agent, source: capsule.source?.agent, target: capsule.target?.agent,
  trigger: capsule.trigger?.type, observed_percent: capsule.trigger?.observed_percent ?? null,
}, { now });
```
(Only in the fresh-write path — NOT in the idempotent "already published" early-return path.)

In `consumeCapsule(claim, { now = Date.now() } = {})`, after `writeState(claim.statePath, next);` and before `releaseLock(claim.lock);`, add (derive fingerprint from the state path — statePath is `<dataRoot>/projects/<fp>/handoff/<taskId>/state.json`, so the fingerprint is three `dirname` levels up):
```js
const fp = basename(dirname(dirname(dirname(claim.statePath))));
appendHistory(fp, { event: 'resumed', taskId: next.task_id }, { now });
```
(`next` is the updated state object; it carries `task_id`. If for some reason `next.task_id` is undefined, fall back to `st.task_id`.)

## Step 4 — record skip / create in `core/hooks/handoff.mjs`
Add `import { appendHistory } from '../capsule/history.mjs';`.
- In `skipApproval`, before `return { skipped: true, fingerprint: fp };`, add: `appendHistory(fp, { event: 'skipped', key: approval.key }, { now });`
- In `createFromApproval`, after the `publishCapsule(fp, capsule, ...)` call, add: `appendHistory(fp, { event: 'created_from_approval', taskId: capsule.task_id, agent: context.agent }, { now });`

## Step 5 — add the CLI command in `core/cli.mjs`
Add `import { readHistory } from './capsule/history.mjs';`. Add handler + dispatch entry `'handoff:history': handoffHistory`:
```js
async function handoffHistory(args) {
  const input = await readInput(args);
  const limit = Number(argValue(args, '--limit', '20')) || 20;
  const fp = projectFingerprint(input.cwd || process.cwd());
  await writeStdout(JSON.stringify(readHistory(fp, { limit }), null, 2) + '\n');
}
```

## Step 6 — run tests
`node --test tests/cli-handoff.test.mjs` → PASS (new test green). Then full `node --test` → no regressions.

## Step 7 — commit
```
git add -A
git commit -m "feat: record handoff lifecycle events and add handoff:history"
```

## Global constraints
Node ≥18, zero deps. Use `readInput(args)` for CLI input. `node --test` green. Do not change unrelated behavior; the `created` event must fire only on a fresh publish, not on the idempotent re-publish path.
