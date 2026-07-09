<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

# AI Handoff

[English](../README.md) | [한국어](README.ko.md) | **日本語**

AI Handoff は Claude Code と Codex の間で作業を引き継ぐローカルファーストのツールです。

片方のエージェントが使用量上限に近づくと、現在の目標、ブランチ、変更ファイル、メモ、残作業をローカルのカプセルとして保存します。もう片方のエージェントはそのカプセルを読み、同じ文脈から作業を続けられます。

すべての動作はローカルファイルを優先する設計です。カプセルと hook メッセージはユーザーのコンピューターに残ります。

## 目次

- [要件](#要件)
- [Quick Start](#quick-start)
- [主なコマンド](#主なコマンド)
- [ローカルファイル](#ローカルファイル)
- [使用量の数字](#使用量の数字)
- [プライバシーと安全性](#プライバシーと安全性)
- [詳しいドキュメント](#詳しいドキュメント)

## 要件

必要なもの:

- Claude Code または Codex
- macOS、Linux、Windows、WSL のいずれか
- インストール方法のいずれか: Homebrew、`curl`、PowerShell、Git Bash、WSL

リリースビルドを使う通常ユーザーには Node.js や Rust は不要です。

## Quick Start

### Homebrew CLI

```sh
brew install Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Homebrew デスクトップアプリ

デスクトップダッシュボードも使いたい場合に使います。

```sh
brew install --cask Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Windows (PowerShell)

デフォルトの `latest` は GitHub の "Latest" バッジではなく、最も大きい stable `vX.Y.Z` GitHub Release を選びます。

PowerShell で実行します。CLI をダウンロードしてユーザー PATH に追加し、インストーラーを実行します。

**GUI + TUI** — デスクトップアプリと CLI/TUI をまとめてインストールします:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

**TUI** — CLI/TUI のみをインストールします:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes
```

追加オプションを渡す場合（片方のエージェントのみ、バージョン固定）は、同じ scriptblock 形式を使います。

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
```

再現可能なインストールが必要な場合はリリースを固定します:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Version v2.1.7
```

### Shell Installer

デフォルトの `latest` は GitHub の "Latest" バッジではなく、最も大きい stable `vX.Y.Z` GitHub Release を選びます。

macOS、Linux、WSL、Git Bash で使います。

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

インストール後:

1. Claude Code と Codex を再起動します。
2. Codex で `/hooks` を開きます。
3. AI Handoff hooks を trust します。
4. インストール状態を確認します。

```sh
ai-handoff doctor
```

## 主なコマンド

| コマンド | 役割 | 使うタイミング |
|---|---|---|
| `handoff` | 現在のプロジェクトで待機中の handoff カプセルを取得し、消費済みにします。 | 別のエージェントが残した作業を続けるとき |
| `handoff config` | AI Handoff の共有設定を表示または変更します。 | threshold、mode、language、display 設定を変更したいとき |
| `handoff doctor` | インストール状態、hook、daemon、IPC、カプセル状態を確認します。 | hook 失敗、Codex hook エラー、インストール異常が見えるとき |
| `handoff checkpoint` | 現在の作業を handoff カプセルとして保存します。 | 今すぐ別のエージェントへ作業を渡したいとき |

ターミナルでも同じ操作を実行できます。

```sh
ai-handoff handoff --agent <codex|claude-code>
ai-handoff checkpoint --message "work snapshot"
ai-handoff doctor
ai-handoff config list
```

詳しいコマンド説明: [Advanced Guide](advanced/README.ja.md)

## ローカルファイル

AI Handoff はローカルホームフォルダーを 1 つ作成します。

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

初心者が知るべき項目は 3 つです。

| 項目 | 意味 |
|---|---|
| `config.toml` | Claude Code と Codex が共有する設定です。 |
| `store/` | ローカルカプセルと handoff 履歴です。 |
| `ipc/` | hook と daemon が使うローカルメッセージキューです。 |

プロジェクト全体とランタイムのファイル構成: [Advanced Guide](advanced/README.ja.md#ファイル構成)

## 使用量の数字

`ai-handoff usage` はローカルの Claude Code/Codex ログを読みます。

token と cost はローカルログに基づく推定値です。公式の請求、quota、provider 側の使用量レポートではありません。

## プライバシーと安全性

| トピック | AI Handoff の動作 |
|---|---|
| ローカルファースト設計 | カプセル、設定、IPC メッセージ、使用量推定はユーザーのコンピューターに残ります。 |
| Hook データ | hook はローカルイベントデータをローカル IPC に送ります。作業フォルダーをアップロードしません。 |
| アカウント credential | アカウント credential と OAuth token は hook で使われず、カプセルや hook output に書いてはいけません。 |
| アカウント操作 | アカウント切り替えはエージェントスキルではなく、ローカル CLI/TUI/GUI で行います。 |

## 詳しいドキュメント

- [Advanced Guide](advanced/README.ja.md)
- [English](../README.md)
- [Korean](README.ko.md)

## License

[MIT](../LICENSE)
