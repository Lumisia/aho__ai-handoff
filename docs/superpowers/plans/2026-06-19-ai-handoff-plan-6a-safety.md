# Plan 6a — Safety and Bidirectional Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development task-by-task.

**Goal:** Make automatic handoff genuinely bidirectional and reject unsafe capsules.

**Architecture:** Add a persisted Claude status-line sensor, deterministic ask state, two-pass Stop generation, and verified prepare/finalize delivery. Keep filesystem state atomic and dependency-free.

**Tech Stack:** Node.js 18+, `node:test`, JSON files.

**Status:** COMPLETE — implemented with focused red/green tests and repository regression coverage.

## Global Constraints

- Tests fail for the intended missing behavior before production edits.
- Never overwrite a published capsule payload.
- Never consume before output delivery succeeds.
- No global settings mutation in tests; use temp roots.

### Task 1: Claude status-line sensor

**Files:** Create `core/sensors/claude-statusline.mjs`; modify `core/lib/paths.mjs`, `core/cli.mjs`; test `tests/claude-statusline.test.mjs`.

**Interfaces:** `recordClaudeRateLimit(input,{now})`; `readClaudeRateLimit({sessionId,freshnessMs,now})` returning the common sensor shape or `null`.

- [ ] Write tests for valid input, missing rate limit, wrong session, and stale capture.
- [ ] Run `node --test tests/claude-statusline.test.mjs`; expect module-not-found failure.
- [ ] Implement atomic per-session storage and CLI bridge input.
- [ ] Re-run focused and full tests; expect pass.

### Task 2: Ask state and commands

**Files:** Create `core/capsule/approval.mjs`; modify `core/hooks/stop.mjs`, `core/hooks/handoff.mjs`, `core/cli.mjs`; test `tests/approval.test.mjs`, `tests/stop.test.mjs`, `tests/cli-handoff.test.mjs`.

**Interfaces:** `saveApproval`, `findApproval`, `resolveApproval`; CLI `handoff:create`, `handoff:skip`.

- [ ] Write tests proving `AWAITING_USER` persists and repeated Stop does not erase it.
- [ ] Run focused tests; expect missing exports/commands.
- [ ] Implement state persistence, notification call, create, and skip.
- [ ] Run focused and full tests; expect pass.

### Task 3: Capsule verification and claim recovery

**Files:** Modify `core/capsule/store.mjs`, `core/hooks/session-start.mjs`, `core/cli.mjs`; test `tests/store-verify.test.mjs`, `tests/store-claim.test.mjs`, `tests/session-start.test.mjs`.

**Interfaces:** `verifyStoredCapsule`; `prepareSessionStart`; `finalizeSessionStart`; `abortSessionStart`.

- [ ] Write tests for schema/hash/target/fingerprint rejection, stale Git warning, expired claim recovery, and delayed consume.
- [ ] Run focused tests; expect missing behavior.
- [ ] Implement verification, recovery, and prepare/finalize lifecycle.
- [ ] Run focused and full tests; expect pass.

### Task 4: Two-pass automatic semantic capsule

**Files:** Modify `core/hooks/stop.mjs`, `core/capsule/checkpoint.mjs`, `core/cli.mjs`; test `tests/stop-semantic.test.mjs`, `tests/cli-hooks.test.mjs`.

**Interfaces:** first Stop returns `request-summary`; second Stop parses `HANDOFF_CAPSULE_JSON` and publishes `AVAILABLE`, otherwise `DEGRADED_AVAILABLE`.

- [ ] Write first/second Stop and no-loop tests.
- [ ] Run focused tests; expect failures.
- [ ] Implement sentinel extraction and platform-compatible Stop JSON output.
- [ ] Run full tests; expect pass.
