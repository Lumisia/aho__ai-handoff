# AI Handoff 高级指南

[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **中文**

本指南说明初学者 README 中故意省略的细节。

## 目录

- [命令详情](#命令详情)
- [文件结构](#文件结构)
- [项目结构](#项目结构)
- [开发检查](#开发检查)
- [故障排查](#故障排查)

## 命令详情

| 命令 | 终端等价命令 | 说明 |
|---|---|---|
| `handoff` | `ai-handoff handoff --agent <self>` | 获取并消费当前项目和当前 agent 对应的最新待处理 capsule。 |
| `handoff config` | `ai-handoff config list` | 显示可编辑的 config keys。直接编辑时使用 `ai-handoff config get <key>` 和 `ai-handoff config set <key> <value>`。 |
| `handoff doctor` | `ai-handoff doctor` | 检查 plugin 状态、hook trust、daemon 可达性、IPC、store 和常见重复 hook 问题。 |
| `handoff checkpoint` | `ai-handoff checkpoint --message "work snapshot"` | 从当前任务创建本地 capsule。message 应简短说明下一个 agent 为什么要看这个 checkpoint。 |

常用终端命令：

```sh
ai-handoff
ai-handoff tui
ai-handoff checkpoint --message "backend auth work"
ai-handoff doctor
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff usage
ai-handoff account status
ai-handoff daemon run
ai-handoff autostart status
ai-handoff uninstall --keep-store
```

## 文件结构

AI Handoff runtime home：

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

重要 runtime entries：

| 路径 | 用途 |
|---|---|
| `config.toml` | Claude Code、Codex、daemon、TUI 和 hooks 共用的配置。 |
| `store/` | 本地 capsules、project buckets 和 handoff state。 |
| `ipc/` | hooks 与 daemon 使用的本地 file IPC queue。Codex 只需要这里的写入权限。 |
| `logs/` | 启用时保存 daemon 与诊断日志。 |
| `accounts/` | 本地 account metadata。credential 不能输出到 hooks 或 capsules。 |
| `install-state.json` | 记录 installer 写入的内容，让 uninstall 只删除受管理的文件。 |

## 项目结构

| 路径 | 用途 |
|---|---|
| `crates/ai-handoff-cli/` | 原生 CLI entrypoint 与用户命令。 |
| `crates/ai-handoff-core/` | 共享 config、install、hook event、fingerprint、redaction 和 capsule logic。 |
| `crates/ai-handoff-daemon/` | 接收 hook requests 并写入 capsules 的本地 daemon。 |
| `crates/ai-handoff-ipc/` | 基于文件的 IPC protocol 与 client/server helpers。 |
| `crates/ai-handoff-tui/` | 终端 dashboard。 |
| `crates/ai-handoff-usage/` | 本地 Claude/Codex usage log parser 与 cost estimator。 |
| `apps/desktop/` | 可选 Tauri desktop dashboard。 |
| `skills/` | plugin bundle 提供的 agent-facing skills。 |
| `schemas/` | capsule 与 memory schema 文件。 |
| `scripts/` | package validation 与 release helper scripts。 |

## 开发检查

提交前运行：

```sh
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
git diff --check
```

当 daemon 没有使用 `target/release/ai-handoff.exe` 时运行 release build：

```sh
cargo build --release -p ai-handoff-cli
```

如果 Windows build 报 access denied，先停止正在运行的本地 daemon：

```powershell
Get-Process ai-handoff | Stop-Process
cargo build --release -p ai-handoff-cli
```

## 故障排查

| 现象 | 检查内容 |
|---|---|
| Codex 显示 hook errors | 打开 `/hooks`，trust AI Handoff hooks，然后运行 `ai-handoff doctor`。 |
| hooks 以 code 1 退出 | 检查旧的 v1 Node hooks 或旧 plugin cache。用 `ai-handoff install --yes` 重新安装。 |
| daemon offline | 在一个终端运行 `ai-handoff daemon run`，再在另一个终端运行 `ai-handoff doctor`。 |
| usage 为空 | AI Handoff 只从本地 logs 估算。先使用 Claude Code 或 Codex，再运行 `ai-handoff usage`。 |
| Windows build 无法替换 exe | 停止正在运行的 `ai-handoff.exe` process，然后重新 build。 |
