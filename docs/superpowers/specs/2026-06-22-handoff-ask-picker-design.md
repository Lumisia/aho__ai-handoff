# Handoff "create/skip" ask → human-facing picker

Date: 2026-06-22

## Problem

When the 5-hour trigger fires in `ask` mode, the Stop hook emits
`{"decision":"block","reason":"Create a capsule? /handoff create | /handoff skip"}`
([core/cli.mjs](../../../core/cli.mjs) `hookStop`, `ask` branch).

`decision:block`'s `reason` is a **model-facing continuation prompt** in *both*
Claude Code and Codex (verified against
[developers.openai.com/codex/hooks](https://developers.openai.com/codex/hooks)
and the Claude Code hook contract). So the question is delivered to the agent,
not to the human: the agent reads the slash-command text and can resolve it
itself — "the agent decides instead of me." Under Codex the literal text is even
less useful because Codex never renders it as a human choice.

The desired UX is a real selectable question to the **human**: option 1 = Yes
(create), option 2 = No (skip), plus a free-text "Other" for a custom request —
i.e. Claude Code's `AskUserQuestion` picker, and the equivalent on Codex.

## Key constraint

A Stop hook is a subprocess. It **cannot** open a picker directly — pickers are
**tools the model calls**:

- Claude Code: `AskUserQuestion` tool.
- Codex 0.141.0: `request_user_input` tool (app-server
  `tool/requestUserInput`). Native in **Plan mode**; in **Default mode** it is
  gated behind the experimental `default_mode_request_user_input` feature flag
  (handler rejects the call in Default mode unless the flag is on). Verified:
  [request_user_input_spec.rs](https://github.com/openai/codex/blob/rust-v0.141.0/codex-rs/core/src/tools/handlers/request_user_input_spec.rs),
  [Issue #24750](https://github.com/openai/codex/issues/24750),
  [PR #12735](https://github.com/openai/codex/pull/12735).

So the hook must keep using `decision:block` + `reason`, but change `reason`
from the *question* into an **instruction to the model to ASK the human** (via
the picker) and **not decide**. The model becomes a question-renderer, not the
decider. The human's choice drives the existing `/handoff create | /handoff skip`
path. This preserves the core requirement (human decides) within the only
mechanism the harness offers.

## Design

Agent-specific continuation prompt. `hookStop` already knows the agent
(`--agent`, value `claude-code` or `codex`).

### Claude path (`ask.instruct.claude`)
Instruct the model to call `AskUserQuestion` once: header "Handoff capsule",
question "Create a capsule?", options Yes (recommended) / No. Note the tool adds
"Other" automatically (do not add your own). Map: Yes → `/handoff create`,
No → `/handoff skip`, Other → treat the free text as capsule requirements (if
clear, create accordingly; if unclear, ask once for only the missing detail).
Forbid running either command before the human answers.

### Codex path (`ask.instruct.codex`)
Same shape, but: call `request_user_input` **if available**; do **not** add an
"Other" option (the client adds a free-text one and `normalize_*` forces
`is_other = true`). If the tool is unavailable or refused, fall back to a
one-line text question (`Yes / No / Other`) and wait. Same answer mapping.

### Reused unchanged
- Approval persistence ([core/capsule/approval.mjs](../../../core/capsule/approval.mjs),
  `saveApproval` → `AWAITING_USER`). The choice can be resolved any time, so a
  picker timeout or a missed turn never strands the user.
- `/handoff create` / `/handoff skip` handlers, dedupe, the OS notification
  (still the short `ask.create_or_skip` text).

## Safety / non-goals
- **Text fallback is the default safe path.** The native picker is a
  progressive enhancement, never a hard dependency.
- The plugin **must not** edit the user's `config.toml` or auto-enable the
  Codex feature flag. Enabling Default-mode `request_user_input` stays a
  documented, opt-in, user-run step (README).
- `request_user_input`'s `autoResolutionMs` auto-resolve must not silently
  create a capsule: rely on persisted approval; the timeout default must not be
  "create".

## Changes
- [core/lib/i18n.mjs](../../../core/lib/i18n.mjs): add `ask.instruct.claude` and
  `ask.instruct.codex` for en/ko/ja/zh; add `askInstruction(agent, locale)`
  helper.
- [core/cli.mjs](../../../core/cli.mjs): `ask` branch → `reason:
  askInstruction(agent, locale)`. Keep `decision:block`.
- Tests: `askInstruction` agent mapping + locale fallback; CLI ask branch wiring
  (codex + claude-code) emits `decision:block` with the right instruction; OS
  notification body unchanged; e2e `auto`-mode path unaffected.
- README: optional Codex feature-flag note (opt-in).
