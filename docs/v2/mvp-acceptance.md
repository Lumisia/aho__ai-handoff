# AI Handoff v2 MVP Acceptance

Date: 2026-06-25
Branch: v2-rust-tauri

## Automated Verification

- [x] `cargo test -p ai-handoff-core`
- [x] `cargo test -p ai-handoff-ipc`
- [x] `cargo test -p ai-handoff-daemon`
- [x] `cargo test -p ai-handoff-cli`
- [x] `cargo test -p ai-handoff-cli --test e2e_hook_daemon`
- [x] `cargo build --release`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `npm --prefix apps/desktop run build`
- [x] `npm --prefix apps/desktop run tauri build`
- [x] `npm audit --audit-level=moderate` in `apps/desktop`

## Manual Codex-on-Windows Acceptance

Live reproduction run 2026-06-26 against the real install on this machine
(real `ai-handoff.exe`, real daemon process outside any sandbox, real store
under `C:\Users\PC\.ai-handoff`). The hooks were invoked directly with the
exact payload Codex's lifecycle hooks send, which exercises the full
hook -> file-IPC -> out-of-sandbox-daemon -> store path. EPERM did not occur.

- [x] Start daemon outside Codex:
  `target/release/ai-handoff.exe daemon run`
- [x] IPC dir present in Codex `writable_roots`:
  `C:\Users\PC\.ai-handoff\ipc` (verified in `config.toml`)
- [x] Codex lifecycle hooks point to:
  `target/release/ai-handoff.exe hook <event> --agent codex` (verified in `hooks.json`)
- [x] `Stop` (agent codex) writes a capsule under
  `store/capsules/fae6b6d550ddee4e6151a916/cap_*.json` — no `EPERM`.
- [x] Peer `SessionStart` (agent claude-code) injects `additionalContext`
  with the capsule and marks it `consumed`; `ipc/requests` drained.

Residual (Codex-only, cannot be reproduced outside interactive Codex):

- [ ] Trust the new hooks via `/hooks` (interactive TUI; records a trust hash).
- [ ] Run a real interactive Codex session so the hook fires *inside* Codex's
  workspace-write sandbox. The enabling config (`writable_roots` += IPC dir) is
  already present, so this is the final "trust + one session" step for the user.

## Installer Acceptance

Automated coverage now includes the v2 installer/config-patcher path:

- [x] `ai_handoff_core::install::plan_install` computes dry-run file plans without writes.
- [x] `apply_install` aborts before any file write if a later selected agent config is malformed.
- [x] `apply_uninstall` removes only managed entries and preserves user edits made after install.
- [x] CLI `install --dry-run` writes nothing and renders the planned Codex/Claude file changes.
- [x] CLI Scheduled Task argv is covered:
  `schtasks /Create /SC ONLOGON /TN "AI Handoff" /TR "\"<exe>\" daemon run" /RL LIMITED /F`
- [x] CLI HKCU Run fallback argv is covered:
  `reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v "AI Handoff" /t REG_SZ /d "\"<exe>\" daemon run" /f`
- [x] CLI uninstall Scheduled Task argv is covered:
  `schtasks /Delete /TN "AI Handoff" /F`
- [x] CLI uninstall HKCU Run fallback argv is covered:
  `reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v "AI Handoff" /f`

Live mutating install was not run in this session. Manual Windows checklist:

- [x] Build release:
  `cargo build --release`
- [x] Run read-only installer preview:
  `target/release/ai-handoff.exe install --dry-run`
- [x] Confirm the dry-run plan mentions:
  `~/.codex/hooks.json`, `~/.codex/config.toml`, and `~/.claude/settings.json`
  when both agent config directories exist.
- [x] Confirm dry-run does not change file contents or mtimes.
- [x] Run install only after approving live config changes:
  `target/release/ai-handoff.exe install --yes`
- [x] Confirm Scheduled Task failure falls back to HKCU Run and records
  `autostart.kind = "hkcu_run"` in `install-state.json`.
- [ ] In Codex, open `/hooks` and trust the new AI Handoff hooks.
- [x] Confirm either Windows Task Scheduler or HKCU Run contains
  `AI Handoff` with `daemon run`.
- [ ] Run a Codex session and confirm no `EPERM` appears during IPC writes.
- [ ] Run uninstall cleanup:
  `target/release/ai-handoff.exe uninstall --keep-store`
- [ ] Confirm only AI Handoff managed hooks/writable root/env entries were removed;
  unrelated user config remains.
- [ ] Optional destructive cleanup after review:
  `target/release/ai-handoff.exe uninstall --purge-store`

Live Windows result:

- Scheduled Task primary + HKCU Run fallback is implemented and covered by unit
  tests.
- Live `install --yes` succeeded on this machine by falling back to HKCU Run
  after Scheduled Task creation was denied.
- `install-state.json` records `autostart.kind = "hkcu_run"` and `scheduled_task = null`.
- `~/.codex/hooks.json`, `~/.codex/config.toml`, and `~/.claude/settings.json`
  contain the managed AI Handoff entries.
- Duplicate v1 plugin warnings were cleared by removing Codex v1
  `hooks.state` entries and deleting the Claude v1
  `enabledPlugins["ai-handoff@claude-codex-auto-handoff"]` key.

## Tauri Dashboard Acceptance

Automated and local build result:

- [x] `apps/desktop` Tauri 2 + React/Vite/TypeScript scaffold exists.
- [x] Read-only Tauri commands compile:
  `get_dashboard_snapshot`, `list_capsules`, `read_capsule`, `read_logs`.
- [x] Overview, Doctor, Capsules, Settings, and Logs views build.
- [x] Frontend dependency audit reports `0 vulnerabilities`.
- [x] Tauri build produced:
  `target/release/AI Handoff.exe`.
- [x] CLI `dashboard` subcommand parses and attempts to launch
  `AI Handoff.exe` next to `ai-handoff.exe`.
- [x] Unit coverage verifies `aho.cmd` generation and launcher state recording.

Live launcher mutation (completed 2026-06-26):

- [x] Live `target/release/ai-handoff.exe install --yes` created
  `C:\Users\PC\.ai-handoff\bin\aho.cmd` (launches `AI Handoff.exe`) and added
  that directory to the HKCU user `Path`.
- [x] `aho` resolves in a fresh shell; the GUI launches and the read-only
  Overview renders (Codex hooks/config, Claude settings, IPC all `ok`).

Note: the `install --yes` run that wrote the config also pre-dated the
state-ownership fix, so the live `install-state.json` recorded
`writable_root_added`/`env_key_added`/`path_dir_added` as `null`. With the fix
(record managed entries by ownership), the user's next `install` run self-heals
these so a later `uninstall` removes them cleanly.

## Current MVP Result

The automated vertical slice passes: hook CLI writes file IPC requests, daemon
handles requests outside the hook process, Stop stores a pending capsule, and
peer SessionStart returns `additionalContext` and marks the capsule consumed.

The Tauri read-only dashboard now builds locally and runs; `aho` launcher +
HKCU PATH + HKCU Run autostart are installed. The live daemon path was
reproduced end-to-end on the real install with no `EPERM`. The only remaining
manual step is Codex-only: trust the hooks via `/hooks` and run one interactive
Codex session so the hook fires inside Codex's own sandbox.
