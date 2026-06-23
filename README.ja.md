<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

[English](README.md) | [한국어](README.ko.md) | **日本語** | [中文](README.zh.md)

# claude-codex-auto-handoff

Claude Code と Codex の間で作業を引き継ぐプラグインです。

片方の5時間使用上限が近づくと、現在の作業状態を **capsule** という小さなファイルに保存します。もう片方のツールはその capsule を読み、同じ場所から作業を続けられます。

プラグイン内部名は `ai-handoff` です。

困ったときや詳しい情報を見たい場合は、[こちらをクリックしてください](docs/advanced/README.ja.md)。

## なぜ必要ですか?

Claude Code と Codex には、それぞれ5時間の使用上限があります。作業中に片方の上限が来ると、もう片方のツールで目標、変更ファイル、次にやることを説明し直す必要があります。

このプラグインは、その引き継ぎ情報を代わりに準備します。

## capsule に入るもの

- 現在の目標
- 完了した作業
- 残っている作業
- 変更したファイル
- 現在の Git ブランチとコミット
- 次のツールが最初に確認すべきこと

capsule は一度使われると consumed として記録されます。

## 必要なもの

- Node.js 18 以上
- Claude Code または Codex
- 両方使うと双方向に引き継げます

Node の確認:

```bash
node --version
```

## インストール

### Claude Code

Claude Code 内で実行:

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

またはターミナルで実行:

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

その後、`/reload-plugins` を実行するか Claude Code を再起動してください。

### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

## Claude Code の追加設定

Claude Code の使用量は status line から読みます。そのため、次の設定を一度だけ実行します。

必要なのは `core/cli.mjs` が入っているローカルフォルダです。いちばん簡単な方法は、このリポジトリを clone することです。

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

次に、プラグインフォルダへ移動して実行します。

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

戻すときも、同じプラグインフォルダで実行します。

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

Codex には追加のセンサー設定は不要です。

## 仕組み

1. Claude Code または Codex が使用量を確認します。
2. 既定の80%に近づくと、プラグインが capsule を準備します。
3. `ask` mode では、先にユーザーへ確認します。
4. `auto` mode では、自動で capsule を作ります。
5. もう片方のツールを開くと、capsule を読んで続きから作業します。

Claude Code では plugin monitor が使用量を自動で見張れます。`scripts/usage-monitor.mjs` を自分で実行しないでください。

monitor には Claude Code v2.1.105 以上、interactive CLI session、user/personal-scope plugin install が必要です。monitor が使えない環境では Stop hook が fallback として動きます。

## 基本コマンド

| コマンド | 説明 |
|---|---|
| `/handoff` | 待機中の capsule を再開します |
| `/handoff status` | 現在の状態を表示します |
| `/handoff preview` | capsule の内容を確認します |
| `/handoff checkpoint` | 現在の状態を手動保存します |
| `/handoff history` | 現在のプロジェクトの引き継ぎ履歴を表示します |
| `/handoff recent` | 全プロジェクトの最近の capsule を表示します |
| `/handoff doctor` | 設定や capsule の問題を診断します |
| `/handoff config` | 設定を表示します |

Claude Code では `/ai-handoff:handoff-...` のように表示される場合があります。この README では読みやすさのため `/handoff` と書いています。

## 設定

設定ファイルの場所:

- Windows: `%LOCALAPPDATA%\ai-handoff\config.json`
- macOS: `~/Library/Application Support/ai-handoff/config.json`
- Linux: `~/.local/state/ai-handoff/config.json`

よく使う例:

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

重要な設定:

| Key | Default | Meaning |
|---|---:|---|
| `triggers.five_hour.threshold_percent` | `80` | 何%で引き継ぎを準備するか |
| `triggers.five_hour.mode` | `ask` | `ask`, `auto`, `off` のどれか |
| `approval.ttl_ms` | `900000` | 回答が有効な時間。既定は15分 |
| `sensors.claude.freshness_ms` | `10000` | Claude 使用量 sample の有効時間。既定は10秒 |
| `realtime.enabled` | `true` | Claude Code monitor を使うか |
| `realtime.poll_interval_ms` | `1000` | monitor の確認間隔。既定は1秒 |

設定を変えたら新しいセッションを開始してください。

## 注意点

- capsule と memory は自分のコンピューター内に保存されます。
- API key や token などの秘密情報は保存前に伏せられます。
- capsule は参考資料です。実際のファイル、Git 状態、テスト結果を優先してください。
- monitor は実行中の回答を中断しません。現在の回答が終わった後に反応することがあります。

## 開発者向けテスト

```bash
npm test
npm run validate:package
```

## ライセンス

[MIT](LICENSE)
