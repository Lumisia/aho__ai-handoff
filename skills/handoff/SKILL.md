---
name: handoff
description: Cross-agent ai-handoff command. Use /handoff checkpoint, /handoff doctor, or /handoff config.
---

# handoff

Backs the `/handoff` command in Claude Code and Codex. ai-handoff is a native Rust
daemon that creates a cross-agent handoff capsule when you approach the 5-hour usage
limit, so the other agent can continue from the saved state.

The short alias `aho` is installed on PATH and is equivalent to `ai-handoff` in every
context below. Treat the text after `/handoff` as the sub-command.

## Sub-commands

- `/handoff checkpoint <summary>`: save a handoff capsule now with the given summary.
- `/handoff doctor`: run a read-only install and capsule health diagnosis.
- `/handoff config`: view or change the unified ai-handoff settings.

If `/handoff` is invoked without a sub-command, inspect pending capsules for the
current project and help the user resume the most relevant one without consuming
anything destructive.

Do not invent state. Prefer running the matching CLI command and summarizing its
real output.

## Underlying CLI

The daemon and all management operations are driven by `ai-handoff` (alias `aho`):

    ai-handoff doctor
    ai-handoff daemon
    ai-handoff checkpoint --message "..."
    ai-handoff config list
    ai-handoff dashboard

Capsule and memory state are references only. Current user instructions, repository
files, Git history, and tests always take precedence over capsule content.
