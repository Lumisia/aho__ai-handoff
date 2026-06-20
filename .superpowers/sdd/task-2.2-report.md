# Task 2.2 Report — Record lifecycle events + `handoff:history` command

## Changes

### `core/capsule/store.mjs`
- Extended `node:path` import to include `basename` and `dirname`.
- Added `import { appendHistory } from './history.mjs';`.
- In `publishCapsule`: added `appendHistory(fingerprint, { event: 'created', ... }, { now })` in the fresh-write branch only (after `writeState`, before `expireOtherPending`). The idempotent early-return path is untouched.
- In `consumeCapsule`: added fingerprint derivation via `basename(dirname(dirname(dirname(claim.statePath))))` and `appendHistory(fp, { event: 'resumed', taskId: next.task_id ?? st.task_id }, { now })` after `writeState` and before `releaseLock`.

### `core/hooks/handoff.mjs`
- Added `import { appendHistory } from '../capsule/history.mjs';`.
- In `skipApproval`: added `appendHistory(fp, { event: 'skipped', key: approval.key }, { now })` before the return.
- In `createFromApproval`: added `appendHistory(fp, { event: 'created_from_approval', taskId: capsule.task_id, agent: context.agent }, { now })` after `publishCapsule`.

### `core/cli.mjs`
- Added `import { readHistory } from './capsule/history.mjs';`.
- Added `handoffHistory(args)` function using `readInput`, `argValue` for `--limit`, `projectFingerprint`, and `readHistory`.
- Added `'handoff:history': handoffHistory` to the dispatch table.

### `tests/cli-handoff.test.mjs`
- Added `'handoff:history records created then resumed'` test as specified in the brief.

## Test commands and output

### Failing test (Step 2)
```
node --test tests/cli-handoff.test.mjs
# → ✖ handoff:history records created then resumed
# Error: unknown command: handoff:history
```

### After implementation (Step 6)
```
node --test tests/cli-handoff.test.mjs
# ✔ handoff:checkpoint then status/preview shows pending
# ✔ repeated manual checkpoints in one session supersede instead of colliding
# ✔ handoff:resume injects then consumes
# ✔ handoff:create resolves persisted ask state and publishes capsule
# ✔ handoff:history records created then resumed
# ✔ memory:remember stores evidence and memory:recall returns only relevant memory
# tests 6 / pass 6 / fail 0
```

### Full suite
```
node --test
# tests 157 / pass 156 / fail 0 / skipped 1 (pre-existing skip)
```

## Concerns
None. All constraints met: zero new deps, fresh-write-only `created` event, correct fingerprint derivation from statePath, `readInput` used in CLI handler, full suite green.
