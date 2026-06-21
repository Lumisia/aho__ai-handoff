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
- `/handoff doctor` -> diagnose capsule integrity, claim recovery, and approval state.
- `/handoff recent` -> list recent capsules across all projects, newest first.
- `/handoff remember` -> store one verified durable fact with concrete evidence.
- `/handoff recall` -> retrieve relevant verified memory without consuming it.
- `/handoff config` -> show or change settings (threshold, mode, notification, memory).

Run the underlying CLI. Pass the project directory with the `--cwd` flag rather
than embedding it in JSON — argv keeps backslashes literal, so Windows paths
need no escaping. If you omit `--cwd`, the CLI uses the process working
directory. `status`, `preview`, `skip`, `create`, and `doctor` then need
nothing on stdin:

    node <pluginRoot>/core/cli.mjs handoff:status --cwd "<project dir>"

For payloads with rich JSON (`checkpoint`, `memory:remember`), do NOT pipe the
JSON through the shell on Windows: PowerShell prepends a UTF-8 BOM and mangles
backslashes. Write the JSON to a UTF-8 file with your native file API and pass
its path with `--input`:

    node <pluginRoot>/core/cli.mjs handoff:checkpoint --agent <agent> --input <file.json>

where `<file.json>` holds
`{"cwd":"<cwd>","session_id":"<id>","sentinel":{"goal":"...","next_actions":["..."]}}`.
Piping JSON on stdin still works (a leading BOM is stripped automatically) and
is fine on macOS/Linux:

    echo '{"cwd":"<cwd>","sentinel":{...}}' | node <pluginRoot>/core/cli.mjs handoff:checkpoint --agent <agent>

`<agent>` must be your own runtime identity: `claude-code` on Claude Code or
`codex` on Codex. These are the only accepted values; any other string (e.g.
`claude`) fails capsule validation.

For `create`, use `handoff:create` and the same sentinel. For `skip` and
`doctor`, use `handoff:skip` and `handoff:doctor`.

For `recent`, call `handoff:recent` (optionally `--limit <n>`, default 10). It
scans every project bucket and returns recent capsules newest-first with status,
source→target, goal, branch, and a `current` flag for this project's bucket:

    node <pluginRoot>/core/cli.mjs handoff:recent --cwd "<project dir>" --limit 10

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
