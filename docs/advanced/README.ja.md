# AI Handoff 詳細ガイド

[한국어](README.md) | [English](README.en.md) | **日本語** · [クイックスタート](../README.ja.md)

インストール方法、全 CLI コマンドとオプション、ダッシュボード操作、開発手順を説明します。エージェントのスキル一覧から AI Handoff の `handoff-checkpoint` を選んで保存し、`handoff` で引き継ぎます。

- [インストールオプション](#インストールオプション)
- [コマンドリファレンス](#コマンドリファレンス)
- [TUIとデスクトップ](#tuiとデスクトップ)
- [カプセルとプライバシー](#カプセルとプライバシー)
- [ローカルファイルとプロジェクト構成](#ローカルファイルとプロジェクト構成)
- [開発](#開発)
- [トラブルシューティング](#トラブルシューティング)

## インストールオプション

Windows、macOS、Linux、WSL で Claude Code、Codex、または両方を使います。連携をインストールする前に、対象エージェントをインストールして一度起動し、設定フォルダーを作成してください。CLI リリースは x86_64 と ARM64 に対応し、実行に Node.js や Rust は不要です。現在のリリースワークフローが提供する GUI インストーラーは Windows 用です。

### 公式リリースから直接ダウンロード · 推奨

**[公式リリースを開く](https://github.com/Lumisia/aho__ai-handoff/releases)** → リリースの **Assets** を展開し、以下のファイルを選びます。`Source code` ではなく、インストーラーまたは CLI アーカイブを選んでください。`x86_64` は x64、`aarch64` は ARM64 を表し、Apple Silicon は ARM64 です。

| OS・CPU | 配布ファイル | 内容 |
|---|---|---|
| Windows x64 · 推奨 | `ai-handoff-gui-windows-x86_64-setup.exe` | GUI + CLI/TUI |
| Windows ARM64 · 推奨 | `ai-handoff-gui-windows-aarch64-setup.exe` | GUI + CLI/TUI |
| Windows x64 · CLI/TUI のみ | `ai-handoff-cli-windows-x86_64.zip` | CLI + ホスト |
| Windows ARM64 · CLI/TUI のみ | `ai-handoff-cli-windows-aarch64.zip` | CLI + ホスト |
| macOS · Apple Silicon | `ai-handoff-cli-darwin-aarch64.tar.gz` | CLI + ホスト |
| macOS · Intel | `ai-handoff-cli-darwin-x86_64.tar.gz` | CLI + ホスト |
| Linux x64 | `ai-handoff-cli-linux-x86_64.tar.gz` | CLI + ホスト |
| Linux ARM64 | `ai-handoff-cli-linux-aarch64.tar.gz` | CLI + ホスト |

WSL では Linux 用のファイルを選び、Windows GUI は Windows 側でインストールします。同じ名前に `.sha256` が付いたファイルも取得し、Windows の `Get-FileHash -Algorithm SHA256`、macOS の `shasum -a 256`、Linux の `sha256sum` で計算した値と照合します。

**Windows GUI:** 名前が `-setup.exe` で終わるインストーラーを実行し、ウィザードを完了します。CLI/TUI とバックグラウンドホストが含まれ、エージェント連携の登録も行います。インストール後、スタートメニューで **aho** または **AI Handoff** を検索して GUI を開きます。署名と SmartScreen については、下記の Windows の説明を参照してください。

**CLI アーカイブを手動でインストールする場合:**

1. アーカイブを継続して使うフォルダーに展開します。`ai-handoff` と `ai-handoff-host` を同じフォルダーに置き、名前を変えないでください。Windows では両方に `.exe` が付きます。ホストは必要に応じてデーモンを起動するプログラムです。
2. Windows では、展開したフォルダーで PowerShell を開いて連携を登録します。

```powershell
.\ai-handoff.exe install --yes
```

3. macOS・Linux では、展開したフォルダーでターミナルを開き、実行権限を確認して連携を登録します。

```sh
chmod +x ./ai-handoff ./ai-handoff-host
./ai-handoff install --yes
```

4. 展開したフォルダーを OS のユーザー **PATH** に追加し、ターミナルを開き直します。PATH は、ターミナルがコマンドを探すフォルダーの一覧です。これで `ai-handoff` から TUI を開けます。Windows のインストールでは短縮コマンド `aho` も用意されます。

インストール後にこれらのファイルを移動した場合は、新しい場所から `install --yes` を再実行してフックのパスを更新してください。その後は下記の共通手順に従い、エージェントの再起動・Codex フックの信頼設定・`doctor` での確認を行います。以下のスクリプトは、ダウンロードと登録をまとめて行う別の方法です。

### Homebrew

以前案内していた Homebrew tap の公開ソースを確認できないため、このガイドでは GitHub リリースからのインストールを案内します。macOS デスクトップ cask の提供も未確認です。

### 別の方法: Windows PowerShell スクリプト

CLI/TUI とデスクトップアプリを一緒にインストールします。

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

CLI/TUI だけなら `-WithGui` を省略します。スクリプトはリリースファイルをダウンロードして検証し、CLI とバックグラウンドホストを `%USERPROFILE%\.ai-handoff\bin` または `$env:AI_HANDOFF_HOME\bin` に配置し、ユーザー PATH を追加して `ai-handoff install` を実行します。

| スクリプトオプション | 効果 |
|---|---|
| `-Yes` | CLI 連携の確認を省略します。`-WithGui` と併用すると GUI インストーラーも無人モードで実行します。 |
| `-Only codex` / `-Only claude` / `-Only claude-code` | CLI 連携の対象を一つに絞ります。省略時はインストール済みエージェントを検出します。 |
| `-Version vX.Y.Z` | バージョンを固定します。既定の `latest` は GitHub の「Latest」表示ではなく、最も新しい安定版 `vX.Y.Z` を選びます。 |
| `-WithGui` | CLI/TUI を含む Windows GUI インストーラーも取得して実行します。 |
| `-DryRun` | CLI 連携設定に `--dry-run` を渡します。ダウンロード、バイナリ配置、PATH 変更、指定した GUI インストールは実行されます。 |

バージョンを固定して Codex だけ連携する例:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex -Version v2.2.8
```

GUI インストーラーは独自の連携設定も行うため、`-Only` は別の GUI インストール段階まで制限しません。ダウンロードやバイナリ置換を伴わない設定プレビューには、インストール済みの `ai-handoff install --dry-run` を使います。

Windows インストーラーは現在未署名です。SmartScreen が **Windows によって PC が保護されました / 不明な発行元** と表示した場合は、[公式リリース](https://github.com/Lumisia/aho__ai-handoff/releases)のファイルであることと対応する `.sha256` を確認してから **詳細情報 → 実行** を選びます。SmartScreen 全体を無効にしないでください。

### 別の方法: シェルスクリプト

macOS、Linux、WSL、Git Bash で使います。

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

| スクリプトオプション | 効果 |
|---|---|
| `--yes`, `-y` | 連携設定の確認を省略します。 |
| `--only codex` / `--only claude` / `--only claude-code` | 対象エージェントを一つに絞ります。 |
| `--version vX.Y.Z` | バージョン固定。`latest` は最も新しい安定リリースです。 |
| `--dry-run` | 連携設定の変更だけをプレビューします。ダウンロードとバイナリ置換は実行されます。 |
| `--with-gui` | 現在は未対応の案内を表示するだけです。このスクリプトは CLI/TUI のみをインストールします。 |
| `--help`, `-h` | ヘルプを表示し、インストール前に終了します。 |

オプションは `sh -s --` の後に付けます。例: `--yes --only codex --version v2.2.8`。バイナリは `${AI_HANDOFF_HOME:-$HOME/.ai-handoff}/bin` に配置されます。コマンドが見つからない場合は表示された場所を PATH に追加します。`AI_HANDOFF_HOME` を指定しない場合、この配置先と下記 OS 別の実行データ保存先は異なることがあります。

インストール後にエージェントを再起動します。Codex では `/hooks` を開いて AI Handoff のフックを信頼し、`ai-handoff doctor` で連携を確認します。

## コマンドリファレンス

プロジェクト固有のコマンドは対象プロジェクトのフォルダーで実行します。プロジェクト識別値によりカプセルの保存領域が決まります。`ai-handoff --help`、`ai-handoff <command> --help`、`-h` でインストール済み CLI のヘルプを確認できます。

### エージェントスキル

チャットのスキル一覧から選び、実際の CLI はエージェントが実行します。既定のプラグインは次の名前を公開します。スキルの選択とターミナルコマンドは別の入口です。

| プラグインスキル | 用途・CLI 対応 |
|---|---|
| `ai-handoff:handoff` | 自分宛て、または宛先なしの最新待機カプセルを受け取って続行します。CLI: `ai-handoff handoff --agent <self>`。 |
| `ai-handoff:handoff-checkpoint` | 現在の作業を記述してすぐ保存します。任意の `md` / `json` と要約を渡せます。CLI の作成規約: `ai-handoff checkpoint guidance --agent <self> --json`。 |
| `ai-handoff:handoff-config` | 共通設定の確認・変更。CLI: `ai-handoff config list/get/set`。 |
| `ai-handoff:handoff-doctor` | インストール、フック、デーモン、IPC、カプセルの診断。CLI: `ai-handoff doctor`。 |

`/handoff checkpoint` のような短縮表記が全ホスト共通の実行可能なチャットコマンドであるとは限りません。一覧に表示されるスキル名を選ぶか、CLI を使ってください。短い要約だけでは完了事項、残りの作業、リスク、次の指示は埋まりません。

### 全トップレベルコマンド

| コマンド | 用途 |
|---|---|
| `ai-handoff` | ターミナルダッシュボード。Windows では短縮コマンド `aho` も使えます。リダイレクト時は静的な状態要約です。 |
| `ai-handoff dashboard` | 実行中の CLI の隣に `AI Handoff.exe` がある場合のみ GUI を起動します。 |
| `ai-handoff checkpoint [md\|json\|guidance]` | カプセル保存、または作成規約の表示。 |
| `ai-handoff handoff` | 待機カプセルを受け取り、消費済みにします。 |
| `ai-handoff retarget <capsule-id>` | 待機カプセルの宛先変更、または宛先解除。 |
| `ai-handoff config <get\|set\|list>` | 共通設定の読み取り・変更。 |
| `ai-handoff doctor` | 連携・実行環境の診断。 |
| `ai-handoff account <list\|status\|doctor>` | 保存アカウント、ローカル状態、アカウント構成の確認。 |
| `ai-handoff usage` | ローカルログによるトークン・費用の推定。 |
| `ai-handoff hook <event>` | stdin JSON → ローカル IPC → フック応答 JSON のアダプター。 |
| `ai-handoff daemon <run\|status>` | ローカルワーカーの実行・接続確認。 |
| `ai-handoff install` | エージェント連携のインストール・修復。 |
| `ai-handoff update` | 公式インストーラーで最新安定 CLI/TUI を取得。 |
| `ai-handoff uninstall` | 管理対象の連携を削除。データは既定で保持し、バイナリ削除は実行場所に依存します。 |
| `ai-handoff autostart <on\|off\|status>` | Windows ログオン自動起動と設定の管理。 |
| `ai-handoff statusline` | Claude の状態行 JSON を stdin で受け、ローカル上限サンプルを記録して状態行を表示。 |

### Checkpoint

| 引数・オプション | 意味 |
|---|---|
| `md`, `json` | 作成・保存形式を明示します。省略時は入力形式を検出し、設定された保存形式を使います。 |
| `guidance` | エージェント、言語、形式、項目数上限、入力テンプレート、正確な保存コマンドを JSON で表示。保存はしません。 |
| `--agent <id>` | 作成者。入力の `agent` より優先し、両方なければ `codex`。別名を正規化し、他の ID も受け付けます。 |
| `--target <id>` | 受信者。入力の `target` より優先し、両方なければ誰でも受信できます。 |
| `--message <text>` | 入力に目標がない場合の説明・目標。作業要約の自動生成ではありません。 |
| `--file <path>` | UTF-8 の JSON/Markdown 下書き。PowerShell/POSIX で推奨。省略時は EOF まで stdin を読みます。 |
| `--format <md\|json>` | 設定形式を上書きします。主に `guidance` 用。位置引数の形式と矛盾するとエラーです。 |
| `--json` | `guidance` 専用の圧縮 JSON。省略しても整形済み JSON です。 |
| `--episode <id>` | 上限確認の質問で渡された進行 ID に保存を関連付けます。渡された ID を使い、通常の手動保存では不要です。 |

まず guidance を確認し、返された言語・テンプレートで記述して、返されたコマンドで保存します。

```sh
ai-handoff checkpoint guidance --agent claude-code --format json --json
```

JSON 下書きの例:

```json
{
  "goal": "設定変更を完了する",
  "done": ["パーサーの修正済み"],
  "remaining": ["GUI 呼び出し元の確認"],
  "risks": ["GUI の検証は未実行"],
  "next_prompt": "GUI 呼び出し元を確認し、関連する検証を実行する"
}
```

`checkpoint.json` に保存し、受信者を指定する例:

```sh
ai-handoff checkpoint json --agent claude-code --target codex --file checkpoint.json
```

`--message` だけでは会話や完了作業を復元できません。スキルが現在の文脈から下書きを埋めます。認証情報や会話ログ原文を含めないでください。

### Handoffとretarget

| 引数・オプション | 意味 |
|---|---|
| `handoff --agent <id>` | 受信 ID。既定は `codex`。宛先なし、またはその ID 宛てのカプセルを選びます。 |
| `handoff --peek` | 消費状態を変更せず、最新の自分宛て・宛先なしカプセルをプレビューします。 |
| `handoff --id <capsule-id>` | その待機カプセルだけを受信します。他の宛先でも取得し、`--force` より優先します。消費用の指定です。 |
| `handoff --force` | 他の宛先も候補に入れて最新の受信可能なカプセルを取得します。宛先を無視した記録が残ります。複数ある場合は確認後に `--id` を使います。消費用です。 |
| `retarget <capsule-id> --to <id>` | 待機カプセルの宛先変更。`--to` を省略すると宛先なしになります。 |

```sh
ai-handoff handoff --agent codex --peek
ai-handoff handoff --agent codex
```

ID を確認してから誤った宛先を直す例:

```sh
ai-handoff retarget <capsule-id> --to codex
```

消費応答には `hookSpecificOutput.additionalContext` と `consumed: true` が含まれます。終了コードが 0 のときの `{}` は対象の待機カプセルなし、`pending: false` と `others` は別の宛先の一覧です。失敗時も `{}` が出ることがあるため終了コードを確認します。`--peek` は `preview` を返し、`--id` / `--force` は適用しません。特定 ID のプレビューとは見なさないでください。セッション開始は通知だけで、受信は明示的な handoff で行います。他のエージェントを起動・切替せず、実行したエージェントに保存済みの文脈を渡します。

### 設定と診断

| コマンド・オプション | 意味 |
|---|---|
| `config list` | CLI で編集できるキーと実効値。 |
| `config get <key>` | 未設定時の既定値を含む実効値。 |
| `config set <key> <value>` | 対応する設定を保存。無関係の TOML キーは保持します。 |
| `doctor --json` | 機械可読な診断結果。 |
| `doctor --fix` | 実行ディレクトリ修復とデーモン起動を試し、残りの手動操作を案内。`--json` と併用可能です。 |

```sh
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff config set triggers.five_hour.mode ask
ai-handoff config set capsule.language ja
ai-handoff doctor --json
```

5 時間上限のモードは `off`、`ask`、`auto`。既定は有効・80%・`ask` です。`ask` は保存するか質問し、`auto` は質問せず checkpoint の作成を要求し、`off` はトリガーを無効にします。保存後は元の作業を再開します。下書きの作成・保存はエージェントに依存し、会話バックアップや自動エージェント切替ではありません。

フックが有効な対話型 CLI とローカルのデスクトップセッションでは、`ask` はセッションごとに同じ5時間枠で一度だけ **はい／いいえ／その他** を提示します。「その他」には指示を自由に入力できます。選択画面が使えない場合はチャットで尋ねます。回答やキャンセルの後は、提供元のリセット時刻を過ぎ、新しい枠で使用率が再びしきい値に達したときに尋ねます。リセット時刻が不明な場合は最初の質問から5時間を目安にします。

他の CLI 設定には `daemon.idle_timeout_seconds`、`autostart.enabled`、`statusline.show`、UI の `language`、カプセル形式・項目数、消費速度、ターミナルの `theme.*` 色があります。正確なキーは `config list` で確認し、GUI 専用設定はデスクトップの設定画面で編集します。実際のログオン登録には `autostart on/off` を使います。boolean の変更だけとは別の操作です。

### アカウントと使用量

| コマンド・オプション | 意味 |
|---|---|
| `account list [--json]` | 両エージェントの保存アカウントとアクティブ表示。 |
| `account status [--json]` | 追加済みのアクティブアカウント、ローカルサンプルのプラン・上限。 |
| `account status --fetch [--json]` | 保存済みのアクティブアカウントで Claude 使用量も取得。OAuth 更新が発生する場合があります。Codex はローカルのままです。 |
| `account doctor [--json]` | ログイン、保存アカウント、PATH 上の公式 CLI、実行中エージェントの診断。 |
| `usage --group-by <day\|model\|project\|source>` | 使用量の分類。省略時は合計とエージェント別の要約。 |
| `usage --source <claude\|codex>` | 一方に絞ります。ここでは `claude-code` ではなく `claude` です。 |
| `usage --since <YYYY-MM-DD>` | 指定日以降を含めます。 |
| `usage --json` | JSON 出力。上記フィルターと併用可能。 |

```sh
ai-handoff account list
ai-handoff account status --json
ai-handoff account doctor
ai-handoff usage --group-by model --source codex --since 2026-01-01 --json
```

追加、切替、削除はローカル TUI/GUI のアカウント画面で行います。選択したアカウントでエージェント CLI を起動するには、TUI のアカウント詳細で `l` を押します。保存済み・現在のログイン認証情報を変更する場合があり、スキルはアカウントを切り替えません。ローカルログからの `usage` のトークン・費用推定とプロバイダーのアカウント上限は別の値です。推定は公式の請求額や割当量ではありません。

### フック、デーモン、状態行

| コマンド・オプション | 意味 |
|---|---|
| `hook <session-start\|user-prompt\|post-tool-use\|stop>` | イベント処理。通常はインストール済みフックが stdin JSON とともに呼び出します。 |
| `hook ... --agent <codex\|claude-code>` | 連携エージェント。既定 `codex`。カプセルコマンドとは異なり、この 2 値だけです。 |
| `daemon run` | フォアグラウンド実行。要求がなければ既定 60 秒で終了。 |
| `daemon run --stay-alive` | 手動診断用に停止するまで実行。 |
| `daemon <run\|status> --home <path>` | 今回の `AI_HANDOFF_HOME` を指定。ホスト管理・分離環境用。 |
| `daemon status` | `reachable` / `unreachable`。包括的な診断ではありません。 |
| `statusline` | Claude から stdin を受けます。専用オプションはなく、記録済みの以前の状態行コマンドがあれば連結します。 |

フックは必要に応じてデーモンを起動し、アイドル終了は正常です。Windows ではインストール済みのバックグラウンドホストがエージェントのサンドボックス外でワーカーを起動します。ログオン自動起動は別で、既定では無効です。

手動診断では、一方のターミナルで `ai-handoff daemon run --stay-alive`、別のターミナルで `ai-handoff doctor` を実行します。終了後はフォアグラウンドワーカーを `Ctrl+C` で停止します。フック JSON は stdout、診断は stderr です。テスト入力に認証情報を含めないでください。

### インストール、更新、削除、自動起動

| コマンド・オプション | 効果 |
|---|---|
| `install --dry-run` | 連携設定の変更を適用せずプレビュー。 |
| `install --yes` | 連携設定の質問を省略。 |
| `install --agents <ids>` | `codex,claude-code` のようにカンマ区切り。`claude` も可。省略時は検出。 |
| `install --no-plugin` | 既定プラグインの代わりに旧直接フック方式。互換性・診断用です。 |
| `update` | 最新安定 CLI/TUI の公式インストーラーを取得・実行。固定にはインストールスクリプトを使います。 |
| `update --gui` | Windows GUI も更新。他の OS では CLI/TUI のみ。 |
| `uninstall` | 管理対象の連携、起動登録、ランチャーを削除。データは既定で保持し、バイナリ削除は場所に依存します。 |
| `uninstall --keep-store` | データ保持を明示。再インストール向け。 |
| `uninstall --purge-store` | 確認後に `store/`、`logs/`、`ipc/` も削除。 |
| `uninstall --gui` | Windows GUI も削除。CLI/TUI の削除を含みます。 |
| `uninstall --all` | GUI/CLI/TUI の削除と store/log/IPC 削除をまとめて要求。 |
| `uninstall --yes` | データ削除の確認を省略。選択したオプションの効果を先に確認してください。 |
| `autostart on` | Windows ユーザーのログオンタスク登録と設定有効化。登録失敗時は HKCU Run を使用。 |
| `autostart off` | タスク・Run 登録の削除と設定無効化。 |
| `autostart status` | 設定値と実際のタスク・Run 登録を表示。 |

`--keep-store` は `--purge-store` / `--all` と併用できません。削除対象はカプセル・引き継ぎ履歴を含む store/log/IPC で、`accounts/` と `config.toml` は含みません。purge なしの uninstall も削除操作であり、プレビューではありません。CLI の自己削除は実行データルートの `bin/` から実行した場合のみ試みます。macOS/Linux のシェルインストーラーの別の既定 bin にはバイナリが残る場合があるため、表示された配置先を確認してから手動で整理します。自動起動登録は現在 Windows 用で、他の OS でもフックによるオンデマンド起動は可能です。

プラットフォームの制約: 現在の削除経路は Windows のタスク・レジストリコマンドを呼びます。ソース上は Unix に `schtasks` がない場合、連携削除前に失敗する可能性があります。今回 Unix 実機での再現は行っていません。このエラーを削除成功と見なさず、原因の確認・報告後に手動整理を検討してください。

## TUIとデスクトップ

ターミナルで `aho` または `ai-handoff` を実行すると TUI が開きます。`aho` は Windows のインストールで用意される `aho.cmd` 短縮コマンドです。他の環境では `ai-handoff` を使います。代わりに `ai-handoff tui` も使えます。Windows GUI はスタートメニューで **aho** を検索して開きます。`ai-handoff dashboard` はその CLI の隣の `AI Handoff.exe` だけを探すため、別のフォルダーにある GUI がスタートメニューから開けても、このコマンドでは失敗する場合があります。

### TUIキー

該当領域にフォーカスがあるときの操作です。編集中のフィールドは保存・取消までキー入力を受け取ります。

| 画面 | キー |
|---|---|
| 共通タブ | `Tab` / `Shift+Tab` または `1`–`6`: 概要、カプセル、使用量、アカウント、連携、設定。 |
| タブバー | `←/→`: タブ。`↓` / `Enter` / `Space`: 内容へ。 |
| 戻る・終了 | `q` / `Esc`: 内容・サブ画面から戻る。タブバーの終了確認後にもう一度押すと終了。 |
| カプセルツリー | `↑/↓` または `k/j`: 選択。`→` / `Enter` / `Space`: 展開・開く。`←`: 折りたたむ。`r`: 再読込。 |
| カプセル詳細 | `↑/↓` または `k/j`: フィールド。`e` / `Enter`: 編集。編集中 `Enter`: 保存、`Esc`: 取消。`s`: ライフサイクル状態変更。`d` の後 `d` / `y`: 削除。`←`: ツリー。 |
| 使用量 | `g`: 分類切替。`→` / `Enter` / `Space`: 詳細。`←`: チャート。 |
| アカウント | `↑/↓` または `k/j`: 選択。ツリー `a` / `+`: 追加。`Enter` / `→` / `Space`: 詳細。詳細 `s`: 切替、`l`: 起動、`a`: 追加、`d` の後 `d` / `y`: 削除、`←`: ツリー。`r`: 使用量更新。 |
| 連携 | `d`: 診断。`l`: ログ。`r`: 修復画面。`Enter` / `Space`: 詳細。修復 `↑/↓` または `k/j`: 選択、`Enter` / `y`: 実行・確認、`n`: 確認取消。 |
| 設定 | `↑/↓` または `k/j`: 選択。分類 `Enter` / `→` / `Space`: 詳細。詳細 `←` / `-`: 前の値、`→` / `+` / `=`: 次の値、`Enter` / `Space`: 切替・編集、`r`: リセット。`/`: 検索。 |

ライフサイクル編集は保存状態を変更し、エージェントの会話に内容を注入しません。削除はカプセルを消します。アカウント更新はプロバイダー API を呼ぶ場合があります。変更前に画面の確認と下部のキー案内を確認してください。

### デスクトップ操作

サイドバー、アカウント・カプセルメニュー、設定を使います。`Esc` は設定オーバーレイや開いたカプセルメニューを閉じ、`Enter` は対応する設定入力を保存します。TUI の数字タブショートカットはありません。既定ではウィンドウを閉じると終了します。継続する場合は設定の **トレイに閉じる** を有効にし、トレイメニューで再表示・終了します。更新確認は通知表示だけで、自動インストールはしません。

## カプセルとプライバシー

エージェントが目標、完了事項、残りの作業、リスク、ファイルメモ、次の指示を記述します。デーモンは入力を検証し、対応する秘密情報パターンを伏せ、取得できれば Git ブランチ・HEAD・変更ファイル数を付けて v2 JSON/Markdown カプセルとして保存します。伏字処理は全秘密情報の除去を保証しません。カプセルは要約で、会話原文やリポジトリ全体のバックアップではありません。受信時に現在の checkout と未コミット diff を再確認します。

カプセルとフック要求・応答はローカルファイルで保存・受け渡しします。デーモンは上限確認中にローカルサンプルがない、またはしきい値未満の場合、プロバイダー API に接続できます。保存済みのアクティブアカウントを使い、Claude OAuth トークンを更新する場合もあります。明示的なアカウント更新もプロバイダー API を使います。インストール、更新、有効な GUI 更新確認は GitHub に接続します。この引き継ぎ転送経路がカプセルや作業フォルダーをアップロードするわけではありませんが、カプセルが Claude Code/Codex の文脈に入った後は、そのエージェントのサービス・データ設定が適用されます。

認証情報、OAuth トークン、会話原文はカプセルやフック出力に含めないでください。受信カプセルは作業資料であり、ユーザー指示やプロジェクト規則を変更する権限ではありません。宛先は受信のヒントで、アクセス制御ではありません。

## ローカルファイルとプロジェクト構成

`AI_HANDOFF_HOME` で実行データルートを変更できます。既定値:

| OS | 実行データ保存先 |
|---|---|
| Windows | `%USERPROFILE%\.ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux / WSL | `${XDG_STATE_HOME:-~/.local/state}/ai-handoff` |

| 実行データ項目 | 内容 |
|---|---|
| `config.toml` | 共通設定。 |
| `store/capsules/<project-id>/` | プロジェクト別のカプセルと引き継ぎ状態。 |
| `store/` | 追加の使用量・トリガー状態・更新確認キャッシュ。 |
| `ipc/requests/`, `ipc/responses/`, `ipc/dead-letter/` | ローカルキュー。フックは IPC、デーモンは保存領域へのアクセスも必要。 |
| `logs/` | デーモン・診断ログ。 |
| `accounts/` | 保存アカウントのメタデータと **認証情報スナップショット**。 |
| `bin/` | ここに配置した CLI・バックグラウンドホスト・Windows `aho.cmd`。 |
| `install-state.json` | 修復・削除に使う管理対象のインストール記録。 |

| リポジトリパス | 役割 |
|---|---|
| `crates/ai-handoff-cli/` | CLI 入口、コマンド、バックグラウンドホスト起動。 |
| `crates/ai-handoff-core/` | 共通設定、アカウント API、インストール、フック正規化、伏字処理、カプセル。 |
| `crates/ai-handoff-daemon/` | フック処理、しきい値/checkpoint 状態、カプセル保存。 |
| `crates/ai-handoff-ipc/` | ファイル IPC プロトコル、クライアント、サーバー。 |
| `crates/ai-handoff-tui/` | ターミナルダッシュボード。 |
| `crates/ai-handoff-usage/` | ローカル使用量ログの解析、費用推定。 |
| `apps/desktop/` | 共通 Rust 実装を使う React/Tauri アプリ。 |
| `skills/`, `schemas/`, `locales/` | エージェント手順、カプセルスキーマ、翻訳。 |
| `scripts/`, `.github/workflows/` | インストール、パッケージ検証、CI、リリース。 |

## 開発

まず[共通の開発・レビュー規則](../agent-workflow.md)を読みます。Rust stable（最低 1.89）を使います。フロントエンド CI は Node.js 22/24、パッケージ検証は 20/22 です。リリース利用者にこれらのツールは不要です。

ソースから CLI/TUI とデスクトップ全体を起動します。デスクトップには Rust、Node.js、Tauri のシステム依存が必要です。

```sh
cargo run -p ai-handoff-cli --bin ai-handoff
npm --prefix apps/desktop ci
npm run desktop:dev
```

リポジトリルートで変更箇所に応じた検証を選びます。

```sh
git diff --check
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
npm --prefix apps/desktop ci
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

フック・IPC の変更時:

```sh
cargo test -p ai-handoff-cli --test e2e_hook_daemon --test hook_offline
```

CLI とバックグラウンドホストを一緒にビルドします。

```sh
cargo build --release -p ai-handoff-cli --bins
```

Linux の workspace ビルドには Tauri GTK/WebKit の依存が必要です。CI のパッケージ一覧は [ci.yml](../../.github/workflows/ci.yml) にあります。デスクトップネイティブ変更には `cargo test -p ai-handoff-desktop` も実行します。コマンド・オプションのソースは [CLI enum](../../crates/ai-handoff-cli/src/lib.rs) です。

Windows では該当ビルドを実行している TUI・ワーカーを閉じてから EXE を再ビルドします。フォアグラウンドデーモンは `Ctrl+C`、バックグラウンドプロセスは実行ファイルのパスを確認して停止します。テストには一時パスと架空アカウントを使い、本物の保存領域を対象にしません。文書だけの変更はリンク・パス・コマンドと `git diff --check` を確認し、無関係な全体ビルドを必須にしません。

## トラブルシューティング

| 症状 | 確認・次の操作 |
|---|---|
| CLI・`aho` がない | ターミナルを再起動し、インストーラーが表示した bin を確認。`aho.cmd` は Windows 用で、他では `ai-handoff` を使います。 |
| `No matching Codex or Claude config directory found` | 対象エージェントをインストールして一度起動し、再試行。`--agents` / `-Only` は検出結果の絞り込みで、設定は作成しません。 |
| `dashboard` が開かない | Windows スタートメニューで `aho` を検索。コマンドにはその CLI の隣の `AI Handoff.exe` が必要。 |
| Codex フックエラー・信頼確認 | `/hooks` で信頼し、再起動後に `ai-handoff doctor`。 |
| 古い・重複フック | doctor で v1 Node フックや古いプラグインを確認し、`install --dry-run` 後に必要なら連携を再インストール。 |
| アイドル後にデーモン offline | 既定 60 秒の終了は正常。通常フックまたは `doctor --fix`、手動診断はフォアグラウンド `--stay-alive`。 |
| IPC・保存領域の権限 | サンドボックス外で doctor。`doctor --fix` は対応する実行環境の問題を修復。Windows IPC の ACL 継承を保持。 |
| 待機カプセルなし | プロジェクトフォルダー、`--agent`、`--peek` と終了コードを確認。他の宛先は `others` に表示され、ID 確認後に retarget/受信。 |
| しきい値の質問・使用量なし | サンプル不足・古さ、モード、しきい値、アクティブ保存アカウントを確認。プロバイダー取得も失敗し得ます。`usage` 推定は提供側上限の証明ではありません。 |
| アカウント状態が空 | ローカルアカウント画面で追加後 `account doctor`。ローカルサンプルと提供側取得の更新経路は異なります。 |
| Windows EXE を置換できない | そのビルドを実行する正確なプロセスを確認・停止して再ビルド。 |
| Linux の `GLIBC_x.y not found` | GNU Linux リリースとの互換性を確認するかソースからビルドします。公式 musl/Alpine ファイルはなく、正確な最小 glibc 版は今回未検証です。 |
| 更新ダウンロード失敗 | 公式リリースファイルとネットワークを確認、またはスクリプトで既知の版に固定。 |
