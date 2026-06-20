---
name: handoff
description: Resume, create, diagnose, or recall a cross-agent handoff. Accepts [status|preview|checkpoint|create|skip|recover|remember|recall|config] as an argument.
---

Use the handoff-session skill to handle `/handoff` with the provided ARGUMENTS.

Default (no argument) = resume: ingest the pending capsule for this project and
continue the work, treating the capsule as reference only (current files, Git,
tests, and user instructions win). Route every subcommand through the
handoff-session skill.
