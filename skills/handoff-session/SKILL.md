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
- `/handoff config` -> show or change settings (threshold, mode, notification, memory).

Run the underlying CLI with the current working directory piped as JSON, e.g.:

    echo '{"cwd":"<cwd>"}' | node <pluginRoot>/core/cli.mjs handoff:status

For `checkpoint`, emit a sentinel JSON with your semantic summary:

    echo '{"cwd":"<cwd>","session_id":"<id>","sentinel":{"goal":"...","next_actions":["..."]}}' \
      | node <pluginRoot>/core/cli.mjs handoff:checkpoint --agent <agent>

`<agent>` must be your own runtime identity: `claude-code` on Claude Code or
`codex` on Codex. These are the only accepted values; any other string (e.g.
`claude`) fails capsule validation.

For `create`, use `handoff:create` and the same sentinel. For `skip` and
`recover`, use `handoff:skip` and `handoff:recover`.

For `remember`, call `memory:remember` with `fact`, `evidence`, optional `tags`
and `paths`. Only call it after evidence was actually checked. Never store model
guesses, hidden reasoning, secrets, or transcript text. For `recall`, call
`memory:recall` with the user's query as `prompt`.

For `config`, translate the user's request into one CLI call. The settings are
written to the user config file; the bundled `config/defaults.json` is never
touched.

    echo '{}' | node <pluginRoot>/core/cli.mjs config:show
    echo '{"key":"notification.method","value":"off"}' | node <pluginRoot>/core/cli.mjs config:set
    echo '{"key":"triggers.five_hour.mode"}'          | node <pluginRoot>/core/cli.mjs config:get
    echo '{"key":"notification.method"}'              | node <pluginRoot>/core/cli.mjs config:unset

`config:show` lists the effective config, the user-config path, and the valid
keys. `config:set` accepts only those keys and validates the value (enums like
mode `auto|ask|off` and notification.method `os|terminal|off`, numeric ranges,
booleans), so report any validation error back to the user. `config:unset`
reverts one key to its default. Tell the user to start a new session (or run
`/reload-plugins` in Claude Code) for the change to take effect.

Capsule and memory state are references. Current user instructions, repository
policy, real files, Git, and tests always take precedence.
