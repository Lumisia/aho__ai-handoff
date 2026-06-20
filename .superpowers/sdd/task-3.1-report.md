# Task 3.1 Report — Claude status-line segment + config key

## Per-file changes

### Created: `core/lib/statusline-segment.mjs`
Pure function `statuslineSegment({ usedPercent, cwd, show })`.
- Returns `''` when `show` is false.
- Calls `statusFor(cwd)` from `core/hooks/handoff.mjs` to count pending capsules.
- Returns `"AH 82%"` when no pending capsule, `"AH 82% · ⏳1"` when one is pending.
- Errors from `statusFor` are silently swallowed (try/catch) so the statusline never crashes.

### Modified: `core/lib/config-edit.mjs`
Added `'statusline.show_handoff': { type: 'boolean' }` to `CONFIG_KEYS`.

### Modified: `config/defaults.json`
Added top-level block `"statusline": { "show_handoff": true }` (valid JSON, comma after preceding key).

### Modified: `core/cli.mjs`
- Added import: `import { statuslineSegment } from './lib/statusline-segment.mjs';`
- Replaced body of `sensorClaudeStatusline()`:
  - Reads config via `loadConfig` / `configPath`.
  - Calls `statuslineSegment` with `usedPercent` from `input.rate_limits.five_hour.used_percentage`, `cwd` from `input.cwd || input.workspace.current_dir`, and `show` from `cfg.statusline.show_handoff !== false`.
  - Calls `runPreviousStatusline(raw)` in a try/catch, writing error to stderr.
  - Outputs `"<seg> | <prev>"` if both exist, `"<seg>\n"` if only segment, or `prev` if segment is empty.

### Created: `tests/statusline-segment.test.mjs`
Three tests matching the brief exactly:
1. `statuslineSegment shows usage only when no pending capsule` — empty tmp dir → `"AH 82%"`.
2. `statuslineSegment adds pending marker when a capsule is pending` — runs `handoff:checkpoint` via `execFileSync`, then checks `"AH 82% · ⏳1"`.
3. `statuslineSegment returns empty when show is false` → `""`.

## Test commands and output

### Failing test (Step 2)
```
node --test tests/statusline-segment.test.mjs
# → ERR_MODULE_NOT_FOUND (as expected)
```

### Segment tests after implementation (Step 6)
```
node --test tests/statusline-segment.test.mjs
✔ statuslineSegment shows usage only when no pending capsule (48.917ms)
✔ statuslineSegment adds pending marker when a capsule is pending (247.072ms)
✔ statuslineSegment returns empty when show is false (0.16ms)
tests 3, pass 3, fail 0
```

### Full suite (Step 6)
```
node --test
tests 160, pass 159, fail 0, skipped 1 (pre-existing live app-server SKIP)
```

## Concerns
None. All three new tests pass. Zero regressions in the full suite. The one skipped test (`reads live rate limit from codex app-server`) was already skipped before this task.
