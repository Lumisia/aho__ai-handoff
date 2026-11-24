---
name: handoff-checkpoint
description: Use /handoff checkpoint when the user asks to save the current work state for a later agent handoff.
argument-hint: "[md|json] [summary]"
disable-model-invocation: true
---

# handoff-checkpoint

Save a capsule immediately without waiting for the automatic usage threshold.
This command is executed by the current agent, not by the user.

## Required flow

1. Read the optional first argument. `/handoff checkpoint md` selects Markdown;
   `/handoff checkpoint json` selects JSON. Otherwise use the configured default.
2. Resolve the current contract before writing any file:

       ai-handoff checkpoint guidance --agent <self> --json

   For an explicit suffix, add `--format md` or `--format json`.
3. Treat the guidance JSON as authoritative. Follow its `agent`, `language`,
   `input_format`, `storage_format`, `limits`, `input_template`, and `command`.
   Do not substitute remembered defaults. Keep every list at or below its
   returned limit; daemon trimming is only a safety net.
4. Write the current work state to a temporary file using `input_template`.
   Natural-language content must use the returned `language`. For Markdown,
   preserve the returned headings and use bullets or numbered items. For JSON,
   preserve the English field names shown by the template.
5. Run the returned `command`. For a save requested by a threshold prompt,
   append the `--episode <id>` supplied by that prompt. Append `--target <agent>`
   only when the user explicitly names a consumer.
6. Report the saved capsule id or path briefly.

Use `--file` rather than piping stdin. This is reliable in PowerShell and POSIX
shells. A summary argument supplies the goal but does not replace available
done, remaining, risks, or next-prompt context.

## Automatic threshold prompt

In ask mode, use the question tool named by the current hook when it is
available, or show the same choices once in chat. Wait for the actual answer.
The hook tracks when this session becomes eligible for another question.

- **Yes / 예 / はい** — Save the capsule and resume the interrupted work.
- **No / 아니오 / いいえ** — Skip the capsule and resume.
- **Other / 기타 / その他** — Follow the user's free-text instruction. Save only if requested.
- In automatic mode, save the capsule and resume the interrupted work.

Never include secrets, credentials, or raw transcript text in the capsule.
