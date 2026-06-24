<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **中文**

# claude-codex-auto-handoff

这是一个在 Claude Code 和 Codex 之间接续工作的插件。

当其中一个工具接近 5 小时使用上限时，插件会把当前工作状态保存成一个叫 **capsule** 的小文件。另一个工具读取这个 capsule 后，就能从同一位置继续。

插件内部名称是 `ai-handoff`。

如果需要帮助或想查看更详细的信息，请[点击这里](docs/advanced/README.zh.md)。

## 为什么需要它?

Claude Code 和 Codex 都有各自的 5 小时使用上限。如果工作中途其中一个到达上限，你通常需要在另一个工具里重新说明目标、改过哪些文件、下一步要做什么。

这个插件会替你准备这些交接信息。

## capsule 里有什么?

- 当前目标
- 已完成的工作
- 剩下的工作
- 已修改的文件
- 当前 Git 分支和提交
- 下一个工具应先检查的内容

capsule 使用一次后会被标记为 consumed。

## 前置要求

- Node.js 18 或更高版本
- Claude Code 或 Codex
- 两个工具都使用时，可以双向交接

检查 Node:

```bash
node --version
```

## 安装

### Claude Code

在 Claude Code 里运行:

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

或在终端运行:

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

然后运行 `/reload-plugins`，或重启 Claude Code。

### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

## Claude Code statusline 传感器

Claude Code 的用量从 Claude Code status line 输入读取。

不需要再单独运行 setup 命令。安装插件或执行 `/reload-plugins` 后，
第一个 Claude Code 会话会自动安装稳定的本地 statusline runner。

只有自动设置失败时，才需要运行:

```bash
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

要还原之前的 status line:

```bash
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --restore
```

要关闭自动设置，请在 Claude Code 运行环境中设置:

```bash
AI_HANDOFF_NO_AUTO_STATUSLINE=1
```

Codex 不需要额外的传感器设置。

## 工作方式

1. Claude Code 或 Codex 检查使用量。
2. 接近默认 80% 阈值时，插件准备 capsule。
3. 在 `ask` mode 下，会先询问用户。
4. 在 `auto` mode 下，会自动创建 capsule。
5. 打开另一个工具时，它会读取 capsule 并继续工作。

### Codex capsule 行为

Codex 默认不会从 Stop hook 再次调用模型来创建 capsule。Codex 的 Stop `decision: "block"` 和 `reason` 会变成用户可见的 continuation prompt，因此 capsule 创建指令可能出现在对话里。

ai-handoff 改为在 `UserPromptSubmit` 或 `PostToolUse` 阶段注入 developer context。`auto` mode 下，Codex 会在最终回答末尾附加 `ai-handoff-capsule` fenced JSON，Stop hook 只负责解析并保存。`ask` mode 下，会在 Stop 前指示 Codex 使用 `request_user_input` 提问。如果直到 Stop 时才第一次检测到 threshold，ai-handoff 会静默保存 `DEGRADED_AVAILABLE` capsule，不会启动新的 Codex 回合。

在 Claude Code 中，plugin monitor 可以自动监控用量。不要手动运行 `scripts/usage-monitor.mjs`。

monitor 需要 Claude Code v2.1.105 或更高版本、interactive CLI session，以及 user/personal-scope plugin install。monitor 不可用时，Stop hook 会作为 fallback 继续工作。

## 基本命令

| 命令 | 说明 |
|---|---|
| `/handoff` | 恢复等待中的 capsule |
| `/handoff status` | 显示当前状态 |
| `/handoff preview` | 预览 capsule 内容 |
| `/handoff checkpoint` | 手动保存当前状态 |
| `/handoff history` | 查看当前项目的交接历史 |
| `/handoff recent` | 查看所有项目最近的 capsule |
| `/handoff clear <this_project, used, consume, pending, expired>` | 使用不同参数指定删除范围。[请查看详细说明。](docs/advanced/README.zh.md#handoff-clear-arguments) |
| `/handoff doctor` | 诊断设置或 capsule 问题 |
| `/handoff config` | 显示设置 |

在 Claude Code 中，命令可能显示为 `/ai-handoff:handoff-...`。本 README 为了易读，统一写作 `/handoff`。

## 设置

配置文件位置:

- Windows: `%LOCALAPPDATA%\ai-handoff\config.json`
- macOS: `~/Library/Application Support/ai-handoff/config.json`
- Linux: `~/.local/state/ai-handoff/config.json`

常见示例:

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

重要设置:

| Key | Default | Meaning |
|---|---:|---|
| `triggers.five_hour.threshold_percent` | `80` | 到多少百分比时准备交接 |
| `triggers.five_hour.mode` | `ask` | `ask`, `auto`, `off` 之一 |
| `codex.inline_final_capsule` | `true` | 在 Codex auto mode 中使用最终回答 fenced capsule 流程 |
| `codex.stop_continuation_auto_summary` | `false` | 是否允许旧的 Codex Stop `decision:block` summary continuation |
| `codex.stop_continuation_ask` | `false` | 是否允许旧的 Codex Stop `decision:block` ask continuation |
| `codex.degraded_fallback_on_stop` | `true` | Codex 在 Stop 时才首次超过 threshold 时是否保存 degraded capsule |
| `clear.older_than_days` | `30` | 清理 used capsule 的默认时间阈值 |
| `clear.auto.enabled` | `false` | 是否在 SessionStart 时自动删除旧 used capsule |
| `approval.ttl_ms` | `900000` | 回答有效时间，默认 15 分钟 |
| `sensors.claude.freshness_ms` | `10000` | Claude 用量 sample 有效时间，默认 10 秒 |
| `realtime.enabled` | `true` | 是否启用 Claude Code monitor |
| `realtime.poll_interval_ms` | `1000` | monitor 检查周期，默认 1 秒 |

修改设置后，请启动新的会话。

## 注意事项

- capsule 和 memory 只保存在你的电脑上。
- API key、token 等机密会在保存前被隐藏。
- capsule 只是参考资料。真实文件、Git 状态和测试结果更重要。
- monitor 不会中断正在生成的回答，可能会在当前回答结束后才响应。

## 开发者测试

```bash
npm test
npm run validate:package
```

## 许可证

[MIT](LICENSE)
