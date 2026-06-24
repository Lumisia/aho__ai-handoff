[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **中文**

# 高级帮助

当 ai-handoff 没有按预期工作时，请看这份文档。

## 目录

1. [先检查这些](#先检查这些)
2. [看不到 capsule](#看不到-capsule)
3. [Claude Code 和 Codex 不能互通](#claude-code-和-codex-不能互通)
4. [存储位置和 AI_HANDOFF_ROOT](#存储位置和-ai_handoff_root)
5. [高级设置键](#高级设置键)
6. [可设置的参数值](#handoff-clear-arguments)
7. [Codex capsule 行为](#codex-capsule-行为)

## 先检查这些

- 确认 Claude Code 和 Codex 在同一个项目文件夹中运行。
- 用 `/handoff status` 查看当前项目是否有等待中的 capsule。
- 用 `/handoff recent` 查看 capsule 是否保存到了其他项目。
- 用 `/handoff doctor` 诊断存储位置、项目标识和 capsule 状态。
- 修改设置后，请开启新会话，或在 Claude Code 中运行 `/reload-plugins`。

Claude Code monitor 需要 Claude Code v2.1.105 或更高版本、interactive CLI session，以及 user/personal scope 的插件安装。如果当前环境不能使用 monitor，Stop hook 仍会在当前回答结束后作为 fallback 工作。

## 看不到 capsule

请先运行 `/handoff doctor`。大多数情况是以下原因之一。

- 你在不同文件夹中运行，所以项目标识变了。
- capsule 已经被恢复过一次，现在是 consumed 状态。
- Claude Code 和 Codex 正在查看不同的存储位置。
- 在 `ask` mode 下，还没有批准创建 capsule。

建议检查顺序:

```text
/handoff status
/handoff recent
/handoff history
/handoff doctor
```

如果 `recent` 能看到但 `status` 看不到，通常说明 capsule 保存在另一个项目文件夹下。

## Claude Code 和 Codex 不能互通

- 两个工具都要安装插件。
- 插件内部名称是 `ai-handoff`。
- Claude Code 从 status line 读取用量，所以需要执行一次额外设置命令。
- Codex 不需要额外的 status line 设置。
- Windows 的 Store/MSIX Claude 应用可能会分离 `%LOCALAPPDATA%`。这种情况下，需要让两个工具使用同一个 `AI_HANDOFF_ROOT`。

在 Windows 上，如果两个工具互相看不到 capsule，优先检查 `AI_HANDOFF_ROOT`。

## 存储位置和 AI_HANDOFF_ROOT

如果设置了 `AI_HANDOFF_ROOT`，ai-handoff 会使用这个路径。否则使用操作系统默认位置。

| OS | 默认存储根目录 |
|---|---|
| Windows | `%LOCALAPPDATA%\ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux | `$XDG_STATE_HOME/ai-handoff` 或 `~/.local/state/ai-handoff` |

主要子路径:

| 内容 | 路径 |
|---|---|
| 设置 | `<root>/config.json` |
| 项目数据 | `<root>/projects/<fingerprint>` |
| capsule | `<root>/projects/<fingerprint>/handoff` |
| memory | `<root>/projects/<fingerprint>/memory` |
| Claude 用量 sample | `<root>/sensors/claude` |

Windows 共享存储示例:

```powershell
[Environment]::SetEnvironmentVariable("AI_HANDOFF_ROOT", "C:\Users\<you>\ai-handoff-store", "User")
```

macOS/Linux 示例:

```bash
export AI_HANDOFF_ROOT="$HOME/ai-handoff-store"
```

修改环境变量后，请重启 Claude Code 和 Codex。

## 高级设置键

`/handoff config` 会显示当前设置。修改值时必须符合对应的类型和范围。

| 键 | 说明 |
|---|---|
| `triggers.five_hour.burn_rate.enabled` | 用量下降很快时，是否更早准备交接 |
| `triggers.five_hour.burn_rate.runway_minutes` | 预计剩余时间低于多少分钟时准备，5-120 |
| `capsule.completed_autocreate` | 即使任务看起来已完成，是否也自动创建 capsule |
| `codex.inline_final_capsule` | 在 Codex auto mode 中使用最终回答 fenced capsule 流程，默认 `true` |
| `codex.stop_continuation_auto_summary` | 是否允许旧的 Codex Stop `decision:block` summary continuation，默认 `false` |
| `codex.stop_continuation_ask` | 是否允许旧的 Codex Stop `decision:block` ask continuation，默认 `false` |
| `codex.degraded_fallback_on_stop` | Codex 在 Stop 时才首次超过 threshold 时是否保存 degraded capsule，默认 `true` |
| `clear.auto.enabled` | 是否在 SessionStart 时自动删除旧 used capsule，默认 `false` |
| `clear.older_than_days` | 清理 used capsule 的默认时间阈值，默认 30 天 |
| `handoff.notify_newer_pending` | 有更新的等待中 capsule 时是否通知 |
| `locale` | 消息语言，`en`, `ko`, `ja`, `zh` |
| `debug.stop_log` | 是否写入 Stop hook 判断日志 |
| `memory.auto_recall` | 会话开始时是否自动读取已验证 memory |
| `memory.auto_recall_token_budget` | 自动 memory recall 使用的 token 预算 |
| `statusline.show_handoff` | 是否在 Claude status line 显示 handoff 信息 |
| `notification.fallback` | OS 通知失败时是否使用 terminal 通知 |

大多数用户只需要改 `threshold_percent`、`mode` 和 `realtime.enabled`。

<a id="handoff-clear-arguments"></a>

## 6. 可设置的参数值

`/handoff clear` 使用第一个参数决定删除范围。

```text
/handoff clear <this_project, used, consume, pending, expired> [--older-than 7d] [-c]
```

| 参数值 | 说明 |
|---|---|
| `this_project` | 删除当前项目 fingerprint 下的整个 ai-handoff 状态文件夹。不会删除源码仓库。别名：`this-project`, `project`。 |
| `used` | 删除已经结束的 capsule。目标状态包括 `CONSUMED`, `EXPIRED`, `REJECTED`, `SKIPPED`, `FAILED`。 |
| `consume` | 只删除已消费的 capsule。它是 `consumed` 的别名。 |
| `consumed` | 只删除已消费的 capsule。 |
| `pending` | 删除等待中的 capsule。目标状态包括 `AVAILABLE`, `DEGRADED_AVAILABLE`。 |
| `expired` | 只删除已过期的 capsule。 |

| 选项 | 说明 |
|---|---|
| `--older-than 7d` | 只删除早于指定时间的 capsule。支持 `ms`, `m`, `h`, `d`；只写数字时按天计算。 |
| `-c`, `--confirm`, `--yes` | 立即确认 `this_project` 删除。不加该选项时会先返回确认 preview。 |

示例：

```text
/handoff clear used
/handoff clear used --older-than 7d
/handoff clear --older-than 7d
/handoff clear pending
/handoff clear this_project
/handoff clear this_project -c
```

如果只传 `--older-than` 而不传 scope，scope 会按 `used` 处理。省略 `--older-than` 时，used 类 scope 使用 `clear.older_than_days` 配置值，默认 30 天。

自动清理和手动命令是分开的。把 `clear.auto.enabled` 设为 `true` 后，会在 SessionStart 时清理旧的 `used` capsule。默认关闭（`false`）。它不会在后台持续运行，只会在 SessionStart hook 执行时运行。自动清理的时间阈值使用 `clear.older_than_days`，默认 30 天。

## Codex capsule 行为

Codex Stop hook 默认不会为了创建 capsule 使用 `decision: "block"`。在 Codex 中，这会把 `reason` 变成用户可见的 continuation prompt，因此看似内部的 capsule 创建指令可能显示成 hook feedback 和普通 assistant 输出。

默认流程如下。

```text
UserPromptSubmit 或 PostToolUse
  -> 检测 threshold
  -> 注入 developer context，要求最终回答附加 ai-handoff-capsule footer
Assistant final answer
  -> 正常回答用户
  -> 附加 fenced ai-handoff-capsule JSON
Stop
  -> 解析并 publish capsule
  -> 返回 {"continue": true}
```

如果 Codex 到 Stop 时才第一次检测到超过 threshold，当前回答已经结束，无法自然附加 footer。默认行为是保存 `DEGRADED_AVAILABLE` capsule 并静默继续。只有明确想使用旧的 visible continuation 行为时，才把 `codex.stop_continuation_auto_summary` 或 `codex.stop_continuation_ask` 设为 `true`。
