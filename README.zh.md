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
2. **当接近上限时**（默认 **80%**），它会把你当前的进度 —— 目标、已完成的工作、待办事项、当前 Git 分支 —— 写进一个叫 **capsule（胶囊）** 的小文件里。
3. **当你打开另一个工具时**，它会读取那个胶囊，准确地告诉新代理该从哪里接着干。
4. **它还会记住关于项目的、已核实的事实**，并在之后的会话中只把相关的那些重新带回来。

一切都发生在 **你自己的电脑上**。没有云服务器，没有常驻守护进程，也没有需要另行安装的数据库。

## 常见术语，用大白话说

| 术语 | 真正的含义 |
|---|---|
| **Capsule（胶囊）** | 当前任务的一份简短快照（目标、已完成工作、未决问题、下一步、改动文件、分支）。**只用一次**，用完即标记为已消费。 |
| **Handoff（交接）** | 把这份快照从一个代理（Claude Code 或 Codex）传给另一个。 |
| **Verified memory（已核实记忆）** | 由证据（通过的测试、命令运行结果、源文件）支撑的、关于项目的持久事实 —— 绝不保存猜测。 |
| **Hook（钩子）** | 代理在特定时刻（启动时、停止时、你发送提示时）自动运行的小脚本。 |
| **Marketplace（市场）** | 代理用来查找并安装插件的目录。本仓库本身就是一个只含一个插件的市场。 |

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

添加插件有两种方式。日常使用推荐 **方式 A**（从本 GitHub 仓库安装）。如果你想先阅读或修改代码，**方式 B**（加载本地文件夹）更合适。

### 方式 A — 作为插件安装（推荐）

本仓库本身就是一个名为 `claude-codex-auto-handoff` 的 **市场**，其中的插件名为 `ai-handoff`。在每个代理上，先添加市场、再安装插件，分两步。

#### Claude Code

在 Claude Code 内（`/plugin ...` 形式）或在终端里（`claude plugin ...` 形式）运行：

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

然后运行 `/reload-plugins`（或重启 Claude Code）以启用。

#### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### 方式 B — 本地 / 开发

克隆仓库并直接加载该文件夹。把 `PATH/TO/claude-codex-auto-handoff` 替换为你克隆的位置。

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

Claude Code 无需安装即可加载该文件夹：

```bash
claude --plugin-dir PATH/TO/claude-codex-auto-handoff
```

Codex 把本地克隆添加为市场后再安装：

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### Claude Code 传感器的一次额外步骤（两种方式通用）

Claude 从它的 **状态栏（status line）** 读取用量，而插件无法独占那个位置，所以需要运行一次这条命令。如果你原本就有状态栏，它会被安全保留。

> ⚠️ **请把 `PATH/TO/claude-codex-auto-handoff` 替换为真实的绝对路径** —— 不要原样粘贴（那会导致 `Cannot find module ...\PATH\TO\...` 错误）。Windows 示例：`C:\Users\you\claude-codex-auto-handoff`。最稳定的路径是仓库的本地克隆（方式 B）—— 即使你是从市场安装的，克隆路径也不会随插件更新而改变。

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --plugin-root "PATH/TO/claude-codex-auto-handoff"
```

日后撤销：

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --restore
```

（Codex 从官方 App Server 读取用量，因此 **无需** 额外的传感器设置。）

> **状态栏说明：** 状态栏中显示的 `AH <pct>% · ⏳<n>` 段 **仅限 Claude Code**。Codex CLI 内置了 rate-limit 与 token 信息的显示，但不支持外部命令驱动的状态栏段，因此 AH 段不会注入到 Codex 中。

> **i18n 说明：** 所有面向用户的输出（通知、提示、doctor/history 报告以及状态栏段）均可通过 `locale` 配置键（`en` / `ko` / `ja` / `zh`）进行本地化。技能描述始终保持英文。

### 安装之后（两种方式通用）

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

## 功能（逐项说明）

从触发交接的传感器，到它周围的安全网，逐项说明。

### 1. 五小时用量传感器

插件从不猜测用量，而是从每个工具的真实接口读取。

- **Claude Code** → **状态栏（status line）** 桥接器记录已用百分比与重置时间。如果数据缺失或过期，插件保持沉默，而不是凭猜测行动。
- **Codex** → 官方 **App Server**（`account/rateLimits/read`）为主传感器，会话 **JSONL** 中的 rate-limit 字段作为后备。

### 2. 自动胶囊交接

越过阈值后，插件会构建一个 **胶囊**：你的目标、已完成的工作、未决问题、下一步，外加真实的 Git 分支/提交与目前改动的文件。它通过原子发布（临时文件 → flush → 重命名）写入，因此绝不会读到写了一半的胶囊。胶囊 **不可变** 且 **经过完整性校验**（用内容哈希覆盖其字节以检测损坏或改动）；接收方代理用一个短租约占用它，校验、注入之后才标记为 **已消费**。每个胶囊只用一次。

### 3. 三种触发模式

你可以全局或按项目选择插件的积极程度：`auto`（静默交接）、`ask`（每个用量窗口询问一次）、`off`。默认阈值为 **80%**，因此在还有余量时就写好胶囊 —— 因为写语义胶囊本身也会消耗一点用量。

### 4. 已核实记忆的召回

与一次性的胶囊不同，插件会保留关于项目的 **长期记忆** —— 但只保留有证据（通过的测试、命令结果、源文件）支撑的事实。在会话的第一条提示时，它只在 token 预算（默认 800）内召回相关且有证据的记忆。它绝不保存猜测、隐藏推理或完整对话记录。

### 5. 渐进式项目知识

除胶囊外，插件还能携带项目的规范、格式与坑点。借助精简的 **INDEX** 和 **manifest**（文件哈希 + dirty 标记），接收方只读取自上次以来 **真正改动的部分**，而不是全部重读 —— 节省 token。**注意：** 该存储目前不会自动填充 —— 没有内置命令来注册知识文件，因此默认 INDEX 为空。显式注册功能尚在计划中。

### 6. 技能与命令

三个技能封装了这些行为：`handoff-ratelimit`（五小时触发）、`handoff-session`（`/handoff` 命令族）、`handoff-doctor`（诊断）。它们驱动下面列出的 `/handoff` 命令。

### 7. 内置安全机制

机密在保存前被抹去，胶囊经过完整性校验（可检测损坏或改动），而且胶囊始终被当作 *参考* 材料 —— 当前的用户指示、仓库策略、真实文件、Git 与测试都比它优先。见 [隐私与安全](#隐私与安全)。

### 8. 零依赖、跨平台内核

整个内核是纯 Node（基线 18），**没有 npm 依赖**，因此没有需要编译的东西，升级也不会被破坏。它在 Windows、macOS、Linux 上以 Node 18/20/22 进行测试。

---

## 命令

> ⚠️ **在 Claude Code 中，插件命令会按插件名加上命名空间。** 下面每个动作都会作为独立条目出现在斜杠菜单中，形如 **`/ai-handoff:handoff-<动作>`** —— 例如 `/ai-handoff:handoff-status`、`/ai-handoff:handoff-config set notification.method off`。裸的 **`/ai-handoff:handoff`** 用于恢复待处理的胶囊（也接受 `/ai-handoff:handoff <动作>` 形式）。裸的 `/handoff` 会返回 *“Unknown command”*。下表为便于阅读使用了简短的 `/handoff <动作>` 写法。在 **Codex** 中，这些动作来自内置技能，是 model-invoked 的 —— 直接用自然语言提出即可（例如 *“显示我的 ai-handoff 状态”*）。

| 命令 | 作用 |
|---|---|
| `/handoff` | 恢复一个等待中的胶囊（最常用的操作）。 |
| `/handoff status` | 查看当前交接状态。 |
| `/handoff preview` | 在注入之前先查看胶囊。 |
| `/handoff checkpoint` | 立刻手动保存一个胶囊。 |
| `/handoff create` | 在 `ask` 模式下批准创建胶囊。 |
| `/handoff skip` | 在 `ask` 模式下跳过本次使用窗口。 |
| `/handoff doctor` | 诊断胶囊 / 钩子 / 版本问题。还会报告指纹基准（git remote / git root / 路径）、数据存储位置，以及在其他目录或指纹下待处理的胶囊 —— 解释为何交接未出现。 |
| `/handoff history` | 查看每个项目的交接生命周期事件（created / resumed / skipped / created_from_approval）的审计日志。支持 `--limit N`（默认 20）和 `--cwd`。 |
| `/handoff config` | 查看 / 修改设置（阈值、模式、通知、记忆）。 |

记忆是 **显式的**：只有你主动选择、并且有真实证据（通过的测试、命令结果、源文件）时，才会保存事实。它绝不保存隐藏的推理或完整对话记录。

---

## 设置

以下是 **默认值**，随插件内置于 [`config/defaults.json`](config/defaults.json)：

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

> ⚠️ **不要编辑 `config/defaults.json`。** 它位于已安装的插件内部，每次更新都会被覆盖。请改在下面的 *用户配置* 文件里修改设置。

### 你的设置放在哪里

按你的操作系统在对应路径创建（或编辑）**一个** 文件：

- **Windows：** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS：** `~/Library/Application Support/ai-handoff/config.json`
- **Linux：** `~/.local/state/ai-handoff/config.json`（或 `$XDG_STATE_HOME/ai-handoff/config.json`）

该文件会 **深度合并到默认值之上**，所以只需写入你要改的键 —— 不要复制整个文件。

### 如何修改设置

由易到难，共三种：

1. **`/handoff config` 命令**（推荐）：
   - `/handoff config` —— 查看当前设置、用户配置路径，以及有效的键。
   - `/handoff config set notification.method off` —— 修改一个设置（取值会被校验）。
   - `/handoff config unset notification.method` —— 把一个设置还原为默认值。
2. **用自然语言让 Claude Code 或 Codex 来做** —— 例如：*“把 ai-handoff 的通知关掉”* —— 代理会替你运行该命令。
3. **自己编辑 JSON 文件** —— 打开文件（不存在就新建）并添加键。

无论哪种方式，都需要启动一个 **新的** 代理会话（或在 Claude Code 中运行 `/reload-plugins`）才能生效。

### 示例

一个在 75% 自动交接并关闭通知的用户配置 —— 其余保持默认：

```json
{
  "triggers": { "five_hour": { "threshold_percent": 75, "mode": "auto" } },
  "notification": { "method": "off" }
}
```

### 全部设置项

| 键 | 取值 | 含义 |
|---|---|---|
| `triggers.five_hour.enabled` | `true` / `false` | 五小时触发的总开关。 |
| `triggers.five_hour.threshold_percent` | 数字，如 `80` | 触发交接的使用率%。 |
| `triggers.five_hour.mode` | `auto` / `ask` / `off` | 静默交接 / 询问一次 / 什么都不做。 |
| `triggers.five_hour.burn_rate.enabled` | `true` / `false`（默认 `false`） | 可选：根据使用速度（预计到 100% 的时间）提前触发。启用后，在预计耗尽时间在 `runway_minutes` 以内时，额外触发交接（静态阈值照常生效）。 |
| `triggers.five_hour.burn_rate.runway_minutes` | 数字 5–120（默认 `30`） | 预计耗尽时间在此分钟数以内时触发。仅在 `burn_rate.enabled` 为 `true` 时生效。 |
| `capsule.completed_autocreate` | `true` / `false` | 任务完成时也生成胶囊。 |
| `locale` | `en` / `ko` / `ja` / `zh`（默认 `en`） | 本地化所有面向用户的输出：通知、提示、doctor/history 报告以及状态栏段。技能描述始终保持英文。 |
| `notification.method` | `os` / `terminal` / `off` | 系统弹窗 / 输出到终端 / **不发送通知**。 |
| `notification.fallback` | `terminal` / `off` | 仅当 `method` 为 `os` 且系统弹窗失败时使用。 |
| `memory.auto_recall` | `true` / `false` | 在你的第一条提示时召回已核实记忆。 |
| `memory.auto_recall_token_budget` | 数字，如 `800` | 召回记忆的最大 token 数。 |
| `statusline.show_handoff` | `true` / `false`（默认 `true`） | 在 Claude Code 状态栏中显示 `AH <pct>% · ⏳<n>` 段。注意：该段仅限 **Claude Code**，Codex CLI 内置了 rate-limit 显示但不支持外部命令驱动的状态栏段。 |

> 把 `notification.method` 设为 `off` 只会静音 **系统弹窗** —— 交接照常进行，且在 `ask` 模式下代理仍会在聊天中显示询问。

### 按项目

若只想为某个项目覆盖上述设置，添加一个以该项目 fingerprint 为键的 `project_overrides` 块：

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

## 隐私与安全

- **仅限本地。** 胶囊与记忆绝不离开你的机器。没有云端，也没有遥测。
- **机密会被抹去。** 在任何东西被保存之前，常见的机密模式（API 密钥、令牌、bearer 头、私钥）都会被替换为 `[REDACTED]`。
- **胶囊经过完整性校验。** 一旦发布，胶囊即为不可变，并用内容哈希校验，从而检测损坏或发布后的改动，校验失败的胶囊会被拒绝；只有它的投递 *状态* 会变化。（这能发现意外损坏与改动，但并非加密签名，无法防止对本地存储有写权限、且会重算哈希的攻击者。）
- **永远以你的指示为准。** 胶囊只是参考材料。当前的用户指示、仓库自身的策略、真实文件、Git 以及测试结果，全都优先于胶囊。

---

## 运行测试

```bash
npm test                 # 单元 + 集成测试
npm run validate:package # 检查插件 + 市场清单
```

测试是零依赖的纯 `node --test`。CI 矩阵会在 **Windows、macOS、Linux** 上以 **Node 18 / 20 / 22** 运行它们。

若还想针对真实的本地 Codex App Server 运行实时端到端测试：

```bash
AH_E2E=1 npm test
```

---

## 许可证

[MIT](LICENSE).
