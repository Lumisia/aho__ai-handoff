---
name: handoff-checkpoint
description: Use /handoff checkpoint to manually save a handoff capsule right now. Pass a short goal description as the argument.
argument-hint: "[summary]"
disable-model-invocation: true
---

# handoff-checkpoint

Save a handoff capsule immediately so the other agent can pick up where you left off.
Use this any time you want to preserve current progress without waiting for the
automatic 5-hour threshold.

## Usage

    ai-handoff checkpoint --agent <self> --message "<goal summary>"
    aho checkpoint --agent <self> --message "<goal summary>"

Always pass `--agent` set to the agent you are: `claude-code` if you are Claude
Code, `codex` if you are Codex. It sets the handoff direction (source → target).
Omitting it defaults the source to codex, which records the wrong direction when
Claude Code runs the checkpoint.

For richer handoff detail, supply a JSON capsule body. Write the JSON to a file
and pass `--file`, which is robust across shells:

    ai-handoff checkpoint --agent <self> --file <path-to.json>

Prefer `--file` over piping JSON on stdin. PowerShell does not pipe to a native
executable's stdin, so `<json> | ai-handoff checkpoint` silently drops the body
and only `--message` survives. On POSIX shells stdin still works.

JSON fields (top level): `goal`, `done` (array), `remaining` (array),
`risks` (array), `next_prompt` (string), optional `agent`. The daemon trims each
field using the shared config limits:

- `capsule.next_prompt_max_items`
- `capsule.remaining_max_items`
- `capsule.done_max_items`
- `capsule.risks_max_items`

Invoke from the skill list as the handoff checkpoint entry. The user-facing command is
`/handoff checkpoint <goal>`; run `ai-handoff checkpoint --agent <self> --message "<goal>"`
and report the capsule ID on success.

Never include secrets, credentials, or raw transcript text in the message.
