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
2. **When you get close to the limit** (default: **80%**), it writes down exactly where you are — your goal, key decisions, next steps, current Git branch — into a small file called a **capsule**.
3. **When you open the other tool**, it reads that capsule and shows the new agent precisely where to pick up.
4. **It also remembers verified facts** about your project, and brings the relevant ones back in later sessions.

Everything happens **on your own computer**. There is no cloud server, no background daemon, and no database to set up.

## Words you will see, in plain language

| Word | What it really means |
|---|---|
| **Capsule** | A short, saved snapshot of your current task (goal, decisions, next actions, branch). Used **once**, then marked as consumed. |
| **Handoff** | Passing that snapshot from one agent (Claude Code or Codex) to the other. |
| **Verified memory** | A durable fact about your project that is backed by evidence (a passing test, a command result, a source file) — never a guess. |
| **Hook** | A small script the agent runs automatically at certain moments (when it starts, when it stops, when you send a prompt). |

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

First, get the code:

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

In the steps below, replace `PATH/TO/claude-codex-auto-handoff` with where you cloned it.

### Claude Code

1. Load the plugin from the folder:

   ```bash
   claude --plugin-dir PATH/TO/claude-codex-auto-handoff
   ```

2. Claude needs **one extra setup step** for the usage sensor. (Claude reads usage from its *status line*, and a plugin cannot claim that slot by itself — so you run this once. It safely keeps any status line you already had.)

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --plugin-root PATH/TO/claude-codex-auto-handoff
   ```

   To undo it later:

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --restore
   ```

### Codex

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@<marketplace-name>
```

(Codex reads usage from its official App Server, so it needs **no** extra sensor setup.)

### After installing (both)

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

## Commands

Type these inside Claude Code or Codex. They are identical on both.

| Command | What it does |
|---|---|
| `/handoff` | Resume a waiting capsule (the most common action). |
| `/handoff status` | Show the current handoff state. |
| `/handoff preview` | Look at the capsule before injecting it. |
| `/handoff checkpoint` | Manually save a capsule right now. |
| `/handoff create` | In `ask` mode, approve creating the capsule. |
| `/handoff skip` | In `ask` mode, skip it for this usage window. |
| `/handoff recover` | Diagnose capsule / hook / version problems. |

Memory is **explicit**: you save a fact only when you choose to, and only with real evidence (a passing test, a command result, a source file). It never stores hidden reasoning or full transcripts.

---

## Settings

Your settings live in a single file in your OS data directory:

- **Windows:** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS:** `~/Library/Application Support/ai-handoff/config.json`
- **Linux:** `~/.config/ai-handoff/config.json`

The defaults (see [`config/defaults.json`](config/defaults.json)):

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

Common changes:

- **Hand off automatically:** set `"mode": "auto"`.
- **Trigger earlier or later:** change `"threshold_percent"` (e.g. `70` or `90`).
- **Turn it off:** set `"mode": "off"`.

You can also override settings per project.

---

## Privacy & safety

- **Local only.** Capsules and memory never leave your machine. No cloud, no telemetry.
- **Secrets are scrubbed.** Before anything is saved, common secret patterns (API keys, tokens, bearer headers, private keys) are replaced with `[REDACTED]`.
- **Capsules can't be tampered with.** Once published, a capsule is immutable and integrity-checked with a hash; only its delivery *state* changes. A capsule that fails verification is rejected.
- **Your instructions always win.** A capsule is reference material. Your current instructions, the repository's own policy, the real files, Git, and test results all take precedence over it.

---

## Run the tests

```bash
npm test                 # unit + integration tests
npm run validate:package # checks the plugin manifests
```

Tests are plain `node --test` with no dependencies. The CI matrix runs them on **Node 18 / 20 / 22** across **Windows, macOS, and Linux**.

To also run the live end-to-end test against a real local Codex App Server:

```bash
AH_E2E=1 npm test
```

---

## License

[MIT](LICENSE).
