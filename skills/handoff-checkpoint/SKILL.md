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

    ai-handoff checkpoint --message "<goal summary>"
    aho checkpoint --message "<goal summary>"

For richer handoff detail, send JSON on stdin. The daemon trims each field using
the shared config limits:

- `capsule.next_prompt_max_items`
- `capsule.remaining_max_items`
- `capsule.done_max_items`
- `capsule.risks_max_items`

Invoke from the skill list as the handoff checkpoint entry. The user-facing command is
`/handoff checkpoint <goal>`; run `ai-handoff checkpoint --message "<goal>"` and report
the capsule ID on success.

Never include secrets, credentials, or raw transcript text in the message.
