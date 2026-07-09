<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

# AI Handoff

**English** | [한국어](docs/README.ko.md) | [日本語](docs/README.ja.md)

AI Handoff is a local-first handoff tool for Claude Code and Codex.

When one agent is close to a usage limit, AI Handoff saves the current goal, branch, changed files, notes, and remaining work as a local capsule. The other agent can read that capsule and continue from the same context.

Everything is designed around local files first. Capsules and hook messages stay on your computer.

## Contents

- [Requirements](#requirements)
- [Quick Start](#quick-start)
- [Main Commands](#main-commands)
- [Local Files](#local-files)
- [Usage Numbers](#usage-numbers)
- [Privacy And Safety](#privacy-and-safety)
- [More Documentation](#more-documentation)

## Requirements

You need:

- Claude Code and/or Codex
- macOS, Linux, Windows, or WSL
- one install method: Homebrew, `curl`, PowerShell, Git Bash, or WSL

You do not need Node.js or Rust to use a release build.

## Quick Start

### Homebrew CLI

```sh
brew install Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Homebrew Desktop App

Use this when you want the desktop dashboard too.

```sh
brew install --cask Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Windows (PowerShell)

Run this in PowerShell. It downloads the CLI, adds it to your user PATH, and runs the installer.
By default, `latest` means the highest stable `vX.Y.Z` GitHub Release, not GitHub's "Latest" badge.

**GUI + TUI** — installs the desktop app together with the CLI/TUI:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

**TUI** — installs the CLI/TUI only:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes
```

To pass more options (pick one agent, pin a version), keep the same scriptblock form:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
```

Pin a release when you need repeatable installs:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Version v2.1.7
```

### Shell Installer

Use this on macOS, Linux, WSL, or Git Bash.
By default, `latest` means the highest stable `vX.Y.Z` GitHub Release, not GitHub's "Latest" badge.

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

After install:

1. Restart Claude Code and Codex.
2. In Codex, open `/hooks`.
3. Trust the AI Handoff hooks.
4. Check the install:

```sh
ai-handoff doctor
```

## Main Commands

| Command | What it does | Use when |
|---|---|---|
| `handoff` | Fetches and consumes a pending handoff capsule for the current project. | The other agent left work for you to continue. |
| `handoff config` | Shows or changes shared AI Handoff settings. | You want to change thresholds, modes, language, or display settings. |
| `handoff doctor` | Checks install state, hooks, daemon, IPC, and capsule health. | Hooks fail, Codex shows hook errors, or install looks wrong. |
| `handoff checkpoint` | Saves the current work as a handoff capsule. | You want to hand work to the other agent now. |

You can also run the same actions from a terminal:

```sh
ai-handoff hook session-start --agent <codex|claude-code>
ai-handoff checkpoint --message "work snapshot"
ai-handoff doctor
ai-handoff config list
```

Detailed command docs: [Advanced Guide](docs/advanced/README.md).

## Local Files

AI Handoff creates one local home folder:

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

The beginner view has only three important entries:

| Entry | Meaning |
|---|---|
| `config.toml` | Shared settings for Claude Code and Codex. |
| `store/` | Local capsules and handoff history. |
| `ipc/` | Local message queue used by hooks and the daemon. |

Full project and runtime layout: [Advanced Guide](docs/advanced/README.md#file-layout).

## Usage Numbers

`ai-handoff usage` reads local Claude Code and Codex logs.

Token and cost numbers are estimates from local logs. They are not an official bill, quota, or provider-side usage report.

## Privacy And Safety

| Topic | What AI Handoff does |
|---|---|
| Local-first design | Capsules, config, IPC messages, and usage estimates stay on your computer. |
| Hook data | Hooks send local event data through local IPC. They do not upload your workspace. |
| Account credentials | Account credentials and OAuth tokens are not used by hooks and must not be written to capsules or hook output. |
| Account actions | Account switching belongs in the local CLI/TUI/GUI, not in agent skills. |

## More Documentation

- [Advanced Guide](docs/advanced/README.md)
- [Korean](docs/README.ko.md)
- [Japanese](docs/README.ja.md)

## License

[MIT](LICENSE)
