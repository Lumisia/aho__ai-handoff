# Plan 6b — Packaging and Automatic Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development task-by-task.

**Goal:** Package the core as installable Claude Code and Codex plugins with automatic lifecycle hooks.

**Architecture:** Both manifests share the repository root and default `hooks/hooks.json`. A Node dispatcher forwards hook stdin to the core CLI. Claude status-line setup is explicit and reversible because plugin default settings cannot define `statusLine`.

**Tech Stack:** Claude Code plugin manifest, Codex plugin manifest, Node.js.

**Status:** COMPLETE — shared automatic hooks, reversible Claude sensor setup, and both manifests validate.

## Global Constraints

- Use `${CLAUDE_PLUGIN_ROOT}` in shared commands; Codex supplies it for compatibility.
- Preserve existing Claude status-line config and output.
- Do not install to personal marketplaces or mutate home settings during tests.

### Task 1: Shared hook dispatcher

**Files:** Create `scripts/run-hook.mjs`, `hooks/hooks.json`; test `tests/run-hook.test.mjs`, `tests/package-hooks.test.mjs`.

- [ ] Write dispatcher and config-shape tests.
- [ ] Run focused tests; expect missing files.
- [ ] Implement SessionStart, Stop, and UserPromptSubmit forwarding.
- [ ] Re-run tests; expect pass.

### Task 2: Claude status-line setup and chaining

**Files:** Create `core/setup/claude-statusline.mjs`; modify `core/cli.mjs`; test `tests/claude-setup.test.mjs`.

- [ ] Write install/idempotence/restore/chaining tests using temp settings.
- [ ] Run focused tests; expect missing module.
- [ ] Implement `setup:claude-statusline` and `setup:claude-statusline --restore`.
- [ ] Re-run tests; expect pass.

### Task 3: Plugin manifests and package validator

**Files:** Create `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, `scripts/validate-package.mjs`; modify `package.json`; test `tests/plugin-package.test.mjs`.

- [ ] Write manifest identity/path/semver tests.
- [ ] Run focused tests; expect missing files.
- [ ] Add manifests and `npm run validate:package`.
- [ ] Run repository validator, Claude validator when available, and full tests.
