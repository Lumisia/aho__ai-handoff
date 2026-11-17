# AI Handoff Advanced Guide

[한국어](README.md) | **English** | [日本語](README.ja.md) · [Quick start](../README.en.md)

Installation choices, the complete CLI reference, dashboard controls, and development notes. In the agent's skill picker, choose AI Handoff's `handoff-checkpoint` to save work or `handoff` to receive it.

- [Installation options](#installation-options)
- [Command reference](#command-reference)
- [TUI and desktop](#tui-and-desktop)
- [Capsules and privacy](#capsules-and-privacy)
- [Local files and project layout](#local-files-and-project-layout)
- [Development](#development)
- [Troubleshooting](#troubleshooting)

## Installation options

Use Claude Code, Codex, or both on Windows, macOS, Linux, or WSL. Install and run the chosen agent once so its configuration directory exists before installing the integration. Release CLI builds cover x86_64 and ARM64; running a release build does not require Node.js or Rust. The current release workflow provides Windows GUI installers.

### Download directly from official releases · Recommended

**[Open official releases](https://github.com/Lumisia/aho__ai-handoff/releases)** → expand **Assets** on the release and choose a file below. Choose an installer or CLI archive rather than `Source code`. `x86_64` means x64, and `aarch64` means ARM64; Apple Silicon uses ARM64.

| OS and CPU | Release file | Contents |
|---|---|---|
| Windows x64 · Recommended | `ai-handoff-gui-windows-x86_64-setup.exe` | GUI + CLI/TUI |
| Windows ARM64 · Recommended | `ai-handoff-gui-windows-aarch64-setup.exe` | GUI + CLI/TUI |
| Windows x64 · CLI/TUI only | `ai-handoff-cli-windows-x86_64.zip` | CLI + host |
| Windows ARM64 · CLI/TUI only | `ai-handoff-cli-windows-aarch64.zip` | CLI + host |
| macOS · Apple Silicon | `ai-handoff-cli-darwin-aarch64.tar.gz` | CLI + host |
| macOS · Intel | `ai-handoff-cli-darwin-x86_64.tar.gz` | CLI + host |
| Linux x64 | `ai-handoff-cli-linux-x86_64.tar.gz` | CLI + host |
| Linux ARM64 | `ai-handoff-cli-linux-aarch64.tar.gz` | CLI + host |

For WSL, choose a Linux file. Install the Windows GUI from Windows. Download the matching `.sha256` file as well, and compare it with the hash calculated using `Get-FileHash -Algorithm SHA256` on Windows, `shasum -a 256` on macOS, or `sha256sum` on Linux.

**Windows GUI:** Run the installer whose name ends in `-setup.exe` and complete the wizard. It includes CLI/TUI and the background host, and registers the agent integrations. After installation, open the GUI by searching for **aho** or **AI Handoff** in the Start menu. See the Windows section below for signing and SmartScreen guidance.

**Manual CLI archive installation:**

1. Extract the archive into a folder you will keep. Keep `ai-handoff` and `ai-handoff-host` together and retain their names. On Windows, both names end in `.exe`. The host starts the daemon when needed.
2. On Windows, open PowerShell in the extracted folder and register the integrations.

```powershell
.\ai-handoff.exe install --yes
```

3. On macOS or Linux, open a terminal in the extracted folder, ensure both files are executable, and register the integrations.

```sh
chmod +x ./ai-handoff ./ai-handoff-host
./ai-handoff install --yes
```

4. Add the extracted folder to your operating system's user **PATH**, then reopen the terminal. PATH is the list of folders where the terminal looks for commands. You can now open the TUI with `ai-handoff`; Windows installations also provide the `aho` shortcut.

If you move these files after installation, run `install --yes` from the new location again to update hook paths. Follow the shared post-install steps below to restart the agents, trust Codex hooks, and check `doctor`. The scripts below are alternatives that handle downloading and registration for you.

### Homebrew

The public source of the previously documented Homebrew tap could not be verified, so this guide uses GitHub release installation. A macOS desktop cask is also unverified.

### Alternative: Windows PowerShell installer script

CLI/TUI and the optional desktop app:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

Omit `-WithGui` for CLI/TUI only. The script downloads and checks the release files, installs the CLI and background host to `%USERPROFILE%\.ai-handoff\bin` (or `$env:AI_HANDOFF_HOME\bin`), adds that directory to the user PATH, and runs `ai-handoff install`.

| Script option | Effect |
|---|---|
| `-Yes` | Skip the CLI integration prompt; run the GUI installer silently when combined with `-WithGui`. |
| `-Only codex` / `-Only claude` / `-Only claude-code` | Select one integration for the CLI installer. Without this option, detect installed agents. |
| `-Version vX.Y.Z` | Pin a release. Default `latest` selects the highest stable `vX.Y.Z` from GitHub Releases, not the “Latest” badge. |
| `-WithGui` | Also download and run the Windows GUI installer, which includes CLI/TUI. |
| `-DryRun` | Pass `--dry-run` to the CLI integration step. Downloads, binary installation, PATH changes, and the optional GUI installation still occur. |

For example, pin the CLI/TUI and select Codex:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex -Version v2.2.8
```

The GUI installer performs its own integration setup, so `-Only` is not a restriction on that separate setup. For a configuration preview without downloading or replacing binaries, use an already installed `ai-handoff install --dry-run`.

The Windows installer is unsigned. If SmartScreen reports **Windows protected your PC / Unknown publisher**, download only from [official releases](https://github.com/Lumisia/aho__ai-handoff/releases), verify the file against its matching `.sha256`, and use **More info → Run anyway** only after verification. Keep SmartScreen enabled system-wide.

### Alternative: Shell installer script

For macOS, Linux, WSL, or Git Bash:

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

| Script option | Effect |
|---|---|
| `--yes`, `-y` | Skip the integration prompt. |
| `--only codex` / `--only claude` / `--only claude-code` | Select one agent integration. |
| `--version vX.Y.Z` | Pin a release; `latest` uses the highest stable release. |
| `--dry-run` | Preview the integration step; the script still downloads and replaces binaries. |
| `--with-gui` | Currently prints an unsupported-option notice; this script installs CLI/TUI only. |
| `--help`, `-h` | Show the script's help and exit before installation. |

Append options after `sh -s --`, for example `--yes --only codex --version v2.2.8`. The script places binaries in `${AI_HANDOFF_HOME:-$HOME/.ai-handoff}/bin`; add the printed directory to PATH if needed. This download directory is separate from the OS-specific runtime home below unless `AI_HANDOFF_HOME` is set.

After any installation, restart the agents. In Codex, open `/hooks` and trust the AI Handoff hooks. Run `ai-handoff doctor` to inspect the integration.

## Command reference

Run project-specific commands from the project directory. The project fingerprint selects the capsule bucket. `ai-handoff --help`, `ai-handoff <command> --help`, and `-h` show the installed CLI's help.

### Agent skills

Select these in the agent's skill picker; the agent runs the underlying CLI. The default plugin exposes the names below. Skill selection and terminal commands are separate entrypoints.

| Plugin skill | Purpose / CLI equivalent |
|---|---|
| `ai-handoff:handoff` | Receive the newest open capsule or one targeting the current agent. CLI: `ai-handoff handoff --agent <self>`. |
| `ai-handoff:handoff-checkpoint` | Draft and save work now; accepts optional `md` / `json` and a summary. CLI drafting contract: `ai-handoff checkpoint guidance --agent <self> --json`. |
| `ai-handoff:handoff-config` | Inspect or change settings. CLI: `ai-handoff config list/get/set`. |
| `ai-handoff:handoff-doctor` | Diagnose installation, hooks, daemon, IPC, and capsules. CLI: `ai-handoff doctor`. |

Short forms such as `/handoff checkpoint` are not universal executable chat commands across hosts. Use the skill name shown in the picker or the CLI. A short checkpoint summary does not replace available done, remaining, risks, and next-prompt details.

### All top-level commands

| Command | Purpose |
|---|---|
| `ai-handoff` | Open the terminal dashboard; Windows also provides the `aho` shortcut. Redirected output is a static snapshot. |
| `ai-handoff dashboard` | Launch `AI Handoff.exe` only when it exists beside the running CLI. |
| `ai-handoff checkpoint [md\|json\|guidance]` | Save a capsule, or print the current drafting contract. |
| `ai-handoff handoff` | Receive and mark a matching pending capsule consumed. |
| `ai-handoff retarget <capsule-id>` | Change a pending capsule's preferred consumer, or make it open. |
| `ai-handoff config <get\|set\|list>` | Read or edit shared configuration. |
| `ai-handoff doctor` | Inspect integration and runtime health. |
| `ai-handoff account <list\|status\|doctor>` | Inspect saved accounts, local status, and account setup. |
| `ai-handoff usage` | Estimate token usage and cost from local logs. |
| `ai-handoff hook <event>` | Agent hook adapter: stdin JSON → local IPC → hook response JSON. |
| `ai-handoff daemon <run\|status>` | Run the local worker or check its reachability. |
| `ai-handoff install` | Install or repair agent integrations. |
| `ai-handoff update` | Run the official installer for the latest stable CLI/TUI release. |
| `ai-handoff uninstall` | Remove managed integrations; keep local data by default. Binary removal depends on the executable's location. |
| `ai-handoff autostart <on\|off\|status>` | Manage the Windows logon entry and its config flag. |
| `ai-handoff statusline` | Read Claude statusline JSON from stdin, record a local limit sample, and render the status segment. |

### Checkpoint

| Argument / option | Meaning |
|---|---|
| `md`, `json` | Select the draft and storage format explicitly. Without either, input is detected and the configured storage format is used. |
| `guidance` | Print JSON with agent, language, format, item limits, input template, and the exact save command. Does not create a capsule. |
| `--agent <id>` | Author; overrides input `agent`, otherwise defaults to `codex`. Known aliases are normalized; other agent IDs are accepted. |
| `--target <id>` | Preferred consumer; overrides input `target`. Omit both to leave the capsule open. |
| `--message <text>` | Fallback checkpoint description/goal when the input has no goal; does not generate a work summary. |
| `--file <path>` | Read a UTF-8 JSON or Markdown draft from a file. Recommended across PowerShell and POSIX shells. Otherwise read stdin until EOF. |
| `--format <md\|json>` | Override the configured format, especially for `guidance`. Conflicting positional format and flag are an error. |
| `--json` | Compact JSON for `guidance` only; guidance without it is pretty-printed JSON. |
| `--episode <id>` | Commit the checkpoint episode supplied by a threshold prompt. Use the returned ID; ordinary manual checkpoints do not need it. |

Ask for guidance first, then write the returned template in the requested language and save it with the returned command. Example:

```sh
ai-handoff checkpoint guidance --agent claude-code --format json --json
```

A JSON draft file can contain:

```json
{
  "goal": "Finish the settings change",
  "done": ["Updated the parser"],
  "remaining": ["Check the GUI call site"],
  "risks": ["GUI checks have not run"],
  "next_prompt": "Inspect the GUI call site and run the relevant checks"
}
```

After saving that text as `checkpoint.json`, an explicit handoff target can be added:

```sh
ai-handoff checkpoint json --agent claude-code --target codex --file checkpoint.json
```

`--message` alone does not capture the conversation or reconstruct completed work. The agent skill fills the draft with its current context. Do not put credentials or raw transcripts in it.

### Handoff and retarget

| Argument / option | Meaning |
|---|---|
| `handoff --agent <id>` | Consumer ID, default `codex`. Matches open capsules and capsules targeting that ID. |
| `handoff --peek` | Preview the newest matching open/self-targeted capsule without changing consumption state. |
| `handoff --id <capsule-id>` | Consume this exact pending capsule, including one targeting a different agent. Explicit selection overrides `--force`. Applies to consumption only. |
| `handoff --force` | Also consider other targets and consume the newest eligible pending capsule; records a target override. Inspect first; prefer `--id` when several are pending. Applies to consumption only. |
| `retarget <capsule-id> --to <id>` | Change the target of a pending capsule. Omit `--to` to make it open. |

```sh
ai-handoff handoff --agent codex --peek
ai-handoff handoff --agent codex
```

To correct a target after inspecting the capsule ID:

```sh
ai-handoff retarget <capsule-id> --to codex
```

The consume response includes `hookSpecificOutput.additionalContext` and `consumed: true`. With exit code 0, `{}` means no matching pending capsule; `pending: false` with `others` lists capsules for other targets. A failed handoff can also print `{}`, so check the exit code. `--peek` returns a `preview` and ignores `--id` / `--force`; it is not a preview of a selected ID. Session start only announces a pending capsule; receiving it requires an explicit handoff. Handoff does not launch or switch the other agent; it supplies the saved context to the agent running the command.

### Configuration and diagnostics

| Command / option | Meaning |
|---|---|
| `config list` | List editable CLI keys and their effective values. |
| `config get <key>` | Read the effective value, including a built-in default when unset. |
| `config set <key> <value>` | Save a supported setting while preserving unrelated TOML keys. |
| `doctor --json` | Emit machine-readable diagnostics. |
| `doctor --fix` | Repair runtime directories and try to start the daemon, then report remaining manual steps. Can be combined with `--json`. |

```sh
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff config set triggers.five_hour.mode ask
ai-handoff config set capsule.language en
ai-handoff doctor --json
```

Five-hour trigger modes are `off`, `ask`, and `auto`; default is enabled at 80% in `ask` mode. `ask` requests a choice, `auto` requests a checkpoint without asking, and `off` disables the trigger. Both saving paths resume the interrupted task afterward. A threshold request still relies on the agent writing and saving a useful draft; it is not a conversation backup or automatic agent switch.

Other CLI settings cover `daemon.idle_timeout_seconds`, `autostart.enabled`, `statusline.show`, UI `language`, capsule format and item limits, burn-rate settings, and terminal `theme.*` colors. Use `config list` for the exact supported keys. GUI-specific settings are edited in desktop Settings. Use `autostart on/off` to apply an actual logon entry; changing the boolean alone is not the same operation.

### Accounts and usage

| Command / option | Meaning |
|---|---|
| `account list [--json]` | List saved account slots and the active marker for both agents. |
| `account status [--json]` | Show added active accounts and plan/limit information from local samples. |
| `account status --fetch [--json]` | Additionally query Claude usage with the active saved slot's credential; may refresh its OAuth token. Codex status remains local. |
| `account doctor [--json]` | Check sign-in, saved slots, vendor CLIs on PATH, and running agents. |
| `usage --group-by <day\|model\|project\|source>` | Group local usage; omitted means a total plus source summaries. |
| `usage --source <claude\|codex>` | Filter to one agent. Note `claude`, not `claude-code`, for this option. |
| `usage --since <YYYY-MM-DD>` | Include that date and later. |
| `usage --json` | Emit JSON; combines with the filters above. |

```sh
ai-handoff account list
ai-handoff account status --json
ai-handoff account doctor
ai-handoff usage --group-by model --source codex --since 2026-01-01 --json
```

Add, switch, or delete saved accounts in the local TUI/GUI Account view. To launch an agent CLI with the selected account, press `l` in the TUI's account details. These actions can change stored or live credentials. Skills do not switch accounts. `usage` token/cost estimates from local logs are distinct from provider-reported account limits and are not an official bill or quota.

### Hooks, daemon, and statusline

| Command / option | Meaning |
|---|---|
| `hook <session-start\|user-prompt\|post-tool-use\|stop>` | Process an agent event. Normally invoked by installed hooks, with JSON on stdin. |
| `hook ... --agent <codex\|claude-code>` | Select the integrated agent; default `codex`. Unlike capsule commands, this accepts only these two values. |
| `daemon run` | Run in the foreground until idle timeout; default 60 seconds without requests. |
| `daemon run --stay-alive` | Keep running until stopped, for manual diagnosis. |
| `daemon <run\|status> --home <path>` | Override `AI_HANDOFF_HOME` for this invocation; mainly for host-managed instances or an isolated runtime. |
| `daemon status` | Print `reachable` or `unreachable`; this is not a full health check. |
| `statusline` | Claude-managed stdin command; no command-specific options. Chains a recorded previous statusline when available. |

Hooks normally start the daemon on demand. An idle exit is expected; it does not mean the integration is broken. Windows uses the installed background host to start a worker outside the agent sandbox. Logon autostart is separate and disabled by default.

For manual diagnosis, run `ai-handoff daemon run --stay-alive` in one terminal and `ai-handoff doctor` in another. Stop the foreground worker with `Ctrl+C` afterward. Hook JSON uses stdout; diagnostics use stderr. Do not paste credentials into test hook input.

### Install, update, uninstall, and autostart

| Command / option | Effect |
|---|---|
| `install --dry-run` | Preview agent integration changes without applying them. |
| `install --yes` | Apply without the interactive integration prompt. |
| `install --agents <ids>` | Comma-separated agent selection, e.g. `codex,claude-code`; `claude` is accepted. Omitted means detection. |
| `install --no-plugin` | Use legacy direct hooks instead of the default plugin bundle. For compatibility/diagnosis, not a normal requirement. |
| `update` | Download and run the official installer for the latest stable CLI/TUI. Use the script's version option to pin a release. |
| `update --gui` | Also install the Windows GUI update. On other platforms only CLI/TUI is updated. |
| `uninstall` | Remove managed integrations, launch registrations, and launcher. Keep local store/logs by default. Managed binary removal depends on location. |
| `uninstall --keep-store` | Explicitly retain local data; useful for reinstalling. |
| `uninstall --purge-store` | Also delete local `store/`, `logs/`, and `ipc/`, with confirmation. |
| `uninstall --gui` | Also remove the Windows GUI; includes CLI/TUI removal. |
| `uninstall --all` | Combine GUI/CLI/TUI removal with the store/log/IPC purge. |
| `uninstall --yes` | Skip data-purge confirmation. Read the selected removal options first. |
| `autostart on` | Register the Windows user logon task (HKCU Run fallback) and enable the config flag. |
| `autostart off` | Remove the task/Run entry and disable the flag. |
| `autostart status` | Show the config flag and actual task/Run registrations. |

`--keep-store` conflicts with `--purge-store` and `--all`. Purging removes capsules and handoff history; the current purge targets store/log/IPC, not `accounts/` or `config.toml`. Uninstall without purge is already a removal action, not a dry run. CLI self-removal is attempted only when running from the runtime home's `bin/`; the shell installer's separate default bin can remain on macOS/Linux. Check the installer-printed location before manually removing those binaries. Autostart registration currently targets Windows; hooks can start the daemon on demand elsewhere.

Platform limitation: the current uninstall path calls Windows task/registry commands. Source inspection indicates it may fail before removing integrations on Unix when `schtasks` is absent; this has not been reproduced on a Unix machine here. Treat that error as a failed uninstall and inspect/report it before manual cleanup.

## TUI and desktop

Run `aho` or `ai-handoff` in a terminal to open the TUI. Windows installations provide `aho` through the `aho.cmd` shortcut; use `ai-handoff` on other platforms. `ai-handoff tui` is an alternative command. With the Windows GUI installed, search **aho** in the Start menu and open the app. The terminal command `ai-handoff dashboard` only searches beside that CLI for `AI Handoff.exe`; it may fail when the GUI is installed elsewhere even though Start-menu launch works.

### TUI keys

Keys below apply when the corresponding pane has focus. Editing fields own their keys until you finish or cancel.

| View | Keys |
|---|---|
| Global tabs | `Tab` / `Shift+Tab` or `1`–`6`: Overview, Capsule, Usage, Account, Integration, Settings. |
| Tab bar | `←` / `→`: select tab; `↓`, `Enter`, or `Space`: enter its content. |
| Back / quit | `q` / `Esc`: leave content or a subview; at the tab bar, press again after the quit prompt to exit. |
| Capsule tree | `↑/↓` or `k/j`: select; `→`, `Enter`, or `Space`: expand/open; `←`: collapse; `r`: reload. |
| Capsule detail | `↑/↓` or `k/j`: select field; `e` / `Enter`: edit; `Enter`: save edit; `Esc`: cancel edit; `s`: advance lifecycle state; `d`, then `d` or `y`: delete; `←`: return to tree. |
| Usage | `g`: cycle grouping; `→` / `Enter` / `Space`: details; `←`: chart. |
| Account | `↑/↓` or `k/j`: select; tree `a` / `+`: add; `Enter` / `→` / `Space`: detail; detail `s`: switch, `l`: launch, `a`: add, `d` then `d` / `y`: delete, `←`: tree; `r`: refresh usage. |
| Integration | `d`: doctor; `l`: logs; `r`: repair center; `Enter` / `Space`: detail; repair `↑/↓` or `k/j`: select, `Enter` / `y`: activate/confirm, `n`: cancel confirmation. |
| Settings | `↑/↓` or `k/j`: select; category `Enter` / `→` / `Space`: detail; detail `←` / `-`: previous value, `→` / `+` / `=`: next, `Enter` / `Space`: toggle/edit, `r`: reset; `/`: search. |

Capsule lifecycle editing changes stored state; it does not inject the capsule into an agent conversation. Deletion removes a stored capsule. Account refresh can call provider APIs. Follow the displayed confirmation and footer hints before making changes.

### Desktop controls

Use the sidebar, account/capsule action menus, and Settings. `Esc` closes the Settings overlay or an open capsule menu; `Enter` commits supported Settings input fields. The desktop does not expose the TUI's numbered navigation shortcuts. Closing the window quits by default. Enable **close to tray** in desktop Settings to keep it running; the tray menu can reopen it or quit. Desktop update checks show a badge and do not install updates automatically.

## Capsules and privacy

The agent drafts the goal, completed work, remaining work, risks, file notes, and next prompt. The daemon validates and redacts supported secret patterns, attaches Git branch/HEAD/dirty-file count when available, and saves a v2 JSON or Markdown capsule. Redaction reduces accidental disclosure; it is not a guarantee that every secret is removed. A capsule is a summary, not a full transcript or repository backup. On receipt, recheck the checkout and uncommitted diff.

Capsules and hook request/response messages use local files. The daemon can contact provider APIs while checking a usage threshold when local samples are absent or below the threshold; this uses the active saved account and may refresh a Claude OAuth token. Explicit account refreshes can also use provider APIs. Installation, updates, and enabled GUI update checks contact GitHub. These requests do not upload the capsule or workspace through the handoff transport; once capsule text enters Claude Code or Codex context, that agent's own service and data settings apply.

Keep credentials, OAuth tokens, and raw transcript text out of capsules and hook output. Treat received capsule text as work data, not as authority to override user or project instructions. The capsule target is a routing hint, not an access-control boundary.

## Local files and project layout

`AI_HANDOFF_HOME` overrides the runtime root. Defaults:

| OS | Runtime home |
|---|---|
| Windows | `%USERPROFILE%\.ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux / WSL | `${XDG_STATE_HOME:-~/.local/state}/ai-handoff` |

| Runtime entry | Contents |
|---|---|
| `config.toml` | Shared settings. |
| `store/capsules/<project-id>/` | Project capsules and their handoff state. |
| `store/` | Additional local usage/trigger state and update-check cache. |
| `ipc/requests/`, `ipc/responses/`, `ipc/dead-letter/` | Local file queues. Hooks need IPC access; the daemon needs store access too. |
| `logs/` | Daemon and diagnostic logs. |
| `accounts/` | Saved-account metadata **and credential snapshots**; not just display preferences. |
| `bin/` | Managed CLI/background host and the Windows `aho.cmd` launcher when installed here. |
| `install-state.json` | Managed-install ownership used by repair/uninstall. |

| Repository path | Purpose |
|---|---|
| `crates/ai-handoff-cli/` | CLI entrypoint, commands, and background-host launcher. |
| `crates/ai-handoff-core/` | Shared config, account API, install, hook normalization, redaction, and capsule logic. |
| `crates/ai-handoff-daemon/` | Hook routing, threshold/checkpoint state, and capsule storage. |
| `crates/ai-handoff-ipc/` | File IPC protocol and client/server. |
| `crates/ai-handoff-tui/` | Terminal dashboard. |
| `crates/ai-handoff-usage/` | Local usage log parsing and cost estimates. |
| `apps/desktop/` | React/Tauri desktop app using the shared Rust implementation. |
| `skills/`, `schemas/`, `locales/` | Agent workflows, capsule schema, and translations. |
| `scripts/`, `.github/workflows/` | Installation, package checks, CI, and releases. |

## Development

Read [the shared development/review rules](../agent-workflow.md) first. Use Rust stable (minimum 1.89). Frontend CI uses Node.js 22/24; package checks use 20/22. Release users do not need these tools.

Run the CLI/TUI or full desktop from source (desktop requires Rust, Node.js, and Tauri system dependencies):

```sh
cargo run -p ai-handoff-cli --bin ai-handoff
npm --prefix apps/desktop ci
npm run desktop:dev
```

Run from the repository root, choosing the checks for the area changed:

```sh
git diff --check
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
npm --prefix apps/desktop ci
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

For hook/IPC changes, also run:

```sh
cargo test -p ai-handoff-cli --test e2e_hook_daemon --test hook_offline
```

Build both CLI and background host:

```sh
cargo build --release -p ai-handoff-cli --bins
```

Linux workspace builds need Tauri GTK/WebKit dependencies; the exact CI packages are in [ci.yml](../../.github/workflows/ci.yml). Desktop native changes also need `cargo test -p ai-handoff-desktop`. Source entrypoints and flags are defined in [the CLI enum](../../crates/ai-handoff-cli/src/lib.rs).

On Windows, close the specific TUI/worker using a build output before rebuilding that executable; stop a foreground daemon with `Ctrl+C`. Confirm the process's executable path before stopping a background process. Use temporary runtime homes and fake accounts for tests, never the real account vault. Document-only edits need link/path/command checks and `git diff --check`, not an unrelated full build.

## Troubleshooting

| Symptom | Check / next step |
|---|---|
| CLI or `aho` not found | Reopen the terminal. Check the installer-printed bin path; `aho.cmd` is a Windows launcher. Use `ai-handoff` elsewhere. |
| `No matching Codex or Claude config directory found` | Install and run the selected agent once, then retry. `--agents` / `-Only` filters detected configurations; it does not create the agent setup. |
| GUI will not open with `dashboard` | Search `aho` in the Windows Start menu. `dashboard` requires `AI Handoff.exe` beside that CLI executable. |
| Codex hook error / trust prompt | Open `/hooks`, trust AI Handoff, restart the agent, then run `ai-handoff doctor`. |
| Old hook errors / duplicate hooks | Inspect doctor output for old v1 Node hooks or stale plugin copies; preview with `ai-handoff install --dry-run`, then reinstall the integration if appropriate. |
| Daemon says offline after inactivity | The default 60-second idle exit is expected. Trigger a normal hook or use `doctor --fix`; use the foreground `--stay-alive` workflow for diagnosis. |
| IPC/store permission error | Run `ai-handoff doctor` outside the agent sandbox; `doctor --fix` repairs supported runtime problems. Preserve Windows IPC ACL inheritance. |
| No pending capsule | Check the project directory, `--agent`, `--peek` output, and exit code. Capsules for another target appear under `others`; inspect the ID before retargeting or claiming it. |
| Threshold prompt or usage missing | Local samples may be absent/stale; check mode/threshold and active saved account. Provider fetch can fail. `usage` estimates do not prove the provider quota. |
| Account status empty | Add the account through the local Account view and inspect `account doctor`. Local samples and provider data have different refresh paths. |
| Windows build cannot replace an EXE | Identify and close the process running that exact build output, then rebuild. |
| Linux reports `GLIBC_x.y not found` | Releases use GNU Linux targets; check system compatibility or build from source. No official musl/Alpine artifact is provided, and an exact minimum glibc version has not been verified here. |
| Update cannot download | Check the official release artifacts/network, or pin a known release with the installation script. |
