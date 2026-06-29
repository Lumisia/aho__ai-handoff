---
name: handoff-doctor
description: Use /handoff doctor to diagnose the ai-handoff install, plugin state, hooks, daemon, IPC, and capsule health.
disable-model-invocation: true
---

# handoff-doctor

Run a read-only health check of the ai-handoff installation and report findings.

## Usage

    ai-handoff doctor
    ai-handoff doctor --json
    aho doctor

Invoke from the skill list as the handoff doctor entry. The user-facing command is
`/handoff doctor`; run `ai-handoff doctor` or `ai-handoff doctor --json`.

## What it reports

- Install health: binary, plugin, hook, and statusline wiring.
- Data root: the directory where capsules are stored.
- Project fingerprint: git remote, git root, or path fallback.
- Pending capsules and capsule integrity issues.
- Other-fingerprint capsules that may explain missing handoffs.

Do not consume, rewrite, or delete capsules during diagnosis.
