<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

# AI Handoff

[English](../README.md) | **한국어** | [日本語](README.ja.md)

AI Handoff는 Claude Code와 Codex 사이에서 작업을 넘겨주는 로컬 우선 도구입니다.

한 에이전트가 사용량 한도에 가까워지면 현재 목표, 브랜치, 변경 파일, 메모, 남은 작업을 로컬 캡슐로 저장합니다. 다른 에이전트는 이 캡슐을 읽고 같은 맥락에서 이어서 작업할 수 있습니다.

모든 동작은 로컬 파일을 우선으로 설계되어 있습니다. 캡슐과 hook 메시지는 사용자 컴퓨터에 남습니다.

## 목차

- [요구 사항](#요구-사항)
- [Quick Start](#quick-start)
- [주요 명령어](#주요-명령어)
- [로컬 파일](#로컬-파일)
- [사용량 숫자](#사용량-숫자)
- [개인정보와 안전](#개인정보와-안전)
- [자세한 문서](#자세한-문서)

## 요구 사항

필요한 것:

- Claude Code 또는 Codex
- macOS, Linux, Windows, WSL 중 하나
- 설치 방식 하나: Homebrew, `curl`, PowerShell, Git Bash, WSL

릴리스 빌드를 사용하는 일반 사용자는 Node.js나 Rust가 필요하지 않습니다.

## Quick Start

### Homebrew CLI

```sh
brew install Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Homebrew 데스크톱 앱

데스크톱 대시보드까지 설치하고 싶을 때 사용합니다.

```sh
brew install --cask Lumisia/ai-handoff/ai-handoff
ai-handoff install --yes
```

### Windows (PowerShell)

기본 `latest`는 GitHub의 "Latest" 배지가 아니라 가장 높은 stable `vX.Y.Z` GitHub Release를 선택합니다.

PowerShell에서 실행합니다. CLI를 내려받아 사용자 PATH에 추가하고 설치 프로그램을 실행합니다.

**GUI + TUI** — 데스크톱 앱과 CLI/TUI를 함께 설치합니다:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

**TUI** — CLI/TUI만 설치합니다:

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes
```

추가 옵션을 넘기려면(한 에이전트만, 버전 고정) 같은 scriptblock 형태를 사용합니다.

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex
```

반복 가능한 설치가 필요하면 릴리즈를 고정하세요:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Version v2.1.5
```

### Shell Installer

기본 `latest`는 GitHub의 "Latest" 배지가 아니라 가장 높은 stable `vX.Y.Z` GitHub Release를 선택합니다.

macOS, Linux, WSL, Git Bash에서 사용합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

설치 후:

1. Claude Code와 Codex를 재시작합니다.
2. Codex에서 `/hooks`를 엽니다.
3. AI Handoff hook을 신뢰 처리합니다.
4. 설치 상태를 확인합니다.

```sh
ai-handoff doctor
```

## 주요 명령어

| 명령어 | 하는 일 | 언제 쓰나 |
|---|---|---|
| `handoff` | 현재 프로젝트에 대기 중인 handoff 캡슐을 받아오고 소비 처리합니다. | 다른 에이전트가 남긴 작업을 이어서 진행해야 할 때 |
| `handoff config` | AI Handoff 공용 설정을 보거나 바꿉니다. | threshold, mode, language, display 설정을 바꾸고 싶을 때 |
| `handoff doctor` | 설치 상태, hook, daemon, IPC, 캡슐 상태를 점검합니다. | hook 실패, Codex hook 오류, 설치 이상이 보일 때 |
| `handoff checkpoint` | 현재 작업을 handoff 캡슐로 저장합니다. | 지금 다른 에이전트에게 작업을 넘기고 싶을 때 |

터미널에서도 같은 작업을 실행할 수 있습니다.

```sh
ai-handoff handoff --agent <codex|claude-code>
ai-handoff checkpoint --message "work snapshot"
ai-handoff doctor
ai-handoff config list
```

자세한 명령어 설명: [Advanced Guide](advanced/README.ko.md)

## 로컬 파일

AI Handoff는 로컬 홈 폴더 하나를 만듭니다.

- Windows: `%USERPROFILE%\.ai-handoff`
- macOS: `~/Library/Application Support/ai-handoff`
- Linux: `${XDG_STATE_HOME:-~/.local/state}/ai-handoff`

초보자가 알아야 할 항목은 3개입니다.

| 항목 | 의미 |
|---|---|
| `config.toml` | Claude Code와 Codex가 함께 쓰는 설정입니다. |
| `store/` | 로컬 캡슐과 handoff 기록입니다. |
| `ipc/` | hook과 daemon이 쓰는 로컬 메시지 큐입니다. |

전체 프로젝트와 런타임 파일 구성: [Advanced Guide](advanced/README.ko.md#파일-구성)

## 사용량 숫자

`ai-handoff usage`는 로컬 Claude Code/Codex 로그를 읽습니다.

토큰과 비용은 로컬 로그 기반 추정치입니다. 공식 청구서, quota, provider 사용량 리포트가 아닙니다.

## 개인정보와 안전

| 주제 | AI Handoff 동작 |
|---|---|
| 로컬 우선 설계 | 캡슐, 설정, IPC 메시지, 사용량 추정치는 사용자 컴퓨터에 남습니다. |
| Hook 데이터 | hook은 로컬 이벤트 데이터를 로컬 IPC로 보냅니다. 작업 폴더를 업로드하지 않습니다. |
| 계정 credential | 계정 credential과 OAuth token은 hook에서 사용하지 않으며, 캡슐이나 hook output에 쓰면 안 됩니다. |
| 계정 작업 | 계정 전환은 에이전트 스킬이 아니라 로컬 CLI/TUI/GUI에서 해야 합니다. |

## 자세한 문서

- [Advanced Guide](advanced/README.ko.md)
- [English](../README.md)
- [Japanese](README.ja.md)

## 라이선스

[MIT](../LICENSE)
