<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

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

## Claude Code statusline sensor

Claude Code usage is read from the Claude Code status line input.

You do not need to run a separate setup command. The plugin installs a stable
local statusline runner automatically on the first Claude Code session after
the plugin is installed or reloaded.

If automatic setup fails or `/handoff doctor` reports that project Claude
settings shadow the user status line, run:

```text
/handoff doctor --fix-statusline
```

To restore your previous status line from a terminal:

```bash
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --restore
```

To opt out of automatic setup, set this in the Claude Code environment:

```bash
AI_HANDOFF_NO_AUTO_STATUSLINE=1
```

Codex needs no extra sensor setup.

## How it works

1. Claude Code or Codex checks usage.
2. Near the default 80% threshold, the plugin prepares a capsule.
3. In `ask` mode, it asks you first.
4. In `auto` mode, it creates the capsule automatically.
5. Run `/handoff` in the other tool to read the capsule and continue. If `handoff.session_start_auto_fetch` is enabled, a new session can fetch it automatically.

### Codex capsule behavior

Codex does not create capsules by calling the model again from the Stop hook by default. Codex treats Stop `decision: "block"` and `reason` as a visible continuation prompt, so capsule instructions could appear in the conversation.

Instead, ai-handoff injects developer context during `UserPromptSubmit` or `PostToolUse`. In `auto` mode, Codex appends a final `ai-handoff-capsule` fenced JSON section to the answer, and the Stop hook only parses and saves it. In `ask` mode, Codex is instructed to ask with `request_user_input` before Stop. If the threshold is first detected only at Stop time, ai-handoff quietly saves a `DEGRADED_AVAILABLE` capsule and does not start another Codex turn.

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
| `/handoff clear <this_project, used, consume, pending, expired>` | Choose the deletion scope with arguments. [See details.](docs/advanced/README.md#handoff-clear-arguments) |
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
| `handoff.session_start_auto_fetch` | `false` | Automatically inject a pending capsule on SessionStart |
| `codex.inline_final_capsule` | `true` | Use the final-answer fenced capsule flow for Codex auto mode |
| `codex.stop_continuation_auto_summary` | `false` | Allow legacy Codex Stop `decision:block` summary continuation |
| `codex.stop_continuation_ask` | `false` | Allow legacy Codex Stop `decision:block` ask continuation |
| `codex.degraded_fallback_on_stop` | `true` | Save a degraded capsule if Codex first crosses the threshold at Stop |
| `clear.older_than_days` | `30` | Default age cutoff for clearing used capsules |
| `clear.auto.enabled` | `false` | Turn SessionStart auto-clear on or off for old used capsules |
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
