[English](README.md) | [한국어](README.ko.md) | **日本語** | [中文](README.zh.md)

# claude-codex-auto-handoff

> **Claude Code** と **Codex** のどちらかが5時間の使用上限に近づいたら、作業中の内容を自動でもう一方へ引き継ぎます — どこまでやったかを説明し直す必要はありません。

> プラグインの内部名（マニフェストやコマンドで使う名前）は **`ai-handoff`** です。

---

## このプラグインが解決する問題

Claude Code と Codex には、それぞれ **5時間の使用上限** があります。作業に没頭しているときに片方の上限が来ると、たいていはもう一方のツールに切り替えて最初からやり直すことになります。目標は何だったか、すでにどんな判断をしたか、どのファイルを触ったか、何が残っているか——そのすべてを説明し直すのです。

この「説明し直し」は遅く、ミスを招きやすく、間違えやすいものです。

## このプラグインがすること

**リレー走**を思い浮かべてください。前の走者が疲れる前に次の走者へバトンを渡せば、次の走者はまったく同じ地点から走り続けられます。

1. **使用量を見張ります。** 小さなセンサーが、5時間ウィンドウをどれだけ使ったかを読み取ります。
2. **上限に近づくと**（既定値 **80%**）、いまどこまで進んだか——目標、重要な判断、次の作業、現在の Git ブランチ——を **capsule（カプセル）** という小さなファイルに書き出します。
3. **もう一方のツールを開くと**、そのカプセルを読み込み、新しいエージェントにどこから再開すればよいかを正確に示します。
4. **プロジェクトの検証済みの事実も覚えておき**、のちのセッションで関連するものだけを呼び戻します。

すべては **あなた自身のコンピューター内** で行われます。クラウドサーバーも、常駐デーモンも、別途用意するデータベースもありません。

## よく出てくる用語を、やさしい言葉で

| 用語 | 本当の意味 |
|---|---|
| **Capsule（カプセル）** | いまの作業の短いスナップショット（目標・判断・次の作業・ブランチ）。**一度** 使うと消費済みになります。 |
| **Handoff（引き継ぎ）** | そのスナップショットを一方のエージェント（Claude Code または Codex）からもう一方へ渡すこと。 |
| **Verified memory（検証済みメモリ）** | 証拠（成功したテスト、コマンドの実行結果、ソースファイル）で裏づけられた、プロジェクトの継続的な事実。推測は決して保存しません。 |
| **Hook（フック）** | エージェントが特定の瞬間（起動時、停止時、プロンプト送信時）に自動で実行する小さなスクリプト。 |

---

## 必要なもの

- **Node.js 18 以上**（ツール全体が純粋な Node 製で、**npm 依存ゼロ** です）。
- **Claude Code または Codex**（どちらか一方でも片方向で動きますが、両方そろうと真価を発揮します）。
- 初回インストール時に **フックを一度確認して信頼する** こと（[`hooks/hooks.json`](hooks/hooks.json) を参照）。

Node のバージョン確認:

```bash
node --version
```

---

## インストール

まずコードを取得します:

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

以下では `PATH/TO/claude-codex-auto-handoff` を、コードを取得した場所に置き換えてください。

### Claude Code

1. フォルダーからプラグインを読み込みます:

   ```bash
   claude --plugin-dir PATH/TO/claude-codex-auto-handoff
   ```

2. Claude は使用量センサーのために **追加の設定が一度だけ** 必要です。（Claude は使用量を *ステータスライン* から読み取りますが、プラグインがその枠を単独で占有できないため、このコマンドを一度実行します。既存のステータスラインがあれば安全に保持します。）

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --plugin-root PATH/TO/claude-codex-auto-handoff
   ```

   あとで元に戻すには:

   ```bash
   node PATH/TO/claude-codex-auto-handoff/core/cli.mjs setup:claude-statusline --restore
   ```

### Codex

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@<marketplace-name>
```

（Codex は使用量を公式の App Server から読み取るため、**追加のセンサー設定は不要** です。）

### インストール後（共通）

**新しい** エージェントセッションを開始し、案内が出たら lifecycle フックを **確認して信頼** してください。通常利用では「フック信頼のスキップ」フラグを使わないでください——信頼を自分で判断することが、このツールの肝心な点です。

---

## 仕組み（自動で起こる3つの瞬間）

プラグインは安全な瞬間にのみ動作し、実行中のツールを途中で止めることは決してありません。

- **エージェントが停止したとき**（`Stop`）: 使用量を確認します。選んだモードに応じて:
  - `auto` → 何も尋ねずにカプセルを作成します。
  - `ask` → 一度だけ尋ねます: *「カプセルを作成しますか? `/handoff create` | `/handoff skip`」*。
  - `off` → 何もしません。
- **エージェントが起動したとき**（`SessionStart`）: 待機中のカプセルがあれば検証（スキーマ、ファイルハッシュ、プロジェクト一致、有効期限）し、新しいエージェントに作業内容と薄いプロジェクトインデックスを示します。
- **最初のプロンプトを送ったとき**（`UserPromptSubmit`）: 関連する **検証済みの** プロジェクトメモリだけを、小さなトークン予算の範囲で呼び戻します。

典型的なリレーの様子:

```
Claude Code (80% 使用)  →  カプセル作成  →  Codex を開く  →  Codex が作業を再開
        ↑                                                            │
        └──────────────────  いつでも逆方向にも  ───────────────────┘
```

---

## コマンド

Claude Code または Codex の中で入力します。両方で同じです。

| コマンド | 動作 |
|---|---|
| `/handoff` | 待機中のカプセルを再開します（最もよく使う操作）。 |
| `/handoff status` | 現在の引き継ぎ状態を表示します。 |
| `/handoff preview` | 注入する前にカプセルを確認します。 |
| `/handoff checkpoint` | いますぐカプセルを手動保存します。 |
| `/handoff create` | `ask` モードでカプセル作成を承認します。 |
| `/handoff skip` | `ask` モードで今回の使用ウィンドウをスキップします。 |
| `/handoff recover` | カプセル / フック / バージョンの問題を診断します。 |

メモリは **明示的** です: 自分で選んだときだけ、しかも実際の証拠（成功したテスト、コマンド結果、ソースファイル）があるときだけ事実を保存します。隠れた推論や会話の全文は決して保存しません。

---

## 設定

設定は OS のデータフォルダーにある1つのファイルにまとまっています:

- **Windows:** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS:** `~/Library/Application Support/ai-handoff/config.json`
- **Linux:** `~/.config/ai-handoff/config.json`

既定値（[`config/defaults.json`](config/defaults.json) を参照）:

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

よく変える値:

- **自動で引き継ぐ:** `"mode": "auto"` に設定。
- **早め/遅めにトリガー:** `"threshold_percent"` を変更（例: `70` または `90`）。
- **オフにする:** `"mode": "off"` に設定。

プロジェクトごとに設定を上書きすることもできます。

---

## プライバシーと安全性

- **ローカル限定。** カプセルとメモリは決してあなたのマシンから出ません。クラウドもテレメトリもありません。
- **秘密情報は伏せられます。** 何かが保存される前に、よくある秘密のパターン（API キー、トークン、bearer ヘッダー、秘密鍵）を `[REDACTED]` に置き換えます。
- **カプセルは改ざん不可。** いったん発行されたカプセルは不変で、ハッシュで完全性を検証します。変わるのは配送 *状態* だけで、検証に失敗したカプセルは拒否されます。
- **常にユーザーの指示が優先。** カプセルは参考資料です。現在のユーザー指示、リポジトリ自身のポリシー、実際のファイル、Git、テスト結果は、すべてカプセルより優先されます。

---

## テストの実行

```bash
npm test                 # 単体 + 結合テスト
npm run validate:package # プラグインマニフェストの検査
```

テストは依存ゼロの純粋な `node --test` です。CI マトリクスは **Windows, macOS, Linux** 上で **Node 18 / 20 / 22** を実行します。

実際のローカル Codex App Server に対してライブの end-to-end テストまで実行するには:

```bash
AH_E2E=1 npm test
```

---

## ライセンス

[MIT](LICENSE).
