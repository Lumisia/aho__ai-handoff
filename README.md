**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [中文](README.zh.md)

# claude-codex-auto-handoff

> Automatically carry your unfinished work between **Claude Code** and **Codex** when one of them runs low on its 5-hour usage limit — so you never have to re-explain where you were.

> The plugin's internal name (in its manifests and commands) is **`ai-handoff`**.

---

## The problem this solves

Claude Code and Codex each have a rolling **5-hour usage limit**. When you are deep in a task and one of them runs out, you normally switch to the other tool and start over: re-describing the goal, the decisions you already made, which files you touched, and what was left to do.

That re-explaining is slow, error-prone, and easy to get wrong.

## What this plugin does

Think of it like a **relay race**. When the first runner is about to tire, they pass the baton to the next runner — who keeps running from the exact same spot.

1. **It watches your usage.** A small sensor reads how much of your 5-hour window you have used.
2. **When you get close to the limit** (default: **80%**), it writes down exactly where you are — your goal, what you finished, what's left to do, and your current Git branch — into a small file called a **capsule**.
3. **When you open the other tool**, it reads that capsule and shows the new agent precisely where to pick up.
4. **It also remembers verified facts** about your project, and brings the relevant ones back in later sessions.

Everything happens **on your own computer**. There is no cloud server, no background daemon, and no database to set up.

## Words you will see, in plain language

| Word | What it really means |
|---|---|
| **Capsule** | A short, saved snapshot of your current task (goal, completed work, open issues, next actions, changed files, branch). Used **once**, then marked as consumed. |
| **Handoff** | Passing that snapshot from one agent (Claude Code or Codex) to the other. |
| **Verified memory** | A durable fact about your project that is backed by evidence (a passing test, a command result, a source file) — never a guess. |
| **Hook** | A small script the agent runs automatically at certain moments (when it starts, when it stops, when you send a prompt). |
| **Marketplace** | A catalog an agent reads to find and install plugins. This repo is its own one-plugin marketplace. |

---

## Requirements

- **Node.js 18 or newer** (the whole tool is plain Node with **zero npm dependencies**).
- **Claude Code and/or Codex** installed. The plugin works one-directionally with just one, but it shines when you have both.
- Willingness to **review and trust the hooks** once, the first time you install (see [`hooks/hooks.json`](hooks/hooks.json)).

Check your Node version:

```bash
node --version
```

---

## Install

There are two ways to add the plugin. **Method A** (install from this GitHub repo) is recommended for normal use. **Method B** (load a local folder) is best if you want to read or modify the code first.

### Method A — Install as a plugin (recommended)

This repository is its own **marketplace** named `claude-codex-auto-handoff`, and the plugin inside it is named `ai-handoff`. Adding the marketplace, then installing the plugin, is a two-step process on each agent.

#### Claude Code

Run these inside Claude Code (the `/plugin ...` form), or in your terminal (the `claude plugin ...` form):

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

Then run `/reload-plugins` (or restart Claude Code) to activate it.

#### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### Method B — Local / development

Clone the repo and load the folder directly. Replace `PATH/TO/claude-codex-auto-handoff` with where you cloned it.

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

Claude Code can load the folder without installing:

```bash
claude --plugin-dir PATH/TO/claude-codex-auto-handoff
```

For Codex, add the local clone as a marketplace, then install:

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### One extra step for Claude Code's sensor (both methods)

Claude reads usage from its **status line**, and a plugin cannot claim that slot by itself — so run this once. It safely keeps any status line you already had.

> ⚠️ **Replace `PATH/TO/claude-codex-auto-handoff` with the real, absolute path** — do not paste it literally (that is what causes `Cannot find module ...\PATH\TO\...`). Example on Windows: `C:\Users\you\claude-codex-auto-handoff`. The simplest stable path is a local clone of the repo (Method B), even if you installed the plugin from the marketplace — a clone path will not change when the plugin updates.

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --plugin-root "PATH/TO/claude-codex-auto-handoff"
```

To undo it later:

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --restore
```

(Codex reads usage from its official App Server, so it needs **no** extra sensor setup.)

> **Status-line note:** the `AH <pct>% · ⏳<n>` segment that appears in the status line is **Claude Code only**. Codex CLI has a built-in status line that shows rate-limit and token information natively; it does not support external command-backed segments, so the AH segment is not injected there.

> **i18n note:** all human-facing output (notifications, prompts, doctor / history reports, and the status-line segment) can be localized via the `locale` config key (`en` / `ko` / `ja` / `zh`). Skill descriptions remain in English.

### After installing (both methods)

Start a **new** agent session, and **review and trust** the lifecycle hooks when prompted. Do not use any "skip hook trust" flag for normal use — the whole point is that you decide to trust them.

---

## How it works (three automatic moments)

The plugin only acts at safe moments — it never interrupts a running tool.

- **When the agent stops** (`Stop`): it checks your usage. Then, depending on your chosen mode:
  - `auto` → it writes the capsule for you, no questions asked.
  - `ask` → it asks once: *"Create a capsule? `/handoff create` | `/handoff skip`"*.
  - `off` → it does nothing.
- **When an agent starts** (`SessionStart`): if a capsule is waiting, it verifies it (schema, file hashes, project match, expiry) and shows the new agent your task plus a thin project index.
- **When you send your first prompt** (`UserPromptSubmit`): it brings back only the **verified** project memory that is relevant, within a small token budget.

A typical relay looks like this:

```
Claude Code (80% used)  →  writes capsule  →  you open Codex  →  Codex resumes your task
        ↑                                                                  │
        └───────────────────────  and back again, any time  ──────────────┘
```

---

## Features

Each feature explained, from the sensor that triggers a handoff to the safety net around it.

### 1. Five-hour usage sensors

The plugin never guesses your usage; it reads it from each tool's real interface.

- **Claude Code** → the **status line** bridge records the used percentage and reset time. If the data is missing or stale, the plugin stays silent rather than acting on a guess.
- **Codex** → the official **App Server** (`account/rateLimits/read`) is the primary source, with the session **JSONL** rate-limit field as a fallback.

### 2. Automatic capsule handoff

When you cross the threshold, the plugin builds a **capsule**: your goal, completed work, open issues, next actions, plus the real Git branch/commit and the files changed so far. It is written with an atomic publish (temp file → flush → rename) so a half-written capsule can never be read. A capsule is **immutable** and **integrity-checked** (a content hash covers its bytes to detect corruption or edits); the receiving agent claims it with a short lease, verifies it, injects it, and only then marks it **consumed**. Each capsule is used once.

### 3. Three trigger modes

You choose how eager the plugin is, globally or per project: `auto` (hand off silently), `ask` (ask once per usage window), or `off`. The default threshold is **80%**, so the capsule is written while there is still headroom — the semantic write itself costs a little usage.

In `ask` mode the question is put to **you**, not decided for you: the agent surfaces a one-question picker — **Yes** (create), **No** (skip), and a free-text **Other** for a custom request. In Claude Code this is the native `AskUserQuestion` picker. In Codex it is the native `request_user_input` picker, which works out of the box in **Plan mode**; to get it in **Default mode** you can opt in to an experimental Codex feature:

```bash
codex features enable default_mode_request_user_input   # then restart Codex, open a new thread
codex features list                                     # expect: default_mode_request_user_input  under development  true
```

This is **optional**. The plugin never edits your Codex config or enables the flag for you, and if the picker is unavailable it falls back to a plain text question (`Yes / No / Other`) — `ask` mode works either way. The exact flag name is experimental and may change between Codex versions.

### 4. Verified memory recall

Separate from the one-use capsule, the plugin keeps a **long-lived memory** of facts about your project — but only facts backed by evidence (a passing test, a command result, a source file). On your first prompt of a session, it recalls only the relevant, evidence-backed memory, within a token budget (default 800). It never stores guesses, hidden reasoning, or whole transcripts.

### 5. Progressive project knowledge

Alongside the capsule, the plugin can carry project guidelines, formats, and gotchas. A thin **INDEX** plus a **manifest** (file hashes + dirty flags) lets the receiving agent read only what actually changed since last time, instead of re-reading everything — saving tokens. **Note:** this store is not auto-populated yet — there is no built-in command to register knowledge files, so by default the INDEX is empty. Explicit registration is planned.

### 6. Skills and commands

Three skills package the behavior: `handoff-ratelimit` (the 5-hour trigger), `handoff` (the `/handoff` command family), and `handoff-doctor` (diagnostics). They drive the `/handoff` commands listed below.

### 7. Built-in safety

Secrets are redacted before storage, capsules are integrity-checked (so corruption or edits are detected), and a capsule is always treated as *reference* material — your current instructions, the repo's policy, the real files, Git, and tests all outrank it. See [Privacy & safety](#privacy--safety).

### 8. Zero-dependency, cross-platform core

The entire core is plain Node (baseline 18) with **no npm dependencies**, so there is nothing to compile and nothing to break on upgrade. It is tested on Node 18/20/22 across Windows, macOS, and Linux.

---

## Commands

> ⚠️ **In Claude Code, plugin commands are namespaced by the plugin name.** Each action below is its own entry in the slash menu as **`/ai-handoff:handoff-<action>`** — e.g. `/ai-handoff:handoff-status`, `/ai-handoff:handoff-config set notification.method off`. The bare **`/ai-handoff:handoff`** resumes a pending capsule (it also accepts `/ai-handoff:handoff <action>`). A bare `/handoff` returns *"Unknown command"*. The table below uses the short `/handoff <action>` form for readability. In **Codex**, these actions come from the bundled skills and are model-invoked — just ask in plain language (e.g. *"show my ai-handoff status"*).

| Command | What it does |
|---|---|
| `/handoff` | Resume a waiting capsule (the most common action). |
| `/handoff status` | Show the current handoff state. |
| `/handoff preview` | Look at the capsule before injecting it. |
| `/handoff checkpoint` | Manually save a capsule right now. |
| `/handoff create` | In `ask` mode, approve creating the capsule. |
| `/handoff skip` | In `ask` mode, skip it for this usage window. |
| `/handoff doctor` | Diagnose capsule / hook / version problems. Also reports the fingerprint basis (git remote / git root / path), the data-store location, and capsules pending under a different directory or fingerprint — explaining why a handoff is not appearing. |
| `/handoff history` | Show a per-project audit log of handoff lifecycle events (created / resumed / skipped / created_from_approval). Accepts `--limit N` (default 20) and `--cwd`. |
| `/handoff config` | Show or change settings (threshold, mode, notification, memory). |

Memory is **explicit**: you save a fact only when you choose to, and only with real evidence (a passing test, a command result, a source file). It never stores hidden reasoning or full transcripts.

---

## Settings

These are the **defaults**, shipped inside the plugin at [`config/defaults.json`](config/defaults.json):

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

> ⚠️ **Do not edit `config/defaults.json`.** It lives inside the installed plugin and is overwritten on every update. Change settings in your own *user config* file instead (below).

### Where your settings go

Create (or edit) **one** file at the path for your OS:

- **Windows:** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS:** `~/Library/Application Support/ai-handoff/config.json`
- **Linux:** `~/.local/state/ai-handoff/config.json` (or `$XDG_STATE_HOME/ai-handoff/config.json`)

This file is **deep-merged on top of the defaults**, so include only the keys you want to change — never the whole file.

### How to change a setting

You have three ways, easiest first:

1. **The `/handoff config` command** (recommended):
   - `/handoff config` — show the current settings, the user-config path, and the valid keys.
   - `/handoff config set notification.method off` — change one setting (the value is validated).
   - `/handoff config unset notification.method` — revert one setting to its default.
2. **Ask Claude Code or Codex in plain words** — e.g. *"turn ai-handoff notifications off"* — and the agent runs the command for you.
3. **Edit the JSON file yourself** — open it (create it if it does not exist) and add the keys.

Either way, start a **new** agent session (or run `/reload-plugins` in Claude Code) so the change takes effect.

### Example

A user config that hands off automatically at 75% and turns notifications off — everything else stays at the defaults:

```json
{
  "triggers": { "five_hour": { "threshold_percent": 75, "mode": "auto" } },
  "notification": { "method": "off" }
}
```

### Every setting

| Key | Values | Meaning |
|---|---|---|
| `triggers.five_hour.enabled` | `true` / `false` | Master switch for the 5-hour trigger. |
| `triggers.five_hour.threshold_percent` | number, e.g. `80` | Usage % that triggers a handoff. |
| `triggers.five_hour.mode` | `auto` / `ask` / `off` | Hand off silently / ask once / do nothing. |
| `triggers.five_hour.burn_rate.enabled` | `true` / `false` (default `false`) | Opt-in: also fire earlier based on usage velocity. When enabled, a handoff is triggered if the projected time-to-100% is within `runway_minutes`, in addition to the static threshold. |
| `triggers.five_hour.burn_rate.runway_minutes` | number 5–120 (default `30`) | Fire when projected exhaustion is within this many minutes. Only used when `burn_rate.enabled` is `true`. |
| `capsule.completed_autocreate` | `true` / `false` | Also make a capsule when a task is finished. |
| `locale` | `en` / `ko` / `ja` / `zh` (default `en`) | Localizes all human-facing output: notifications, prompts, doctor and history reports, and the status-line segment. Skill descriptions stay in English regardless. |
| `notification.method` | `os` / `terminal` / `off` | OS pop-up / print to the terminal / **send nothing**. |
| `notification.fallback` | `terminal` / `off` | Used only when `method` is `os` and the OS pop-up fails. |
| `memory.auto_recall` | `true` / `false` | Recall verified memory on your first prompt. |
| `memory.auto_recall_token_budget` | number, e.g. `800` | Max tokens of memory to recall. |
| `statusline.show_handoff` | `true` / `false` (default `true`) | Show the `AH <pct>% · ⏳<n>` segment in the Claude Code status line. Note: this segment is **Claude Code only** — Codex CLI has a built-in rate-limit display but does not support external status-line segments. |

> Turning `notification.method` to `off` only silences the OS pop-up — the handoff still happens, and in `ask` mode the agent still shows the prompt in chat.

### Per project

To override any of the above for a single project only, add a `project_overrides` block keyed by that project's fingerprint:

```json
{
  "project_overrides": {
    "<project-fingerprint>": {
      "triggers": { "five_hour": { "mode": "auto" } }
    }
  }
}
```

---

## Privacy & safety

- **Local only.** Capsules and memory never leave your machine. No cloud, no telemetry.
- **Secrets are scrubbed.** Before anything is saved, common secret patterns (API keys, tokens, bearer headers, private keys) are replaced with `[REDACTED]`.
- **Capsules are integrity-checked.** Once published, a capsule is immutable and verified with a content hash, so corruption or post-write edits are detected and a failing capsule is rejected; only its delivery *state* changes. (This catches accidental damage and edits — it is not a cryptographic signature, so it does not defend against a determined attacker with write access to your local store, who could recompute the hash.)
- **Your instructions always win.** A capsule is reference material. Your current instructions, the repository's own policy, the real files, Git, and test results all take precedence over it.

---

## Run the tests

```bash
npm test                 # unit + integration tests
npm run validate:package # checks the plugin + marketplace manifests
```

Tests are plain `node --test` with no dependencies. The CI matrix runs them on **Node 18 / 20 / 22** across **Windows, macOS, and Linux**.

To also run the live end-to-end test against a real local Codex App Server:

```bash
AH_E2E=1 npm test
```

---

## License

[MIT](LICENSE).
