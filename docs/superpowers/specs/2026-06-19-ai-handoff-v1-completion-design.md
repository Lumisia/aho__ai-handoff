# AI Handoff v1 Completion Design

## Goal

Finish the repository as a safe, installable Claude Code ↔ Codex handoff plugin. Both agents must automatically run lifecycle hooks, Codex must read its official App Server rate limit, Claude must read a fresh status-line bridge sample, capsules must be verified before injection, and verified memory must be recalled only when relevant.

## Boundaries

- Node 18+, zero runtime npm dependencies.
- Local files only; no broker, daemon, OAuth polling, transcript upload, or hidden reasoning.
- User instructions, live files, and Git always outrank capsule and memory content.
- Plugin installation must not silently replace an existing Claude status line. A one-time setup command records and chains the previous command.
- Hook trust and workspace trust remain controlled by Claude Code and Codex.
- No commit, push, marketplace installation, or global settings mutation during implementation.

## 6a — Safety and bidirectional core

Claude status-line input is recorded per session with capture time. Claude Stop reads only a matching, fresh sample. Codex Stop keeps App Server primary and JSONL fallback. `ask` stores `AWAITING_USER`; `/handoff create` and `/handoff skip` resolve it deterministically. `auto` requests one semantic continuation using `stop_hook_active`; the second Stop publishes `AVAILABLE`, or a minimal valid fallback publishes `DEGRADED_AVAILABLE`.

Before injection, the receiver validates schema, payload integrity, external SHA, project fingerprint, target agent, expiry, and claim state. Git changes produce a warning rather than rejection. An expired claim returns to `AVAILABLE`. Consumption occurs only after hook output is successfully written.

## 6b — Packaging and automatic hooks

One `hooks/hooks.json` is shared by Claude and Codex. Commands use `${CLAUDE_PLUGIN_ROOT}` because Claude defines it and Codex supplies it for compatibility. A small Node dispatcher identifies Codex through `PLUGIN_ROOT`, forwards stdin unchanged, and runs the appropriate CLI hook.

`.claude-plugin/plugin.json` and `.codex-plugin/plugin.json` expose skills and commands. Codex relies on default `hooks/hooks.json` discovery so its validator-compatible manifest does not need a `hooks` field. A setup command installs/restores the Claude status-line wrapper while preserving any prior command.

## 6c — Memory Lite

Memory shards contain one redacted, user-confirmed or evidence-backed fact. Each shard records project fingerprint, scope, source, evidence, and verification time. Recall uses deterministic lexical/path/branch scoring, rejects unverified shards, respects the configured token budget, and runs once after the first `UserPromptSubmit` per session. Capsule and memory blocks remain visibly separate.

## Error handling

- Missing/stale sensors: no automatic threshold decision.
- Invalid/tampered capsule: `REJECTED`, no injection.
- Temporary output failure: release claim to `AVAILABLE`.
- Expired claim: recover on next pending lookup.
- Invalid semantic sentinel: publish a minimal degraded capsule once; never loop.
- Invalid memory shard: reject before disk write.

## Verification

- Unit tests cover each transition, validation failure, sensor freshness, ask resolution, recall filtering, and hook output.
- Package tests verify both manifests, shared hook paths, dispatcher behavior, and setup preservation.
- Matrix CI runs Node 18/20/22 on Windows, macOS, and Linux.
- Gated live E2E continues to call the installed Codex App Server.

