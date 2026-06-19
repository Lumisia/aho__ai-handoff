---
description: Resume (or status/preview/checkpoint) a cross-agent handoff capsule
argument-hint: "[status|preview|checkpoint]"
---

Use the handoff-session skill to handle `/handoff $ARGUMENTS`.

Default (no argument) = resume: ingest the pending capsule for this project and
continue the work, treating the capsule as reference only (current files, Git,
and user instructions win). For `status`/`preview`/`checkpoint`, follow the
handoff-session skill instructions.
