---
name: handoff-config
description: Use /handoff config to view or change ai-handoff settings shared by Claude Code and Codex.
argument-hint: "[get|set|list] [key] [value]"
disable-model-invocation: true
---

# handoff-config

ai-handoff uses a single unified config file shared by both agents:

    ~/.ai-handoff/config.toml

Invoke from the skill list as the handoff config entry. The user-facing command is
`/handoff config`; to read, run `ai-handoff config get <key>` or `ai-handoff config list`.
To change a value, run `ai-handoff config set <key> <value>` and report the confirmation
line or surface any `error:` output verbatim.

## Common keys

- `triggers.five_hour.enabled`
- `triggers.five_hour.threshold_percent`
- `triggers.five_hour.mode`
- `capsule.format`
- `capsule.next_prompt_max_items`
- `capsule.remaining_max_items`
- `capsule.done_max_items`
- `capsule.risks_max_items`
- `theme.preset`
- `theme.codex_color`
- `theme.claude_color`
- `theme.focus_border_color`
- `theme.selection_bg_color`
- `theme.selection_fg_color`
