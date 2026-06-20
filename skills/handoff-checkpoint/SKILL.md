---
name: handoff-checkpoint
description: Manually save a handoff capsule now. Pass a short goal description as the argument.
---

Use the handoff-session skill to author a capsule now via `handoff:checkpoint`.
Build a sentinel JSON with the current `goal` and `next_actions` (use the
provided ARGUMENTS as a hint for the goal when present), then publish it.

Pass `--agent` set to your own runtime: `claude-code` on Claude Code, `codex` on
Codex. Those are the only valid agent values. Never include secrets, hidden
reasoning, or transcript text.
