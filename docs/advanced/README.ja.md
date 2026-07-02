# AI Handoff 詳細ガイド

[English](README.md) | [한국어](README.ko.md) | **日本語** | [中文](README.zh.md)

この文書では、初心者向け README から意図的に省いた詳細を説明します。

## 目次

- [コマンド詳細](#コマンド詳細)
- [ファイル構成](#ファイル構成)
- [プロジェクト構成](#プロジェクト構成)
- [開発チェック](#開発チェック)
- [トラブルシューティング](#トラブルシューティング)

## コマンド詳細

| コマンド | ターミナル相当 | 詳細 |
|---|---|---|
| `handoff` | `ai-handoff handoff --agent <self>` | 現在のプロジェクトと現在のエージェントに対応する最新の待機中カプセルを取得し、消費済みにします。 |
| `handoff config` | `ai-handoff config list` | 編集可能な config key を表示します。直接編集する場合は `ai-handoff config get <key>` と `ai-handoff config set <key> <value>` を使います。 |
| `handoff doctor` | `ai-handoff doctor` | plugin 状態、hook trust、daemon 接続、IPC、store、重複 hook 問題を確認します。 |
| `handoff checkpoint` | `ai-handoff checkpoint --message "work snapshot"` | 現在の作業からローカルカプセルを作成します。次のエージェントが何のために見る checkpoint なのかを短く書きます。 |

便利なターミナルコマンド:

```sh
ai-handoff
ai-handoff tui
ai-handoff checkpoint --message "backend auth work"
ai-handoff doctor
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff usage
ai-handoff account status
ai-handoff daemon run
ai-handoff autostart status
ai-handoff uninstall --keep-store
```

## ファイル構成

AI Handoff runtime home:

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

重要な runtime entry:

| パス | 目的 |
|---|---|
| `config.toml` | Claude Code、Codex、daemon、TUI、hook が共有する設定です。 |
| `store/` | ローカルカプセル、プロジェクト bucket、handoff 状態を保存します。 |
| `ipc/` | hook と daemon が使うローカル file IPC queue です。Codex はここだけを書ければ十分です。 |
| `logs/` | 有効な場合、daemon と診断ログを保存します。 |
| `accounts/` | ローカルアカウント metadata です。credential を hook や capsule に出してはいけません。 |
| `install-state.json` | installer が書いた内容を記録し、uninstall が管理対象ファイルだけを消せるようにします。 |

## プロジェクト構成

| パス | 目的 |
|---|---|
| `crates/ai-handoff-cli/` | ネイティブ CLI entrypoint とユーザー向けコマンドです。 |
| `crates/ai-handoff-core/` | 共通 config、install、hook event、fingerprint、redaction、capsule logic です。 |
| `crates/ai-handoff-daemon/` | hook request を受け取り capsule を書くローカル daemon です。 |
| `crates/ai-handoff-ipc/` | ファイルベース IPC protocol と client/server helper です。 |
| `crates/ai-handoff-tui/` | ターミナルダッシュボードです。 |
| `crates/ai-handoff-usage/` | ローカル Claude/Codex usage log parser と cost estimator です。 |
| `apps/desktop/` | 任意機能の Tauri desktop dashboard です。 |
| `skills/` | plugin bundle が提供する agent-facing skill です。 |
| `schemas/` | capsule と memory schema ファイルです。 |
| `scripts/` | package validation と release helper script です。 |

## 開発チェック

コミット前に実行:

```sh
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
git diff --check
```

daemon が `target/release/ai-handoff.exe` を使用していないときに release build を実行します。

```sh
cargo build --release -p ai-handoff-cli
```

Windows で build 中に access denied が出る場合は、実行中のローカル daemon を先に止めます。

```powershell
Get-Process ai-handoff | Stop-Process
cargo build --release -p ai-handoff-cli
```

## トラブルシューティング

| 症状 | 確認すること |
|---|---|
| Codex が hook error を表示する | `/hooks` を開き、AI Handoff hooks を trust してから `ai-handoff doctor` を実行します。 |
| hook が code 1 で終了する | 古い v1 Node hook または古い plugin cache を確認します。`ai-handoff install --yes` で再インストールします。 |
| daemon が offline | 1 つのターミナルで `ai-handoff daemon run` を実行し、別のターミナルで `ai-handoff doctor` を実行します。 |
| usage が空 | AI Handoff はローカルログだけを推定します。Claude Code または Codex を先に使ってから `ai-handoff usage` を実行します。 |
| Windows build が exe を置き換えられない | 実行中の `ai-handoff.exe` process を止めてから再度 build します。 |
