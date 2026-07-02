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

    ai-handoff checkpoint --agent <self> --file <path-to.json>
    aho checkpoint --agent <self> --file <path-to.json>

Always pass `--agent` set to the agent you are: `claude-code` if you are Claude
Code, `codex` if you are Codex. It sets the handoff direction (source → target).
Omitting it defaults the source to codex, which records the wrong direction when
Claude Code runs the checkpoint.

Always supply a JSON capsule body unless the user explicitly asks for a terse
message-only checkpoint. Write the JSON to a file and pass `--file`, which is
robust across shells. PowerShell does not pipe to a native
executable's stdin, so `<json> | ai-handoff checkpoint` silently drops the body
and only `--message` survives. On POSIX shells stdin still works.

JSON fields (top level): `goal`, `done` (array), `remaining` (array),
`risks` (array), `next_prompt` (string), optional `agent`. The daemon trims each
field using the shared config limits:

- `capsule.language`
- `capsule.next_prompt_max_items`
- `capsule.remaining_max_items`
- `capsule.done_max_items`
- `capsule.risks_max_items`

Before writing the JSON, read `capsule.language` with
`ai-handoff config get capsule.language` when practical. Write the natural
language values in `goal`, `done`, `remaining`, `risks`, and `next_prompt` in
that language: `ko`, `ja`, `zh`, or `en`. Keep JSON key names in English. If the
setting cannot be read, default to English. Existing capsules are not
translated by the daemon.

Invoke from the skill list as the handoff checkpoint entry. The user-facing command is
`/handoff checkpoint <goal>`; create a short JSON file with non-empty fields when
the information is available, run `ai-handoff checkpoint --agent <self> --file <path-to.json>`,
and report the capsule ID on success. Use `--message` only when no done /
remaining / risks / next_prompt detail is available.

Never include secrets, credentials, or raw transcript text in the JSON or message.
