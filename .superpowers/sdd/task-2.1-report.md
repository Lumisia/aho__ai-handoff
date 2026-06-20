# Task 2.1 Report — History store helper

## Summary
Completed TDD implementation of `core/capsule/history.mjs` and its test suite. All steps followed exactly as specified.

## Files Changed
- **Created:** `core/capsule/history.mjs` — Per-project handoff history store with append/read operations
- **Created:** `tests/history.test.mjs` — Complete test coverage for history functionality

## Implementation Details

### `core/capsule/history.mjs`
Exports two functions:
- `appendHistory(fingerprint, entry, { now = Date.now(), max = 500 })` — Appends a JSON line with timestamp to `dataRoot/projects/<fp>/history.jsonl`, capping file to last `max` entries
- `readHistory(fingerprint, { limit = 20 })` — Returns the last `limit` entries as parsed objects, oldest-first (newest last)

Depends on:
- `projectDir(fingerprint)` from `core/lib/paths.mjs`
- `writeFileAtomic(path, text)` from `core/lib/fsx.mjs`

### `tests/history.test.mjs`
Single comprehensive test validating:
- Multiple appends are written correctly
- File is capped at specified `max` limit
- Entries are returned with `ts` field
- Newest entry is last in the array

## Test Results

### Step 2: Initial Test Run (Expected Fail)
```
✖ appendHistory writes entries and readHistory returns newest last, capped
Error [ERR_MODULE_NOT_FOUND]: Cannot find module 'core/capsule/history.mjs'
```
Status: FAIL as expected ✓

### Step 4: After Implementation
```
✔ appendHistory writes entries and readHistory returns newest last, capped (43.6865ms)
ℹ tests 1
ℹ pass 1
ℹ fail 0
```
Status: PASS ✓

### Full Test Suite
```
node --test
```
Results:
- tests: 156
- pass: 155
- fail: 0
- skipped: 1
- duration_ms: 3876.3723

Status: ALL TESTS PASS ✓

## Commit
```
Commit: f063ea6e4382a49a6aaa70ecee5e89e68845e9a2
Message: feat: add per-project handoff history store
Branch: v1.1-enhancements
```

## Concerns
None. Implementation follows specification exactly, test coverage is complete, and no regressions introduced to existing test suite.
