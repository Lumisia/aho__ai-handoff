---
name: handoff-ratelimit
description: Detects when the agent's 5-hour usage limit crosses the configured threshold and creates a handoff capsule so the other agent (Claude Code <-> Codex) can continue.
---

# handoff-ratelimit

This skill is driven by the Stop hook. When the 5-hour usage limit reaches the
configured `threshold_percent` (default 80), the hook creates a capsule via
`core/cli.mjs hook:stop`.

- `auto` mode: one Stop continuation requests a strict semantic sentinel, then publishes it.
- `ask` mode: the agent is prompted to run `/handoff create` or `/handoff skip`.
- `off` mode: no automatic detection.

Thresholds and modes live in `config.json` (see `config/defaults.json`). The
capsule is published to the shared store; the other agent ingests it on
SessionStart or via `/handoff`.
