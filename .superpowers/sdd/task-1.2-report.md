# Task 1.2 Report — Rename recover → doctor and expand diagnosis

## Status: DONE

## What changed per file

### `core/hooks/handoff.mjs`
- Added imports: `projectFingerprintInfo` from fingerprint.mjs, `readdirSync`, `readFileSync`, `existsSync`, `realpathSync` from node:fs, `join` from node:path, `dataRoot` from paths.mjs.
- Removed `recoverFor` function.
- Added `scanOtherPending(currentFp)` — walks `dataRoot()/projects/`, skips current fingerprint, collects AVAILABLE/DEGRADED_AVAILABLE capsules from other project dirs.
- Added `doctorFor(cwd, { now })` — uses `projectFingerprintInfo` for `{ fingerprint, basis }`, resolves cwd, calls existing `findPendingCapsule`/`verifyStoredCapsule`/`findApproval`, and calls `scanOtherPending`. Returns full diagnostic object with `fingerprint, basis, cwdResolved, dataRoot, healthy, issues, pending, approval, otherPending`.

### `core/cli.mjs`
- Changed import from `recoverFor` to `doctorFor`.
- Renamed handler `handoffRecover` → `handoffDoctor`, body uses `doctorFor`.
- Dispatch map: `'handoff:recover'` → `'handoff:doctor'`.

### `skills/handoff-recover/SKILL.md` → `skills/handoff-doctor/SKILL.md`
- Renamed via `git mv skills/handoff-recover skills/handoff-doctor`.
- Rewrote SKILL.md: name, description, and body updated to reflect `handoff:doctor`, basis/dataRoot/cwdResolved fields, and cross-fingerprint `otherPending` guidance.

### `skills/handoff-session/SKILL.md`
- Changed `/handoff recover` → `/handoff doctor` in the command list.
- Changed `handoff:recover` → `handoff:doctor` in the "For skip and recover" line.
- Changed `recover` → `doctor` in the list of commands that need nothing on stdin.

### `tests/cli-doctor.test.mjs` (new)
- Created per brief spec: verifies `handoff:doctor` reports `basis.type === 'path'` for a non-git tmp dir, `pending === null` (no capsule in projB), and `otherPending.length === 1` with `goal === 'find me'` from a capsule created for projA.

### `tests/handoff-query.test.mjs`
- Changed import from `recoverFor` to `doctorFor`.
- Renamed test: `'recover diagnoses...'` → `'doctor diagnoses...'`, usage `recoverFor` → `doctorFor`.

### `tests/skills-present.test.mjs`
- Changed `'skills/handoff-recover/SKILL.md'` → `'skills/handoff-doctor/SKILL.md'`.

### `README.md`, `README.ko.md`, `README.ja.md`, `README.zh.md`
- Changed `handoff-recover` → `handoff-doctor` in the skills description paragraph (line 180).
- Changed `| /handoff recover |` → `| /handoff doctor |` in the command table (line 204).

## Test commands and output

### Failing test (Step 2)
```
node --test tests/cli-doctor.test.mjs
# Exit code 1
# unknown command: handoff:doctor
# ✖ handoff:doctor reports basis and cross-fingerprint pending capsules (238.6431ms)
# tests 1, pass 0, fail 1
```

### Target tests after implementation (Step 7)
```
node --test tests/cli-doctor.test.mjs tests/skills-present.test.mjs
# ✔ handoff:doctor reports basis and cross-fingerprint pending capsules (280.8693ms)
# ✔ skill files exist with frontmatter (1.8887ms)
# tests 2, pass 2, fail 0
```

### Full suite (Step 7)
```
node --test
# tests 155, pass 154, fail 0, cancelled 0, skipped 1
# (Skipped: reads live rate limit from codex app-server — always skipped, pre-existing)
```

## git grep result
```
git grep -n "handoff:recover\|recoverFor\|handoff-recover"
```
Returns matches only in:
- `docs/superpowers/plans/2026-06-20-ai-handoff-v1.1-enhancements.md` — the plan document that describes the rename (historical; appropriate to preserve)
- `docs/superpowers/specs/2026-06-20-ai-handoff-v1.1-enhancements-design.md` — the design spec that describes the rename (historical; appropriate to preserve)

No matches in `tests/`, `core/`, `skills/`, or `README*.md`.

## Commit hash
02f0d40

## Concerns
None. The only remaining git grep matches are in `docs/superpowers/` plan and spec files that explicitly describe the `recover → doctor` rename. These are historical documentation artifacts and must preserve the old names to make sense. All production code, tests, skills, and READMEs are fully updated.
