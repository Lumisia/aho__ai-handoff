# ai-handoff v1.1 Enhancements — Design

- **Date:** 2026-06-20
- **Status:** Approved for planning
- **Target release:** v1.1.0
- **Scope:** One batched spec covering six enhancements to the existing plugin. Each is small-to-medium and shares the same codebase; they ship together as v1.1.0.

## Goals

Deepen the plugin's unique value (usage-limit-aware, integrity-checked cross-agent handoff) and close the most painful operational gaps observed in real use:

1. **doctor** — rename `recover` → `doctor` and expand it to diagnose *why a handoff is not appearing* (wrong directory / fingerprint mismatch), not only whether an already-located capsule is healthy.
2. **statusline** — show a live `AH 82% · ⏳1` segment in the Claude Code status line.
3. **history** — an append-only audit log of handoff lifecycle events, readable via `handoff:history`.
4. **burn-rate trigger** — opt-in predictive trigger that fires before exhaustion based on usage velocity, augmenting (not replacing) the static threshold.
5. **token-optimization report** — investigation only this release: measure where tokens are spent on handoff and list levers. No code changes.
6. **i18n** — localize human-facing output (notifications + CLI output) to en/ko/ja/zh via a `locale` config key.

## Non-goals

- Localizing skill `description:` frontmatter (Claude Code / Codex plugin specs have no per-locale description field; skill descriptions stay English — they primarily drive the agent's skill-matching).
- A Codex custom-statusline adapter (Codex has no external command-backed status line yet; see Feature 2).
- Implementing token optimizations (Feature 5 is measurement only).
- Any new runtime/daemon. The no-daemon, zero-dependency, Node ≥18 constraints hold.

## Shared: configuration additions

New user-config keys (deep-merged over `config/defaults.json`; added to the `CONFIG_KEYS` allowlist in `core/lib/config-edit.mjs` with validation, and settable via `/handoff config`):

| Key | Type | Default | Validation |
| --- | --- | --- | --- |
| `locale` | enum | `en` | one of `en`, `ko`, `ja`, `zh` |
| `triggers.five_hour.burn_rate.enabled` | bool | `false` | boolean |
| `triggers.five_hour.burn_rate.runway_minutes` | int | `30` | 5–120 |
| `statusline.show_handoff` | bool | `true` | boolean |

`config/defaults.json` gains the corresponding default values.

---

## Feature 1 — `recover` → `doctor` (hard rename + expand)

### Purpose
`recover` only inspects the single fingerprint derived from the current cwd and reports whether a capsule *there* is healthy. When the user runs it from a different directory than where the capsule was authored, it returns `pending: null, healthy: true, issues: []` — "nothing here, all clean" — which hides the real problem (a fingerprint/cwd mismatch). `doctor` diagnoses the lookup itself.

### Rename (hard, no alias)
- CLI command `handoff:recover` → `handoff:doctor` (`core/cli.mjs` dispatch map + handler).
- Skill dir `skills/handoff-recover/` → `skills/handoff-doctor/` (`name:` and `description:` updated; description reframed to "diagnose why a handoff is not appearing, including wrong-directory / fingerprint mismatches").
- `recoverFor` → `doctorFor` in `core/hooks/handoff.mjs`.
- Update `skills/handoff-session/SKILL.md`, READMEs (en/ko/ja/zh), and `tests/skills-present.test.mjs` (the file list) plus any test referencing `handoff:recover`.

### Expand `doctorFor(cwd)`
Keep existing fields (`fingerprint`, `healthy`, `issues`, `pending`, `approval`) and add:

- `basis` — `{ type: "remote" | "gitroot" | "path", value: string }`. Requires `core/lib/fingerprint.mjs` to expose the basis. Add `projectFingerprintInfo(cwd)` returning `{ fingerprint, basis }`; `projectFingerprint(cwd)` becomes a thin wrapper that returns `.fingerprint`.
- `cwdResolved` — the realpath the CLI actually resolved (catches cwd-passing bugs).
- `dataRoot` — the resolved store root, plus whether `AI_HANDOFF_ROOT` is overriding it.
- `otherPending` — scan `dataRoot/projects/*/handoff/*/state.json` for `AVAILABLE`/`DEGRADED_AVAILABLE` capsules whose fingerprint ≠ the current one. For each, return `{ fingerprint, taskId, goal, source, branch }` as recognition hints (pulled from the stored `capsule.json`). This is the field that surfaces "your capsule is under a different fingerprint."

### Output
Human-readable, localized (see Feature 6). The `handoff-doctor` skill reports basis, store location, current pending state + issues, and — when `otherPending` is non-empty — explicitly flags the mismatch and names the directory/remote the capsule belongs to.

### Testing
- `doctorFor` returns `basis.type` correctly for a git-remote repo vs a non-repo path.
- `otherPending` lists a capsule authored under a different fingerprint and omits the current one.
- Existing recover behaviors (integrity issues surfaced, approval state) still pass under the new name.

---

## Feature 2 — Claude status line live segment

### Purpose
Surface handoff state at a glance: `AH 82% · ⏳1` (usage % + pending capsule count) or `AH 82%` when idle.

### Design
- `core/sensors/claude-statusline.mjs` already records the rate-limit sample and the `sensor:claude-statusline` CLI command chains the user's previous status line (`runPreviousStatusline`). Add a pure function `statuslineSegment(input)` (new `core/lib/statusline-segment.mjs`) that, given the parsed status-line JSON, returns the short `AH …` string: usage % from the just-recorded sample and pending count from `statusFor(cwd)` when `input.cwd`/workspace is available; usage-only if no cwd.
- The `sensor:claude-statusline` command prepends `statuslineSegment(input)` to the chained previous status line, gated on `statusline.show_handoff`.
- Localized labels via Feature 6 (the `pending`/idle wording).

### Codex
Codex CLI has a **built-in** status line (`/statusline` + `tui.status_line` in `~/.codex/config.toml`) that already shows rate limits and token counters natively, but has **no external command-backed status line** (open upstream requests: openai/codex #20043, #17827, #20140, #20244). Therefore the custom AH segment is **Claude-only** for now. `statuslineSegment(input)` is deliberately a pure, transport-agnostic function so that if Codex ships `[tui.status_line_command]`, a thin `sensor:codex-statusline` adapter can reuse it. Documented as a forward hook.

### Testing
- `statuslineSegment` returns `AH <pct>% · ⏳<n>` when a capsule is pending and `AH <pct>%` when not.
- Returns usage-only (no pending marker) when cwd is absent.
- Returns empty string when `statusline.show_handoff` is false.

---

## Feature 3 — `handoff:history` audit log

### Purpose
A per-project audit trail of handoff lifecycle events.

### Design
- Append-only JSONL at `dataRoot/projects/<fingerprint>/history.jsonl`. Each line: `{ ts, event, taskId, agent, source, target, trigger, observed_percent? }`.
- Events: `created` (publish), `resumed` (consume), `skipped`, `created_from_approval`.
- Add `appendHistory(fingerprint, entry)` (new `core/capsule/history.mjs`) and call it at the lifecycle points in `core/capsule/store.mjs` (`publishCapsule`, `consumeCapsule`) and `core/hooks/handoff.mjs` (`skipApproval`, `createFromApproval`). On write, cap the file to the last 500 lines.
- CLI `handoff:history` reads via `readInput`/`--cwd`, prints the last N (default 20; `--limit` override) entries for the project, localized headers.

### Testing
- A checkpoint→resume cycle appends a `created` then a `resumed` entry.
- `handoff:history` returns the most recent entries for the project, newest last, respecting `--limit`.
- The file is capped at 500 lines.

---

## Feature 4 — burn-rate predictive trigger (opt-in, augment)

### Purpose
Fire the handoff earlier when usage is climbing fast, leaving runway before hard exhaustion (100% = agent dead). Augments the static threshold; whichever condition is reached first fires.

### Sample history
Burn-rate needs ≥2 timestamped samples. Today only the latest sample is kept. Add `core/sensors/samples.mjs`:
- `appendSample(fingerprint, agent, { usedPercent, at })` — keeps the last N (default 6) in `dataRoot/projects/<fingerprint>/samples-<agent>.json`.
- `readSamples(fingerprint, agent)` — returns the array.

Wiring:
- `core/hooks/stop.mjs` (`handleStop`) appends the current reading (it already reads the sensor and has `input.cwd`) before evaluating the trigger.
- `recordClaudeRateLimit` additionally appends a sample when the status-line input carries a cwd (Claude status line fires frequently → richer history). The existing single-sample file for `readClaudeRateLimit` is unchanged.

### Trigger logic
Extend `core/hooks/trigger.mjs` `evaluateTrigger` to accept `{ usedPercent, threshold, mode, deduped, samples, burnRate: { enabled, runwayMinutes }, now }`:
- Static path unchanged: `usedPercent >= threshold` → fire.
- Burn-rate path (only if `burnRate.enabled`): compute velocity from the two most-spread samples (Δ% / Δminutes); project minutes-to-100%. If projection ≤ `runwayMinutes`, fire. Reason `burn-rate` vs `threshold`.
- Insufficient/degenerate samples (fewer than 2, zero/negative slope, stale) → fall back to the static path; reason `insufficient-samples` when below threshold.

### Notification interaction (explicit)
Burn-rate changes *when* the trigger is reached, not *what happens after*. The resulting `create`/`ask` action flows through the existing `handleStop` path and honors the existing `mode` (auto/ask) and `notification.method` (os/terminal/off). Notification stays **trigger-reason-agnostic** — no new alarm key, no separate notification for burn-rate.

### Testing
- With `burn_rate.enabled=false`, behavior is identical to today (regression).
- With it enabled and two samples implying exhaustion within `runwayMinutes`, fires below the static threshold with reason `burn-rate`.
- With slow velocity (projection beyond runway), does not fire below threshold.
- Insufficient samples → static-only fallback.
- A burn-rate-fired `ask`/`create` triggers the same notification path as a threshold-fired one (and is silenced by `notification.method=off`).

---

## Feature 5 — token-optimization report (investigation only)

### Purpose
Measure where tokens are spent on a handoff and enumerate levers, with estimated savings. **No code changes this release.**

### Deliverable
A report committed at `docs/superpowers/notes/2026-06-20-token-budget-report.md` covering:
- Measured size (characters + estimated tokens) of: the capsule injected on resume (`prepareSessionStart` output), the memory-recall injection (`renderMemoryRecall`, currently budgeted at 800), and `skills/handoff-session/SKILL.md`.
- Levers with rough savings and risk: (a) progressive disclosure on resume (inject snapshot/INDEX first, pull details on demand); (b) capping/summarizing long capsule fields; (c) terser injection format; (d) tuning recall/budget defaults.
- A recommendation of which levers are worth a follow-up implementation batch (v1.2 candidate).

### Testing
None (documentation only).

---

## Feature 6 — i18n for human-facing output

### Purpose
Localize what the human reads to en/ko/ja/zh.

### Scope
- In: notification bodies (`core/lib/notify.mjs`), the in-chat ask/summary prompts emitted by `core/cli.mjs` (`hookStop`), and the human-readable CLI output of `doctor`, `history`, and `status`.
- Out: skill `description:` frontmatter (platform limitation — stays English); model-authored capsule content (the agent writes in its own language; not our string).

### Design
- `core/lib/i18n.mjs`: a `messages` catalog keyed by locale (`en`, `ko`, `ja`, `zh`) and a `t(key, vars, locale)` lookup with **en fallback** for any missing key.
- `locale` comes from `loadConfig()` (`config/defaults.json` default `en`).
- Replace the current hardcoded (and currently English+Korean-mixed) strings with `t(...)` calls.

### Testing
- `t()` falls back to `en` for an unknown locale and for a key missing in a non-en catalog.
- **Catalog completeness:** every non-en catalog defines exactly the same key set as `en` (guards against drift).
- A localized notification/prompt renders the expected string per locale.

---

## Cross-cutting

### Sequencing
doctor → history → statusline → burn-rate → i18n → token report. (doctor/history/statusline/i18n are small and independent; burn-rate is the largest because of sample history.)

### Testing & process
Test-driven per feature; the existing `node --test` suite must stay green and `npm run validate:package` must pass. Bump 1.0.7 → **1.1.0** across the three manifests (`.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, `package.json`) — the version string keys the plugin cache, so a bump is required for installs to refetch. Update the four READMEs (en/ko/ja/zh) for the rename, the new `handoff:history`/`doctor` commands, the new config keys, and the i18n/`locale` note.

### Compatibility notes
- The `recover`→`doctor` rename is a breaking change for anyone scripting `handoff:recover`; acceptable per owner decision (no alias).
- All new config keys default to non-disruptive values (`burn_rate.enabled=false`, `locale=en`, `statusline.show_handoff=true`).

### Future hooks
- `sensor:codex-statusline` adapter once Codex ships command-backed status lines (openai/codex #20043).
- Token-optimization implementation batch (v1.2) driven by Feature 5's report.
