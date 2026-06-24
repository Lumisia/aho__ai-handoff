**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [中文](README.zh.md)

# Advanced Help

Use this page when ai-handoff does not behave as expected.

## Contents

1. [First Checks](#first-checks)
2. [The Capsule Is Not Visible](#the-capsule-is-not-visible)
3. [Claude Code and Codex Do Not Connect](#claude-code-and-codex-do-not-connect)
4. [Storage Location and AI_HANDOFF_ROOT](#storage-location-and-ai_handoff_root)
5. [Advanced Setting Keys](#advanced-setting-keys)
6. [Clear Command Arguments](#handoff-clear-arguments)
7. [Codex Capsule Behavior](#codex-capsule-behavior)

## First Checks

- Make sure Claude Code and Codex are running in the same project folder.
- Run `/handoff status` to check whether this project has a waiting capsule.
- Run `/handoff recent` to check whether the capsule was saved under another project.
- Run `/handoff doctor` to diagnose storage, project identity, and capsule state.
- After changing settings, start a new session or run `/reload-plugins` in Claude Code.

The Claude Code monitor requires Claude Code v2.1.105 or newer, an interactive CLI session, and a user/personal-scope plugin install. If monitors are unavailable, the Stop hook still works after the current answer finishes.

## The Capsule Is Not Visible

Run `/handoff doctor` first. Most cases are one of these:

- You ran the tool from a different folder, so the project identity changed.
- The capsule was already resumed once and is now consumed.
- Claude Code and Codex are looking at different storage locations.
- In `ask` mode, capsule creation has not been approved yet.
- By default, a new session does not automatically fetch a capsule; run `/handoff`. Set `handoff.session_start_auto_fetch` to `true` only if you want SessionStart auto-fetch.

Suggested order:

```text
/handoff status
/handoff recent
/handoff history
/handoff doctor
```

If a capsule appears in `recent` but not in `status`, it is probably stored under a different project folder.

## Claude Code and Codex Do Not Connect

- Install the plugin in both tools.
- The plugin's internal name is `ai-handoff`.
- Claude Code reads usage from the status line; the plugin installs its statusline runner automatically on the first Claude Code session after install or reload.
- If the status line still does not update, run `/handoff doctor`. Claude
  project settings can override user settings. Run `/handoff doctor
  --fix-statusline` to reinstall the user statusline runner; if doctor still
  reports `claude-statusline-shadowed`, remove or adjust the higher-precedence
  `.claude/settings.local.json` or `.claude/settings.json` statusLine entry.
- Codex does not need extra status line setup.
- On Windows, the Store/MSIX Claude app can split `%LOCALAPPDATA%`. In that case, set `AI_HANDOFF_ROOT` to the same shared path for both tools.

On Windows, checking `AI_HANDOFF_ROOT` is usually the fastest first step when the tools cannot see each other's capsules.

## Storage Location and AI_HANDOFF_ROOT

If `AI_HANDOFF_ROOT` is set, ai-handoff uses that path. Otherwise it uses the OS default.

| OS | Default storage root |
|---|---|
| Windows | `%LOCALAPPDATA%\ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux | `$XDG_STATE_HOME/ai-handoff` or `~/.local/state/ai-handoff` |

Important subpaths:

| Data | Path |
|---|---|
| Settings | `<root>/config.json` |
| Project data | `<root>/projects/<fingerprint>` |
| Capsules | `<root>/projects/<fingerprint>/handoff` |
| Memory | `<root>/projects/<fingerprint>/memory` |
| Claude usage samples | `<root>/sensors/claude` |

Example shared storage on Windows:

```powershell
[Environment]::SetEnvironmentVariable("AI_HANDOFF_ROOT", "C:\Users\<you>\ai-handoff-store", "User")
```

Example on macOS/Linux:

```bash
export AI_HANDOFF_ROOT="$HOME/ai-handoff-store"
```

Restart both Claude Code and Codex after changing the environment variable.

## Advanced Setting Keys

`/handoff config` shows the current settings. Values must match the expected type and range.

| Key | Meaning |
|---|---|
| `triggers.five_hour.burn_rate.enabled` | Prepare handoff earlier when usage is draining quickly |
| `triggers.five_hour.burn_rate.runway_minutes` | Prepare when estimated runway is below this many minutes, 5-120 |
| `capsule.completed_autocreate` | Create an automatic capsule even when the task looks complete |
| `codex.inline_final_capsule` | Use the final-answer fenced capsule flow for Codex auto mode, default `true` |
| `codex.stop_continuation_auto_summary` | Allow legacy Codex Stop `decision:block` summary continuation, default `false` |
| `codex.stop_continuation_ask` | Allow legacy Codex Stop `decision:block` ask continuation, default `false` |
| `codex.degraded_fallback_on_stop` | Save a degraded capsule if Codex first crosses the threshold at Stop, default `true` |
| `clear.auto.enabled` | Turn SessionStart auto-clear on or off for old used capsules, default `false` |
| `clear.older_than_days` | Default age cutoff for clearing used capsules, default 30 |
| `handoff.notify_newer_pending` | Notify when a newer pending capsule exists |
| `handoff.session_start_auto_fetch` | Automatically inject and consume a pending capsule from SessionStart, default `false` |
| `locale` | Message language, `en`, `ko`, `ja`, `zh` |
| `debug.stop_log` | Write Stop hook decision logs |
| `memory.auto_recall` | Automatically recall verified memory at conversation start |
| `memory.auto_recall_token_budget` | Token budget for automatic memory recall |
| `statusline.show_handoff` | Show handoff information in the Claude status line |
| `notification.fallback` | Use terminal notification when OS notification fails |

Most users only need `threshold_percent`, `mode`, and `realtime.enabled`.

<a id="handoff-clear-arguments"></a>

## Clear Command Arguments

`/handoff clear` chooses what to delete from the first argument:

```text
/handoff clear <this_project, used, consume, pending, expired> [--older-than 7d] [-c]
```

| Argument | Meaning |
|---|---|
| `this_project` | Clear this project's entire ai-handoff state folder. It does not delete the source repository. Aliases: `this-project`, `project`. |
| `used` | Clear old terminal capsules: `CONSUMED`, `EXPIRED`, `REJECTED`, `SKIPPED`, and `FAILED`. |
| `consume` | Clear consumed capsules only. Alias for `consumed`. |
| `consumed` | Clear consumed capsules only. |
| `pending` | Clear pending capsules: `AVAILABLE` and `DEGRADED_AVAILABLE`. |
| `expired` | Clear expired capsules only. |

Options:

| Option | Meaning |
|---|---|
| `--older-than 7d` | Only clear matching capsules older than the given age. Supports `ms`, `m`, `h`, and `d`; a bare number means days. |
| `-c`, `--confirm`, `--yes` | Confirm `this_project` deletion immediately. Without this, `/handoff clear this_project` returns a confirmation preview first. |

Examples:

```text
/handoff clear used
/handoff clear used --older-than 7d
/handoff clear --older-than 7d
/handoff clear pending
/handoff clear this_project
/handoff clear this_project -c
```

If `--older-than` is supplied without a scope, the scope defaults to `used`.
When no `--older-than` is supplied, used-like scopes use
`clear.older_than_days` from config. The default is 30 days.

Automatic cleanup is separate from the manual command. Set
`clear.auto.enabled` to `true` to run old-`used` cleanup on SessionStart. It is
off by default (`false`) and does not run continuously in the background; it
runs when a SessionStart hook fires. The automatic age cutoff uses
`clear.older_than_days`, default 30 days.

## Codex Capsule Behavior

Codex Stop hooks should not use `decision: "block"` by default for capsule
creation. In Codex, that field creates a visible continuation prompt from
`reason`, so a hidden-looking capsule instruction can show up as hook feedback
and normal assistant output.

The default flow is:

```text
UserPromptSubmit or PostToolUse
  -> detect threshold
  -> inject developer context asking for a final ai-handoff-capsule footer
Assistant final answer
  -> answer the user normally
  -> append fenced ai-handoff-capsule JSON
Stop
  -> parse and publish the capsule
  -> return {"continue": true}
```

If Codex first crosses the threshold only when Stop runs, the current answer is
already complete and cannot naturally receive a footer. The default is to save a
`DEGRADED_AVAILABLE` capsule and continue silently. Set
`codex.stop_continuation_auto_summary` or `codex.stop_continuation_ask` to
`true` only if you explicitly want the older visible continuation behavior.
