[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **中文**

# claude-codex-auto-handoff

> 当 **Claude Code** 与 **Codex** 中的一方接近其 5 小时使用上限时，自动把未完成的工作交接给另一方 —— 你再也不必重新说明自己做到了哪一步。

> 插件的内部名称（用于清单与命令中）是 **`ai-handoff`**。

---

## 它解决什么问题

Claude Code 和 Codex 各自都有一个滚动的 **5 小时使用上限**。当你正深入某项任务、其中一方用尽额度时，通常只能切换到另一个工具从头再来：重新描述目标、你已经做过的决定、动过哪些文件、还剩下什么没做。

这种“重新说明”既慢，又容易出错，还很容易说错。

## 这个插件做什么

把它想象成一场 **接力赛**。前一名跑者快要力竭时，把接力棒交给下一名跑者 —— 后者便从完全相同的位置继续往前跑。

1. **它会盯着你的用量。** 一个小型传感器读取你已经用掉了多少 5 小时窗口。
2. **当接近上限时**（默认 **80%**），它会把你当前的进度 —— 目标、关键决定、下一步、当前 Git 分支 —— 写进一个叫 **capsule（胶囊）** 的小文件里。
3. **当你打开另一个工具时**，它会读取那个胶囊，准确地告诉新代理该从哪里接着干。
4. **它还会记住关于项目的、已核实的事实**，并在之后的会话中只把相关的那些重新带回来。

一切都发生在 **你自己的电脑上**。没有云服务器，没有常驻守护进程，也没有需要另行安装的数据库。

## 常见术语，用大白话说

| 术语 | 真正的含义 |
|---|---|
| **Capsule（胶囊）** | 当前任务的一份简短快照（目标、决定、下一步、分支）。**只用一次**，用完即标记为已消费。 |
| **Handoff（交接）** | 把这份快照从一个代理（Claude Code 或 Codex）传给另一个。 |
| **Verified memory（已核实记忆）** | 由证据（通过的测试、命令运行结果、源文件）支撑的、关于项目的持久事实 —— 绝不保存猜测。 |
| **Hook（钩子）** | 代理在特定时刻（启动时、停止时、你发送提示时）自动运行的小脚本。 |

---

## 前置要求

- **Node.js 18 或更高版本**（整个工具是纯 Node 编写，**零 npm 依赖**）。
- 已安装 **Claude Code 或 Codex**（只装其一也能单向工作，两者齐备时效果最佳）。
- 首次安装时愿意 **检查并信任这些钩子**（见 [`hooks/hooks.json`](hooks/hooks.json)）。

检查你的 Node 版本：

```bash
node --version
```

---

## 安装

先获取代码：

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

下文中请把 `PATH/TO/claude-codex-auto-handoff` 替换为你克隆代码的位置。

### Claude Code

1. 从该文件夹加载插件：

   ```bash
   claude --plugin-dir PATH/TO/claude-codex-auto-handoff
   ```

2. Claude 的用量传感器需要 **一次额外设置**。（Claude 从它的 *状态栏（status line）* 读取用量，而插件无法独占那个位置，所以你需要运行一次这条命令。如果你原本就有状态栏，它会被安全保留。）

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --plugin-root PATH/TO/claude-codex-auto-handoff
   ```

   日后撤销：

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --restore
   ```

### Codex

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@<marketplace-name>
```

（Codex 从官方 App Server 读取用量，因此 **无需** 额外的传感器设置。）

### 安装之后（通用）

启动一个 **新的** 代理会话，并在提示时 **检查并信任** 这些生命周期钩子。日常使用中请勿使用任何“跳过钩子信任”的开关 —— 由你自己决定是否信任，正是本工具的关键所在。

---

## 工作原理（自动发生的三个时刻）

插件只在安全时刻动作，绝不打断正在运行的工具。

- **当代理停止时**（`Stop`）：检查用量。随后按你选择的模式：
  - `auto` → 不询问，直接为你写好胶囊。
  - `ask` → 只问一次：*“要创建胶囊吗？`/handoff create` | `/handoff skip`”*。
  - `off` → 什么都不做。
- **当代理启动时**（`SessionStart`）：若有等待中的胶囊，先校验（结构、文件哈希、项目匹配、是否过期），再向新代理展示你的任务以及一份精简的项目索引。
- **当你发送第一条提示时**（`UserPromptSubmit`）：在很小的 token 预算内，只带回相关的 **已核实** 项目记忆。

一次典型的接力是这样的：

```
Claude Code (已用 80%)  →  写入胶囊  →  打开 Codex  →  Codex 接手任务
        ↑                                                      │
        └──────────────────  随时也可反向交接  ───────────────┘
```

---

## 命令

在 Claude Code 或 Codex 内输入。两边完全一致。

| 命令 | 作用 |
|---|---|
| `/handoff` | 恢复一个等待中的胶囊（最常用的操作）。 |
| `/handoff status` | 查看当前交接状态。 |
| `/handoff preview` | 在注入之前先查看胶囊。 |
| `/handoff checkpoint` | 立刻手动保存一个胶囊。 |
| `/handoff create` | 在 `ask` 模式下批准创建胶囊。 |
| `/handoff skip` | 在 `ask` 模式下跳过本次使用窗口。 |
| `/handoff recover` | 诊断胶囊 / 钩子 / 版本问题。 |

记忆是 **显式的**：只有你主动选择、并且有真实证据（通过的测试、命令结果、源文件）时，才会保存事实。它绝不保存隐藏的推理或完整对话记录。

---

## 设置

你的设置集中在 OS 数据目录下的单个文件里：

- **Windows：** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS：** `~/Library/Application Support/ai-handoff/config.json`
- **Linux：** `~/.config/ai-handoff/config.json`

默认值（见 [`config/defaults.json`](config/defaults.json)）：

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

常见改动：

- **自动交接：** 设为 `"mode": "auto"`。
- **更早 / 更晚触发：** 修改 `"threshold_percent"`（例如 `70` 或 `90`）。
- **关闭：** 设为 `"mode": "off"`。

你还可以按项目分别覆盖设置。

---

## 隐私与安全

- **仅限本地。** 胶囊与记忆绝不离开你的机器。没有云端，也没有遥测。
- **机密会被抹去。** 在任何东西被保存之前，常见的机密模式（API 密钥、令牌、bearer 头、私钥）都会被替换为 `[REDACTED]`。
- **胶囊不可篡改。** 一旦发布，胶囊即为不可变，并用哈希校验完整性；只有它的投递 *状态* 会变化，校验失败的胶囊会被拒绝。
- **永远以你的指示为准。** 胶囊只是参考材料。当前的用户指示、仓库自身的策略、真实文件、Git 以及测试结果，全都优先于胶囊。

---

## 运行测试

```bash
npm test                 # 单元 + 集成测试
npm run validate:package # 检查插件清单
```

测试是零依赖的纯 `node --test`。CI 矩阵会在 **Windows、macOS、Linux** 上以 **Node 18 / 20 / 22** 运行它们。

若还想针对真实的本地 Codex App Server 运行实时端到端测试：

```bash
AH_E2E=1 npm test
```

---

## 许可证

[MIT](LICENSE).
