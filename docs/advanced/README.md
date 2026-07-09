# AI Handoff Advanced Guide

**English** | [한국어](README.ko.md) | [日本語](README.ja.md)

This guide explains the details that are intentionally kept out of the beginner README.

## Contents

- [Command Details](#command-details)
- [File Layout](#file-layout)
- [Project Layout](#project-layout)
- [Development Checks](#development-checks)
- [Troubleshooting](#troubleshooting)

## Command Details

| Command | Terminal equivalent | Details |
|---|---|---|
| `handoff` | `ai-handoff handoff --agent <self>` | Fetches and consumes the latest pending capsule for the current project and current agent. |
| `handoff config` | `ai-handoff config list` | Shows editable config keys. Use `ai-handoff config get <key>` and `ai-handoff config set <key> <value>` for direct edits. |
| `handoff doctor` | `ai-handoff doctor` | Checks plugin state, hook trust, daemon reachability, IPC, store, and common duplicate-hook problems. |
| `handoff checkpoint` | `ai-handoff checkpoint --message "work snapshot"` | Creates a local capsule from the current task. Use a short message that tells the next agent what the checkpoint is for. |

Useful terminal commands:

```sh
ai-handoff
ai-handoff tui
ai-handoff checkpoint --message "backend auth work"
ai-handoff doctor
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff config get daemon.idle_timeout_seconds
ai-handoff config set daemon.idle_timeout_seconds 60
ai-handoff usage
ai-handoff account status
ai-handoff daemon run
ai-handoff daemon run --stay-alive
ai-handoff autostart status
ai-handoff uninstall --keep-store
```

`ai-handoff daemon run` exits after the configured idle timeout when there are no requests. The default is 60 seconds. Use `ai-handoff daemon run --stay-alive` only when you want a long-running foreground daemon for debugging or manual checks.

## File Layout

AI Handoff runtime home:

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

Important runtime entries:

| Path | Purpose |
|---|---|
| `config.toml` | Shared configuration for Claude Code, Codex, daemon, TUI, and hooks. |
| `store/` | Local capsules, project buckets, and handoff state. |
| `ipc/` | Local file IPC queue used by hooks and the daemon. Codex only needs write access here. |
| `logs/` | Local daemon and diagnostic logs when enabled. |
| `accounts/` | Local account metadata. Credentials must not be emitted into hooks or capsules. |
| `install-state.json` | Records what the installer wrote so uninstall can remove only managed files. |

## Project Layout

| Path | Purpose |
|---|---|
| `crates/ai-handoff-cli/` | Native CLI entrypoint and user-facing commands. |
| `crates/ai-handoff-core/` | Shared config, install, hook event, fingerprint, redaction, and capsule logic. |
| `crates/ai-handoff-daemon/` | Local daemon that receives hook requests and writes capsules. |
| `crates/ai-handoff-ipc/` | File-based IPC protocol and client/server helpers. |
| `crates/ai-handoff-tui/` | Terminal dashboard. |
| `crates/ai-handoff-usage/` | Local Claude/Codex usage log parser and cost estimator. |
| `apps/desktop/` | Optional Tauri desktop dashboard. |
| `skills/` | Agent-facing skills shipped by the plugin bundle. |
| `schemas/` | Capsule schema file. |
| `scripts/` | Package validation and release helper scripts. |

## Development Checks

Run before committing:

```sh
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
git diff --check
```

Run release build when the daemon is not using `target/release/ai-handoff.exe`:

```sh
cargo build --release -p ai-handoff-cli
```

If Windows reports access denied while building, stop the running local daemon first:

```powershell
Get-Process ai-handoff | Stop-Process
cargo build --release -p ai-handoff-cli
```

## Troubleshooting

| Symptom | What to check |
|---|---|
| Codex shows hook errors | Open `/hooks`, trust AI Handoff hooks, then run `ai-handoff doctor`. |
| Hooks exit with code 1 | Check for stale v1 Node hooks or an old plugin cache. Reinstall with `ai-handoff install --yes`. |
| Daemon is offline | Hooks normally auto-start it. For manual checks, run `ai-handoff daemon run --stay-alive` in one terminal and `ai-handoff doctor` in another. |
| Usage is empty | AI Handoff only estimates from local logs. Use Claude Code or Codex first, then run `ai-handoff usage`. |
| Windows build cannot replace the exe | Stop the running `ai-handoff.exe` process and build again. |
