**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [中文](README.zh.md)

# claude-codex-auto-handoff

A plugin that carries work between Claude Code and Codex.

When one tool gets close to its 5-hour usage limit, the plugin saves the current work state into a small file called a **capsule**. The other tool can then read that capsule and continue from the same point.

The plugin's internal name is `ai-handoff`.

Need help or more details? [Click here](docs/advanced/README.md).

## Why use it?

Claude Code and Codex each have a 5-hour usage limit. When one runs out in the middle of work, you usually have to explain the goal, changed files, and next steps again in the other tool.

This plugin prepares that handoff for you.

## What goes into a capsule?

- Current goal
- Completed work
- Remaining work
- Changed files
- Current Git branch and commit
- Notes the next tool should check first

A capsule is marked as consumed after it is used once.

## Requirements

- Node.js 18 or newer
- Claude Code and/or Codex
- Use both tools for two-way handoff

Check Node:

```bash
node --version
```

## Install

### Claude Code

Run inside Claude Code:

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

Or run in a terminal:

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

Then run `/reload-plugins` or restart Claude Code.

### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

## Extra Claude Code setup

Claude Code usage is read from the status line, so run this once.

You need a local folder that contains `core/cli.mjs`. The easiest way is to clone this repository:

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

Then move into the plugin folder and run the setup command.

Windows PowerShell:

```powershell
cd "C:\path\to\claude-codex-auto-handoff"
$PLUGIN_ROOT = (Get-Location).Path
node "$PLUGIN_ROOT\core\cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

macOS/Linux:

```bash
cd "/path/to/claude-codex-auto-handoff"
PLUGIN_ROOT="$(pwd)"
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

To restore, run this from the same plugin folder.

Windows PowerShell:

```powershell
$PLUGIN_ROOT = (Get-Location).Path
node "$PLUGIN_ROOT\core\cli.mjs" setup:claude-statusline --restore
```

macOS/Linux:

```bash
PLUGIN_ROOT="$(pwd)"
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --restore
```

Codex needs no extra sensor setup.

## How it works

1. Claude Code or Codex checks usage.
2. Near the default 80% threshold, the plugin prepares a capsule.
3. In `ask` mode, it asks you first.
4. In `auto` mode, it creates the capsule automatically.
5. When you open the other tool, it reads the capsule and continues.

In Claude Code, a plugin monitor can watch usage automatically. Do not run `scripts/usage-monitor.mjs` yourself.

The monitor requires Claude Code v2.1.105 or newer, an interactive CLI session, and a user/personal-scope plugin install. If monitors are unavailable, the Stop hook still works as a fallback.

## Basic commands

| Command | What it does |
|---|---|
| `/handoff` | Resume a waiting capsule |
| `/handoff status` | Show current status |
| `/handoff preview` | Preview the capsule |
| `/handoff checkpoint` | Save the current state manually |
| `/handoff history` | Show this project's handoff history |
| `/handoff recent` | Show recent capsules across all projects |
| `/handoff doctor` | Diagnose setup or capsule problems |
| `/handoff config` | Show settings |

In Claude Code, commands may appear as `/ai-handoff:handoff-...`. This README uses `/handoff` for readability.

## Settings

Put your config file here:

- Windows: `%LOCALAPPDATA%\ai-handoff\config.json`
- macOS: `~/Library/Application Support/ai-handoff/config.json`
- Linux: `~/.local/state/ai-handoff/config.json`

Common example:

```json
{
  "triggers": {
    "five_hour": {
      "threshold_percent": 75,
      "mode": "auto"
    }
  },
  "notification": {
    "method": "off"
  }
}
```

Important settings:

| Key | Default | Meaning |
|---|---:|---|
| `triggers.five_hour.threshold_percent` | `80` | Usage percent that prepares a handoff |
| `triggers.five_hour.mode` | `ask` | One of `ask`, `auto`, `off` |
| `approval.ttl_ms` | `900000` | How long an answer is valid, default 15 minutes |
| `sensors.claude.freshness_ms` | `10000` | Claude usage sample freshness, default 10 seconds |
| `realtime.enabled` | `true` | Enable the Claude Code monitor |
| `realtime.poll_interval_ms` | `1000` | Monitor polling interval, default 1 second |

Start a new session after changing settings.

## Notes

- Capsules and memory stay on your computer.
- Secrets such as API keys and tokens are redacted before saving.
- A capsule is reference material. Real files, Git state, and test results matter more.
- The monitor does not interrupt an active answer. It may react after the current answer finishes.

## Developer tests

```bash
npm test
npm run validate:package
```

## License

[MIT](LICENSE)
