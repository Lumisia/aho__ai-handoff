# AI Handoff 상세 문서

[English](README.md) | **한국어** | [日本語](README.ja.md)

이 문서는 초보자용 README에서 의도적으로 줄인 자세한 내용을 설명합니다.

## 목차

- [명령어 자세히 보기](#명령어-자세히-보기)
- [파일 구성](#파일-구성)
- [프로젝트 구성](#프로젝트-구성)
- [개발 검증](#개발-검증)
- [문제 해결](#문제-해결)

## 명령어 자세히 보기

| 명령어 | 터미널 명령 | 설명 |
|---|---|---|
| `handoff` | `ai-handoff handoff --agent <self>` | 현재 프로젝트와 현재 에이전트에 맞는 최신 대기 캡슐을 받아오고 소비 처리합니다. |
| `handoff config` | `ai-handoff config list` | 수정 가능한 config key를 보여줍니다. 직접 수정할 때는 `ai-handoff config get <key>`와 `ai-handoff config set <key> <value>`를 씁니다. |
| `handoff doctor` | `ai-handoff doctor` | plugin 상태, hook 신뢰 상태, daemon 연결, IPC, store, 중복 hook 문제를 점검합니다. |
| `handoff checkpoint` | `ai-handoff checkpoint --message "work snapshot"` | 현재 작업에서 로컬 캡슐을 만듭니다. 다음 에이전트가 왜 이 checkpoint를 봐야 하는지 짧게 적습니다. |

유용한 터미널 명령:

```sh
ai-handoff
ai-handoff tui
ai-handoff checkpoint --message "backend auth work"
ai-handoff doctor
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff config get daemon.idle_timeout_seconds
ai-handoff config set daemon.idle_timeout_seconds 60
ai-handoff usage
ai-handoff account status
ai-handoff daemon run
ai-handoff daemon run --stay-alive
ai-handoff autostart status
ai-handoff uninstall --keep-store
```

`ai-handoff daemon run`은 요청이 없으면 설정된 idle timeout 뒤에 종료됩니다. 기본값은 60초입니다. 디버깅이나 수동 점검처럼 계속 켜둘 때만 `ai-handoff daemon run --stay-alive`를 사용합니다.

## 파일 구성

AI Handoff runtime home:

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

중요 runtime 항목:

| 경로 | 목적 |
|---|---|
| `config.toml` | Claude Code, Codex, daemon, TUI, hook이 함께 쓰는 설정입니다. |
| `store/` | 로컬 캡슐, 프로젝트 bucket, handoff 상태를 저장합니다. |
| `ipc/` | hook과 daemon이 쓰는 로컬 file IPC queue입니다. Codex는 여기만 쓸 수 있으면 됩니다. |
| `logs/` | 활성화된 경우 daemon과 진단 로그를 저장합니다. |
| `accounts/` | 로컬 계정 metadata입니다. credential은 hook이나 capsule로 내보내면 안 됩니다. |
| `install-state.json` | installer가 쓴 파일을 기록해서 uninstall이 관리 파일만 지우게 합니다. |

## 프로젝트 구성

| 경로 | 목적 |
|---|---|
| `crates/ai-handoff-cli/` | 네이티브 CLI entrypoint와 사용자 명령입니다. |
| `crates/ai-handoff-core/` | 공용 config, install, hook event, fingerprint, redaction, capsule 로직입니다. |
| `crates/ai-handoff-daemon/` | hook 요청을 받고 capsule을 쓰는 로컬 daemon입니다. |
| `crates/ai-handoff-ipc/` | 파일 기반 IPC protocol과 client/server helper입니다. |
| `crates/ai-handoff-tui/` | 터미널 대시보드입니다. |
| `crates/ai-handoff-usage/` | 로컬 Claude/Codex usage log parser와 cost estimator입니다. |
| `apps/desktop/` | 선택 기능인 Tauri desktop dashboard입니다. |
| `skills/` | plugin bundle이 제공하는 agent-facing skill입니다. |
| `schemas/` | capsule schema 파일입니다. |
| `scripts/` | package validation과 release helper script입니다. |

## 개발 검증

커밋 전 실행:

```sh
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
git diff --check
```

daemon이 `target/release/ai-handoff.exe`를 사용 중이 아닐 때 release build를 실행합니다.

```sh
cargo build --release -p ai-handoff-cli
```

Windows에서 build 중 access denied가 나오면 실행 중인 로컬 daemon을 먼저 끕니다.

```powershell
Get-Process ai-handoff | Stop-Process
cargo build --release -p ai-handoff-cli
```

## 문제 해결

| 증상 | 확인할 것 |
|---|---|
| Codex가 hook 오류를 보여줌 | `/hooks`를 열고 AI Handoff hook을 trust한 뒤 `ai-handoff doctor`를 실행합니다. |
| hook이 code 1로 종료됨 | 오래된 v1 Node hook 또는 이전 plugin cache를 확인합니다. `ai-handoff install --yes`로 다시 설치합니다. |
| daemon이 offline | hook이 보통 자동으로 시작합니다. 수동 점검 시에는 한 터미널에서 `ai-handoff daemon run --stay-alive`를 실행하고, 다른 터미널에서 `ai-handoff doctor`를 실행합니다. |
| usage가 비어 있음 | AI Handoff는 로컬 로그만 추정합니다. Claude Code나 Codex를 먼저 사용한 뒤 `ai-handoff usage`를 실행합니다. |
| Windows build가 exe를 교체하지 못함 | 실행 중인 `ai-handoff.exe` 프로세스를 끄고 다시 build합니다. |
