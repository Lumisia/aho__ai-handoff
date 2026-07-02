<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

# AI Handoff

[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **中文**

AI Handoff 是一个本地优先的 Claude Code 与 Codex 工作交接工具。

当一个 agent 接近使用量限制时，AI Handoff 会把当前目标、分支、修改文件、备注和剩余工作保存为本地 capsule。另一个 agent 可以读取这个 capsule，并从相同上下文继续工作。

所有行为都以本地文件优先。capsule 和 hook 消息会留在你的电脑上。

## 目录

- [要求](#要求)
- [Quick Start](#quick-start)
- [主要命令](#主要命令)
- [本地文件](#本地文件)
- [Usage 数字](#usage-数字)
- [隐私与安全](#隐私与安全)
- [更多文档](#更多文档)

## 要求

你需要：

- Claude Code 和/或 Codex
- macOS、Linux、Windows 或 WSL
- 一种安装方式：Homebrew、`curl`、PowerShell、Git Bash 或 WSL

普通用户使用 release build 时不需要 Node.js 或 Rust。

## Quick Start

### Homebrew CLI

```sh
brew install Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Homebrew 桌面应用

如果你也想使用桌面 dashboard，请用这个方式。

```sh
brew install --cask Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Windows (PowerShell)

默认的 `latest` 会选择最高的 stable `vX.Y.Z` GitHub Release，而不是 GitHub 的 "Latest" 标记。

在 PowerShell 中运行。它会下载 CLI，加入用户 PATH，并运行安装程序。

**GUI + TUI** — 同时安装桌面应用和 CLI/TUI：

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

**TUI** — 仅安装 CLI/TUI：

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes
```

如需传入更多选项（只装一个 agent、固定版本），请使用同样的 scriptblock 形式：

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
```

需要可重复安装时固定 release:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Version v2.0.9
```

### Shell Installer

默认的 `latest` 会选择最高的 stable `vX.Y.Z` GitHub Release，而不是 GitHub 的 "Latest" 标记。

适用于 macOS、Linux、WSL 或 Git Bash。

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

安装后：

1. 重启 Claude Code 和 Codex。
2. 在 Codex 中打开 `/hooks`。
3. trust AI Handoff hooks。
4. 检查安装状态：

```sh
ai-handoff doctor
```

## 主要命令

| 命令 | 作用 | 何时使用 |
|---|---|---|
| `handoff` | 获取并消费当前项目中等待处理的 handoff capsule。 | 需要继续另一个 agent 留下的工作时 |
| `handoff config` | 查看或修改 AI Handoff 共享设置。 | 想修改 threshold、mode、language 或 display 设置时 |
| `handoff doctor` | 检查安装状态、hook、daemon、IPC 和 capsule 健康状态。 | hook 失败、Codex 显示 hook 错误或安装异常时 |
| `handoff checkpoint` | 把当前工作保存为 handoff capsule。 | 现在想把工作交给另一个 agent 时 |

你也可以在终端运行同样的操作：

```sh
ai-handoff handoff --agent <codex|claude-code>
ai-handoff checkpoint --message "work snapshot"
ai-handoff doctor
ai-handoff config list
```

详细命令说明：[Advanced Guide](advanced/README.zh.md)

## 本地文件

AI Handoff 会创建一个本地 home folder：

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

初学者只需要知道 3 个条目：

| 条目 | 含义 |
|---|---|
| `config.toml` | Claude Code 与 Codex 共用的设置。 |
| `store/` | 本地 capsules 和 handoff 历史。 |
| `ipc/` | hook 与 daemon 使用的本地消息队列。 |

完整项目与运行时文件结构：[Advanced Guide](advanced/README.zh.md#文件结构)

## Usage 数字

`ai-handoff usage` 读取本地 Claude Code/Codex logs。

token 和 cost 是基于本地 logs 的估算值，不是官方 bill、quota 或 provider-side usage report。

## 隐私与安全

| 主题 | AI Handoff 的行为 |
|---|---|
| 本地优先设计 | capsules、config、IPC messages 和 usage estimates 会留在你的电脑上。 |
| Hook 数据 | hooks 通过本地 IPC 发送本地事件数据，不会上传你的 workspace。 |
| 账号 credential | 账号 credential 和 OAuth token 不会被 hooks 使用，也不能写入 capsules 或 hook output。 |
| 账号操作 | 账号切换应在本地 CLI/TUI/GUI 中完成，不应由 agent skills 完成。 |

## 更多文档

- [Advanced Guide](advanced/README.zh.md)
- [English](../README.md)
- [Korean](README.ko.md)
- [Japanese](README.ja.md)

## License

[MIT](../LICENSE)
