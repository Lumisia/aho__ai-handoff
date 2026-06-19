---
name: handoff-session
description: Use when the user runs /handoff or wants to resume, preview, check status of, or manually checkpoint a cross-agent handoff capsule.
---

# handoff-session

Backs the `/handoff` command for both Claude Code and Codex.

- `/handoff` (bare) -> resume: ingest the pending capsule and continue.
- `/handoff status` -> show whether a capsule is pending.
- `/handoff preview` -> show the pending capsule without consuming it.
- `/handoff checkpoint` -> author a rich capsule now (provide goal + next_actions).

Run the underlying CLI with the current working directory piped as JSON, e.g.:

    echo '{"cwd":"<cwd>"}' | node <pluginRoot>/core/cli.mjs handoff:status

For `checkpoint`, emit a sentinel JSON with your semantic summary:

    echo '{"cwd":"<cwd>","session_id":"<id>","sentinel":{"goal":"...","next_actions":["..."]}}' \
      | node <pluginRoot>/core/cli.mjs handoff:checkpoint --agent <agent>

Capsule state is a reference. Current user instructions, real files, and Git
always take precedence over the capsule.
