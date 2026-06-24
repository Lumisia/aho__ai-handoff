[English](README.md) | [한국어](README.ko.md) | **日本語** | [中文](README.zh.md)

# 高度なヘルプ

ai-handoff が期待どおりに動かないときに見るページです。

## 目次

1. [最初に確認すること](#最初に確認すること)
2. [capsule が見えません](#capsule-が見えません)
3. [Claude Code と Codex がつながりません](#claude-code-と-codex-がつながりません)
4. [保存場所と AI_HANDOFF_ROOT](#保存場所と-ai_handoff_root)
5. [高度な設定キー](#高度な設定キー)
6. [設定できる引数](#handoff-clear-arguments)
7. [Codex capsule の動作](#codex-capsule-の動作)

## 最初に確認すること

- Claude Code と Codex を同じプロジェクトフォルダで実行しているか確認してください。
- `/handoff status` で、このプロジェクトに待機中の capsule があるか確認します。
- `/handoff recent` で、別プロジェクトに保存された capsule がないか確認します。
- `/handoff doctor` で、保存場所、プロジェクト識別子、capsule 状態を診断します。
- 設定を変えた後は、新しいセッションを開始するか、Claude Code で `/reload-plugins` を実行してください。

Claude Code monitor には Claude Code v2.1.105 以上、interactive CLI session、user/personal scope のプラグインインストールが必要です。monitor が使えない環境では、現在の回答が終わったあと Stop hook が代わりに動きます。

## capsule が見えません

まず `/handoff doctor` を実行してください。多くの場合、原因は次のどれかです。

- 別のフォルダで実行したため、プロジェクト識別子が変わっています。
- capsule はすでに一度再開され、consumed 状態になっています。
- Claude Code と Codex が別々の保存場所を見ています。
- `ask` mode で capsule 作成をまだ承認していません。

確認順:

```text
/handoff status
/handoff recent
/handoff history
/handoff doctor
```

`recent` には出るのに `status` には出ない場合、別のプロジェクトフォルダに保存されている可能性が高いです。

## Claude Code と Codex がつながりません

- 両方のツールにプラグインをインストールしてください。
- プラグイン内部名は `ai-handoff` です。
- Claude Code は使用量を status line から読むため、追加設定コマンドを一度実行する必要があります。
- Codex には追加の status line 設定は不要です。
- Windows の Store/MSIX 版 Claude アプリでは `%LOCALAPPDATA%` が分かれることがあります。その場合は、両方のツールで同じ `AI_HANDOFF_ROOT` を設定してください。

Windows で互いの capsule が見えない場合、まず `AI_HANDOFF_ROOT` を確認するのが最短です。

## 保存場所と AI_HANDOFF_ROOT

`AI_HANDOFF_ROOT` が設定されていれば、その場所を使います。なければ OS の既定場所を使います。

| OS | 既定の保存ルート |
|---|---|
| Windows | `%LOCALAPPDATA%\ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux | `$XDG_STATE_HOME/ai-handoff` または `~/.local/state/ai-handoff` |

主な下位パス:

| 内容 | パス |
|---|---|
| 設定 | `<root>/config.json` |
| プロジェクトデータ | `<root>/projects/<fingerprint>` |
| capsule | `<root>/projects/<fingerprint>/handoff` |
| memory | `<root>/projects/<fingerprint>/memory` |
| Claude 使用量 sample | `<root>/sensors/claude` |

Windows で共有保存場所を指定する例:

```powershell
[Environment]::SetEnvironmentVariable("AI_HANDOFF_ROOT", "C:\Users\<you>\ai-handoff-store", "User")
```

macOS/Linux の例:

```bash
export AI_HANDOFF_ROOT="$HOME/ai-handoff-store"
```

環境変数を変えた後は、Claude Code と Codex の両方を再起動してください。

## 高度な設定キー

`/handoff config` は現在の設定を表示します。値は期待される型と範囲に合わせる必要があります。

| キー | 説明 |
|---|---|
| `triggers.five_hour.burn_rate.enabled` | 使用量の減りが速いとき、早めに引き継ぎを準備するか |
| `triggers.five_hour.burn_rate.runway_minutes` | 残り時間が何分以下なら準備するか、5-120 |
| `capsule.completed_autocreate` | 作業完了に見える状態でも自動 capsule を作るか |
| `codex.inline_final_capsule` | Codex auto mode で最終回答の fenced capsule flow を使うか。既定値は `true` |
| `codex.stop_continuation_auto_summary` | 旧 Codex Stop `decision:block` summary continuation を許可するか。既定値は `false` |
| `codex.stop_continuation_ask` | 旧 Codex Stop `decision:block` ask continuation を許可するか。既定値は `false` |
| `codex.degraded_fallback_on_stop` | Codex が Stop 時点で初めて threshold を超えた場合に degraded capsule を保存するか。既定値は `true` |
| `clear.auto.enabled` | SessionStart 時に古い used capsule の自動削除をオン/オフするか。既定値は `false` |
| `clear.older_than_days` | used capsule を削除する既定の経過日数。既定値は 30 日 |
| `handoff.notify_newer_pending` | より新しい待機中 capsule があるとき通知するか |
| `locale` | メッセージ言語、`en`, `ko`, `ja`, `zh` |
| `debug.stop_log` | Stop hook の判断ログを残すか |
| `memory.auto_recall` | 会話開始時に検証済み memory を自動で呼び出すか |
| `memory.auto_recall_token_budget` | 自動 memory recall に使う token 予算 |
| `statusline.show_handoff` | Claude status line に handoff 情報を表示するか |
| `notification.fallback` | OS 通知に失敗したとき terminal 通知を使うか |

通常は `threshold_percent`, `mode`, `realtime.enabled` だけで十分です。

<a id="handoff-clear-arguments"></a>

## 6. 設定できる引数

`/handoff clear` は最初の引数で削除範囲を決めます。

```text
/handoff clear <this_project, used, consume, pending, expired> [--older-than 7d] [-c]
```

| 引数 | 説明 |
|---|---|
| `this_project` | 現在のプロジェクト fingerprint の ai-handoff 状態フォルダ全体を削除します。ソースリポジトリは削除しません。別名: `this-project`, `project`. |
| `used` | 終了状態の capsule を削除します。対象は `CONSUMED`, `EXPIRED`, `REJECTED`, `SKIPPED`, `FAILED` です。 |
| `consume` | 消費済み capsule だけを削除します。`consumed` の別名です。 |
| `consumed` | 消費済み capsule だけを削除します。 |
| `pending` | 待機中の capsule を削除します。対象は `AVAILABLE`, `DEGRADED_AVAILABLE` です。 |
| `expired` | 期限切れ capsule だけを削除します。 |

| オプション | 説明 |
|---|---|
| `--older-than 7d` | 指定した期間より古い capsule だけを削除します。`ms`, `m`, `h`, `d` を使えます。数字だけなら日数です。 |
| `-c`, `--confirm`, `--yes` | `this_project` の削除をすぐ承認します。指定しない場合は確認用 preview を返します。 |

例:

```text
/handoff clear used
/handoff clear used --older-than 7d
/handoff clear --older-than 7d
/handoff clear pending
/handoff clear this_project
/handoff clear this_project -c
```

scope なしで `--older-than` だけを指定すると、scope は `used` になります。`--older-than` を省略した場合、used 系 scope は `clear.older_than_days` の設定値を使います。既定値は 30 日です。

自動削除は手動コマンドとは別です。`clear.auto.enabled` を `true` にすると、SessionStart 時に古い `used` capsule の削除を実行します。既定値はオフ (`false`) です。バックグラウンドで常時動くのではなく、SessionStart hook が実行されたときだけ動きます。自動削除の経過日数は `clear.older_than_days` を使い、既定値は 30 日です。

## Codex capsule の動作

Codex Stop hook は capsule 作成のために既定で `decision: "block"` を使いません。Codex ではこの値が `reason` をユーザーに見える continuation prompt にするため、内部指示のような capsule 作成文が hook feedback や通常の assistant 出力として表示される可能性があります。

既定の流れは次のとおりです。

```text
UserPromptSubmit または PostToolUse
  -> threshold を検知
  -> 最終回答に ai-handoff-capsule footer を付ける developer context を注入
Assistant final answer
  -> ユーザーへ通常どおり回答
  -> fenced ai-handoff-capsule JSON を追加
Stop
  -> capsule を parse して publish
  -> {"continue": true} を返す
```

Codex が Stop 時点で初めて threshold 超過を検知した場合、回答はすでに終わっているため自然に footer を追加できません。既定では `DEGRADED_AVAILABLE` capsule を保存し、静かに続行します。旧来の visible continuation を明示的に使いたい場合だけ、`codex.stop_continuation_auto_summary` または `codex.stop_continuation_ask` を `true` にしてください。
