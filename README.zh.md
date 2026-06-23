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

## Claude Code 额外设置

Claude Code 的用量从 status line 读取，所以需要运行一次下面的设置。

你需要一个包含 `core/cli.mjs` 的本地文件夹。最简单的方法是 clone 这个仓库:

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

然后进入插件文件夹并运行设置命令。

Windows PowerShell:

```powershell
cd "C:\path\to\claude-codex-auto-handoff"
$PLUGIN_ROOT = (Get-Location).Path
node "$PLUGIN_ROOT\core\cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

macOS/Linux:

```bash
cd "/path/to/claude-codex-auto-handoff"
PLUGIN_ROOT="$(pwd)"
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

还原时，也在同一个插件文件夹里运行。

Windows PowerShell:

```powershell
$PLUGIN_ROOT = (Get-Location).Path
node "$PLUGIN_ROOT\core\cli.mjs" setup:claude-statusline --restore
```

macOS/Linux:

```bash
PLUGIN_ROOT="$(pwd)"
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --restore
```

Codex 不需要额外的传感器设置。

## 工作方式

1. Claude Code 或 Codex 检查使用量。
2. 接近默认 80% 阈值时，插件准备 capsule。
3. 在 `ask` mode 下，会先询问用户。
4. 在 `auto` mode 下，会自动创建 capsule。
5. 打开另一个工具时，它会读取 capsule 并继续工作。

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
