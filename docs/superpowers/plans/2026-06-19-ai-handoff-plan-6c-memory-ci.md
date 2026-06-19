# Plan 6c — Memory Lite, CI, and Documentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development task-by-task.

**Goal:** Store verified project facts, recall relevant facts once, and ship reproducible validation.

**Architecture:** Memory uses small immutable JSON shards and deterministic ranking. UserPromptSubmit injects only verified, relevant shards within budget. CI runs the same package and unit validation on all supported platforms.

**Tech Stack:** Node.js 18+, `node:test`, GitHub Actions.

**Status:** COMPLETE — verified shards, relevant first-prompt recall, CI matrix, and operator docs are present.

## Global Constraints

- No transcript or hidden-reasoning storage.
- Redact before validation and write.
- Auto recall requires verified evidence and a positive relevance score.

### Task 1: Memory shard store

**Files:** Create `schemas/memory-shard.schema.json`, `core/memory/store.mjs`; modify `core/lib/paths.mjs`; test `tests/memory-store.test.mjs`.

- [ ] Write valid/invalid/redaction/immutability tests.
- [ ] Run focused tests; expect missing module.
- [ ] Implement schema validation and atomic write.
- [ ] Re-run tests; expect pass.

### Task 2: Recall and UserPromptSubmit

**Files:** Create `core/memory/recall.mjs`, `core/hooks/user-prompt.mjs`; modify `core/cli.mjs`, `hooks/hooks.json`; test `tests/memory-recall.test.mjs`, `tests/user-prompt.test.mjs`.

- [ ] Write relevance, evidence, budget, and once-per-session tests.
- [ ] Run focused tests; expect failures.
- [ ] Implement deterministic ranking and hook output.
- [ ] Run focused and full tests; expect pass.

### Task 3: Commands, CI, and docs

**Files:** Modify `skills/handoff-session/SKILL.md`, `commands/handoff.md`, `README.md`, `package.json`; create `.github/workflows/ci.yml`.

- [ ] Extend package tests for documented commands and CI matrix.
- [ ] Run focused tests; expect failure.
- [ ] Document setup, trust, create/skip/recover/remember/recall; add Node 18/20/22 × three-OS CI.
- [ ] Run `npm test`, `npm run validate:package`, plugin validators, and gated Codex E2E.
