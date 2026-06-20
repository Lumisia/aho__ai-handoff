# Task 4.1 Report — Rate-limit sample ring buffer

## Changes Made

Created two new files following TDD approach:

1. **`core/sensors/samples.mjs`** — Implements the sample ring buffer module with:
   - `readSamples(fingerprint, agent) → [{ usedPercent, at }]` — reads last N samples, returns empty array on missing/malformed file
   - `appendSample(fingerprint, agent, { usedPercent, at = Date.now() }, { max = 6 } = {}) → void` — appends sample, keeps last `max` samples, validates `usedPercent` is finite
   - `samplesPath(fingerprint, agent)` — internal helper for file path resolution
   - Stores samples at `<dataRoot>/projects/<fp>/samples-<agent>.json`

2. **`tests/samples.test.mjs`** — Single test case verifying:
   - Appending 8 samples with `max: 6` keeps only the last 6
   - Samples are retained in order (oldest first)
   - Newest sample is correctly included

## Test Execution

### Step 1: Create test (failing)
```bash
node --test tests/samples.test.mjs
```
Result: FAIL (module not found) ✓

### Step 3: Create implementation
Implemented exact spec from task brief.

### Step 4: Test specific module
```bash
node --test tests/samples.test.mjs
```
Result: PASS ✓
```
✔ appendSample keeps the last N samples in order (65.1601ms)
ℹ tests 1
ℹ suites 0
ℹ pass 1
ℹ fail 0
```

### Step 4b: Full regression test
```bash
node --test
```
Result: All 161 tests pass, 1 skipped (previously skipped unrelated test) ✓
```
ℹ tests 161
ℹ suites 0
ℹ pass 160
ℹ fail 0
ℹ cancelled 0
ℹ skipped 1
ℹ todo 0
ℹ duration_ms 4490.741
```

The new test appears in the output as:
```
✔ appendSample keeps the last N samples in order (146.4513ms)
```

## Commit

```
5be0e5e28402cbdfa82fa3c0990c576b4ee28b63
feat: add rate-limit sample ring buffer
```

## Concerns

None. Implementation follows spec exactly, all tests pass, zero dependencies, uses only Node.js built-ins, integrates cleanly with existing path and file helpers.
