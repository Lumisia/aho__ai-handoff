---
name: handoff-session
description: Use when the user runs /handoff or wants to resume, inspect, create, skip, recover, remember, or recall cross-agent state.
---

# handoff-session

Backs the `/handoff` command for both Claude Code and Codex.

- `/handoff` (bare) -> resume: ingest the pending capsule and continue.
- `/handoff status` -> show whether a capsule is pending.
- `/handoff preview` -> show the pending capsule without consuming it.
- `/handoff checkpoint` -> author a rich capsule now (provide goal + next_actions).
- `/handoff create` -> approve the pending ask and author a rich capsule.
- `/handoff skip` -> decline the pending ask for this usage window.
- `/handoff recover` -> diagnose capsule integrity, claim recovery, and approval state.
- `/handoff remember` -> store one verified durable fact with concrete evidence.
- `/handoff recall` -> retrieve relevant verified memory without consuming it.

Run the underlying CLI with the current working directory piped as JSON, e.g.:

    echo '{"cwd":"<cwd>"}' | node <pluginRoot>/core/cli.mjs handoff:status

For `checkpoint`, emit a sentinel JSON with your semantic summary:

    echo '{"cwd":"<cwd>","session_id":"<id>","sentinel":{"goal":"...","next_actions":["..."]}}' \
      | node <pluginRoot>/core/cli.mjs handoff:checkpoint --agent <agent>

For `create`, use `handoff:create` and the same sentinel. For `skip` and
`recover`, use `handoff:skip` and `handoff:recover`.

For `remember`, call `memory:remember` with `fact`, `evidence`, optional `tags`
and `paths`. Only call it after evidence was actually checked. Never store model
guesses, hidden reasoning, secrets, or transcript text. For `recall`, call
`memory:recall` with the user's query as `prompt`.

Capsule and memory state are references. Current user instructions, repository
policy, real files, Git, and tests always take precedence.
