---
name: handoff-config
description: Show or change ai-handoff settings. Accepts [show|get|set|unset] [key] [value] as arguments.
---

Use the handoff-session skill to handle `config` with the provided ARGUMENTS.
Map it to `config:show` (no args), `config:get`, `config:set`, or `config:unset`;
pass `key` and `value` as JSON on stdin. Report the result, and surface any
validation error verbatim. Remind the user to start a new session (or run
`/reload-plugins` in Claude Code) for the change to take effect.
