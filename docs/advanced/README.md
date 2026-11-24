# AI Handoff 상세 문서

**한국어** | [English](README.en.md) | [日本語](README.ja.md) · [간단한 시작 방법](../../README.md)

설치 선택지, 전체 CLI 명령과 옵션, 대시보드 조작법, 개발 방법을 설명합니다. 에이전트 채팅의 스킬 목록에서 AI Handoff의 `handoff-checkpoint`로 저장하고 `handoff`로 이어받을 수 있습니다.

- [설치 옵션](#설치-옵션)
- [명령어 참조](#명령어-참조)
- [TUI와 데스크톱](#tui와-데스크톱)
- [캡슐과 개인정보](#캡슐과-개인정보)
- [로컬 파일과 프로젝트 구성](#로컬-파일과-프로젝트-구성)
- [개발](#개발)
- [문제 해결](#문제-해결)

## 설치 옵션

Windows, macOS, Linux, WSL에서 Claude Code, Codex 또는 둘 다 사용합니다. 연동 설치 전에 선택한 에이전트를 설치하고 한 번 실행해 설정 폴더를 만듭니다. CLI 릴리스는 x86_64와 ARM64를 지원하며 실행에는 Node.js나 Rust가 필요하지 않습니다. 현재 배포 워크플로의 GUI 설치기는 Windows용입니다.

### 공식 릴리스에서 직접 다운로드 · 권장

**[공식 릴리스 열기](https://github.com/Lumisia/aho__ai-handoff/releases)** → 릴리스의 **Assets**를 펼쳐 아래 파일을 선택합니다. `Source code`가 아니라 설치기 또는 CLI 압축 파일을 고릅니다. `x86_64`는 x64, `aarch64`는 ARM64이며 Apple Silicon은 ARM64입니다.

| 운영체제·CPU | 배포 파일 | 구성 |
|---|---|---|
| Windows x64 · 권장 | `ai-handoff-gui-windows-x86_64-setup.exe` | GUI + CLI/TUI |
| Windows ARM64 · 권장 | `ai-handoff-gui-windows-aarch64-setup.exe` | GUI + CLI/TUI |
| Windows x64 · CLI/TUI만 | `ai-handoff-cli-windows-x86_64.zip` | CLI + 호스트 |
| Windows ARM64 · CLI/TUI만 | `ai-handoff-cli-windows-aarch64.zip` | CLI + 호스트 |
| macOS · Apple Silicon | `ai-handoff-cli-darwin-aarch64.tar.gz` | CLI + 호스트 |
| macOS · Intel | `ai-handoff-cli-darwin-x86_64.tar.gz` | CLI + 호스트 |
| Linux x64 | `ai-handoff-cli-linux-x86_64.tar.gz` | CLI + 호스트 |
| Linux ARM64 | `ai-handoff-cli-linux-aarch64.tar.gz` | CLI + 호스트 |

WSL에서는 Linux용 파일을 선택합니다. Windows GUI는 Windows에서 설치합니다. 선택한 파일과 이름이 같은 `.sha256` 파일도 내려받아 Windows의 `Get-FileHash -Algorithm SHA256`, macOS의 `shasum -a 256`, Linux의 `sha256sum`으로 계산한 값과 대조합니다.

**Windows GUI:** 이름이 `-setup.exe`로 끝나는 설치기를 실행하고 마법사를 완료합니다. CLI/TUI와 백그라운드 호스트가 포함되어 있으며 설치기가 에이전트 연동도 등록합니다. 설치 후 시작 메뉴에서 **aho** 또는 **AI Handoff**로 GUI를 엽니다. 서명·SmartScreen 안내는 아래 Windows 절을 참고합니다.

**CLI 압축 파일을 수동 설치하는 경우:**

1. 압축을 계속 보관할 폴더에 풉니다. `ai-handoff`와 `ai-handoff-host`를 같은 폴더에 두고 이름을 바꾸지 않습니다. Windows에서는 각각 `.exe`가 붙습니다. 호스트는 필요할 때 데몬을 시작하는 프로그램입니다.
2. Windows에서는 압축을 푼 폴더에서 PowerShell을 열어 연동을 등록합니다.

```powershell
.\ai-handoff.exe install --yes
```

3. macOS·Linux에서는 압축을 푼 폴더에서 터미널을 열어 실행 권한을 확인하고 연동을 등록합니다.

```sh
chmod +x ./ai-handoff ./ai-handoff-host
./ai-handoff install --yes
```

4. 압축을 푼 폴더를 운영체제의 사용자 **PATH**에 추가하고 터미널을 다시 엽니다. PATH는 터미널이 명령을 찾는 폴더 목록입니다. 이제 `ai-handoff`로 TUI를 열 수 있고, Windows 설치는 단축 명령 `aho`도 제공합니다.

설치 후 이 파일들을 옮겼다면 새 위치에서 `install --yes`를 다시 실행해 훅 경로를 갱신합니다. 에이전트 재시작·Codex 훅 신뢰·`doctor` 확인은 아래 공통 설치 후 단계를 따릅니다. 다음 스크립트는 다운로드와 등록을 대신 수행하는 대안입니다.

### Homebrew

이전에 안내한 Homebrew tap의 공개 원본을 확인할 수 없어 이 문서에서는 GitHub 릴리스 설치를 안내합니다. macOS 데스크톱 cask의 제공도 확인되지 않았습니다.

### 대안: Windows PowerShell 설치 스크립트

CLI/TUI와 데스크톱 앱을 함께 설치합니다.

```powershell
Set-ExecutionPolicy Bypass -Scope Process -Force; & ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -WithGui
```

CLI/TUI만 필요하면 `-WithGui`를 뺍니다. 스크립트는 릴리스 파일을 다운로드·검증하고 CLI와 백그라운드 호스트를 `%USERPROFILE%\.ai-handoff\bin` 또는 `$env:AI_HANDOFF_HOME\bin`에 설치한 뒤 사용자 PATH를 추가하고 `ai-handoff install`을 실행합니다.

| 스크립트 옵션 | 효과 |
|---|---|
| `-Yes` | CLI 연동 질문을 생략합니다. `-WithGui`와 함께 사용하면 GUI 설치기도 무인 모드로 실행합니다. |
| `-Only codex` / `-Only claude` / `-Only claude-code` | CLI 연동 설치 대상을 하나로 지정합니다. 생략하면 설치된 에이전트를 감지합니다. |
| `-Version vX.Y.Z` | 버전을 고정합니다. 기본값 `latest`는 GitHub의 “Latest” 표시가 아니라 가장 높은 안정 `vX.Y.Z` 릴리스를 선택합니다. |
| `-WithGui` | CLI/TUI가 포함된 Windows GUI 설치기도 다운로드하고 실행합니다. |
| `-DryRun` | CLI 연동 단계에 `--dry-run`을 전달합니다. 다운로드, 바이너리 설치, PATH 변경, 선택한 GUI 설치는 그대로 진행됩니다. |

버전을 고정하고 Codex만 연동하는 예:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.ps1))) -Yes -Only codex -Version v2.2.8
```

GUI 설치기는 자체 연동 설정도 수행하므로 `-Only`가 별도 GUI 설치 단계까지 제한하지는 않습니다. 다운로드나 바이너리 교체 없는 설정 미리보기는 이미 설치된 `ai-handoff install --dry-run`을 사용합니다.

Windows 설치기는 현재 서명되지 않았습니다. SmartScreen에 **Windows의 PC 보호 / 알 수 없는 게시자**가 표시되면 [공식 릴리스](https://github.com/Lumisia/aho__ai-handoff/releases) 파일인지 확인하고 해당 `.sha256`과 대조한 뒤 **추가 정보 → 실행**을 선택합니다. SmartScreen 자체를 끄지 않습니다.

### 대안: 셸 설치 스크립트

macOS, Linux, WSL, Git Bash에서 사용합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/Lumisia/aho__ai-handoff/master/scripts/install.sh | sh -s -- --yes
```

| 스크립트 옵션 | 효과 |
|---|---|
| `--yes`, `-y` | 연동 설치 질문을 생략합니다. |
| `--only codex` / `--only claude` / `--only claude-code` | 연동할 에이전트를 하나로 지정합니다. |
| `--version vX.Y.Z` | 버전을 고정합니다. `latest`는 가장 높은 안정 릴리스입니다. |
| `--dry-run` | 연동 설정 변경만 미리 봅니다. 다운로드와 바이너리 교체는 진행됩니다. |
| `--with-gui` | 현재 미지원 안내만 출력합니다. 이 스크립트는 CLI/TUI만 설치합니다. |
| `--help`, `-h` | 도움말을 표시하고 설치 전에 종료합니다. |

옵션은 `sh -s --` 뒤에 붙입니다. 예: `--yes --only codex --version v2.2.8`. 바이너리는 `${AI_HANDOFF_HOME:-$HOME/.ai-handoff}/bin`에 설치되며 명령을 찾지 못하면 출력된 경로를 PATH에 추가합니다. `AI_HANDOFF_HOME`을 지정하지 않으면 이 다운로드 경로와 아래 운영체제별 실행 데이터 경로가 다를 수 있습니다.

설치 후 에이전트를 다시 시작합니다. Codex에서는 `/hooks`를 열어 AI Handoff 훅을 신뢰하고 `ai-handoff doctor`로 연동 상태를 확인합니다.

## 명령어 참조

프로젝트별 명령은 해당 프로젝트 폴더에서 실행합니다. 프로젝트 식별값에 따라 캡슐 저장 영역이 정해집니다. `ai-handoff --help`, `ai-handoff <명령> --help`, `-h`로 설치된 CLI의 도움말을 봅니다.

### 에이전트 스킬

채팅의 스킬 목록에서 선택하며 실제 CLI는 에이전트가 실행합니다. 기본 플러그인 설치에서는 다음 이름으로 노출됩니다. 채팅에서 선택하는 방식과 터미널 명령은 구분합니다.

| 플러그인 스킬 | 용도·CLI 대응 |
|---|---|
| `ai-handoff:handoff` | 현재 에이전트 대상 또는 대상 미지정인 최신 대기 캡슐을 받아 이어갑니다. CLI: `ai-handoff handoff --agent <self>`. |
| `ai-handoff:handoff-checkpoint` | 현재 작업을 작성해 즉시 저장합니다. 선택 인수 `md` / `json`과 요약을 받을 수 있습니다. CLI 작성 규약: `ai-handoff checkpoint guidance --agent <self> --json`. |
| `ai-handoff:handoff-config` | 공통 설정을 확인·변경합니다. CLI: `ai-handoff config list/get/set`. |
| `ai-handoff:handoff-doctor` | 설치·훅·데몬·IPC·캡슐을 점검합니다. CLI: `ai-handoff doctor`. |

짧은 `/handoff checkpoint` 같은 표기는 모든 호스트에서 공통인 실제 명령을 보장하지 않습니다. 스킬 목록의 이름을 선택하거나 위 CLI를 사용합니다. Checkpoint의 짧은 요약 인수만으로 완료·남은 일·위험·다음 프롬프트가 채워지지는 않습니다.

### 전체 최상위 명령

| 명령 | 용도 |
|---|---|
| `ai-handoff` | 터미널 대시보드. Windows에서는 단축 명령 `aho`도 사용할 수 있습니다. 리다이렉트 시 정적 상태 요약을 출력합니다. |
| `ai-handoff dashboard` | 실행 중인 CLI 옆에 `AI Handoff.exe`가 있을 때만 GUI를 실행합니다. |
| `ai-handoff checkpoint [md\|json\|guidance]` | 캡슐 저장 또는 작성 규약 출력. |
| `ai-handoff handoff` | 대기 캡슐을 받아 소비 상태로 표시합니다. |
| `ai-handoff retarget <capsule-id>` | 대기 캡슐 대상을 바꾸거나 대상 미지정으로 만듭니다. |
| `ai-handoff config <get\|set\|list>` | 공통 설정 읽기·수정. |
| `ai-handoff doctor` | 연동·실행 환경 점검. |
| `ai-handoff account <list\|status\|doctor>` | 저장 계정·로컬 상태·계정 구성 점검. |
| `ai-handoff usage` | 로컬 로그 기반 토큰·비용 추정. |
| `ai-handoff hook <event>` | stdin JSON → 로컬 IPC → 훅 응답 JSON 어댑터. |
| `ai-handoff daemon <run\|status>` | 로컬 작업자 실행·연결 확인. |
| `ai-handoff install` | 에이전트 연동 설치·복구. |
| `ai-handoff update` | 공식 설치기로 최신 안정 CLI/TUI 설치. |
| `ai-handoff uninstall` | 관리 연동 제거. 로컬 데이터는 기본 보존하며 바이너리 제거는 실행 위치에 따라 달라집니다. |
| `ai-handoff autostart <on\|off\|status>` | Windows 로그인 자동 실행과 설정 관리. |
| `ai-handoff statusline` | Claude 상태줄 JSON을 stdin으로 받아 한도 표본을 로컬 기록하고 상태줄 출력. |

### Checkpoint

| 인수·옵션 | 의미 |
|---|---|
| `md`, `json` | 작성·저장 형식을 명시합니다. 생략하면 입력 형식을 감지하고 설정된 저장 형식을 사용합니다. |
| `guidance` | 에이전트, 언어, 형식, 항목 수 제한, 입력 템플릿, 정확한 저장 명령을 JSON으로 출력합니다. 저장은 하지 않습니다. |
| `--agent <id>` | 작성자. 입력 `agent`보다 우선하며 둘 다 없으면 `codex`입니다. 별칭을 정규화하고 다른 ID도 허용합니다. |
| `--target <id>` | 받을 에이전트. 입력 `target`보다 우선하며 둘 다 없으면 누구나 받을 수 있습니다. |
| `--message <text>` | 입력에 목표가 없을 때 사용하는 설명·목표. 작업 요약을 생성하지는 않습니다. |
| `--file <path>` | UTF-8 JSON/Markdown 초안 파일. PowerShell·POSIX에서 권장하며 생략하면 EOF까지 stdin을 읽습니다. |
| `--format <md\|json>` | 설정된 형식을 덮어씁니다. 주로 `guidance`에 사용하며 위치 형식과 다르면 오류입니다. |
| `--json` | `guidance` 전용 압축 JSON. 생략한 guidance도 들여쓴 JSON입니다. |
| `--episode <id>` | 임계값 질문에서 전달된 진행 건에 저장을 연결합니다. 전달받은 ID만 쓰며 일반 수동 저장에는 필요 없습니다. |

먼저 guidance를 확인하고 반환된 언어·템플릿으로 작성한 뒤 반환된 명령으로 저장합니다.

```sh
ai-handoff checkpoint guidance --agent claude-code --format json --json
```

JSON 초안 예:

```json
{
  "goal": "설정 변경 마무리",
  "done": ["파서 수정 완료"],
  "remaining": ["GUI 호출부 확인"],
  "risks": ["GUI 검증은 아직 실행하지 않음"],
  "next_prompt": "GUI 호출부를 확인하고 관련 검증을 실행한다"
}
```

`checkpoint.json`으로 저장하고 받을 에이전트를 지정하는 예:

```sh
ai-handoff checkpoint json --agent claude-code --target codex --file checkpoint.json
```

`--message`만으로 대화나 완료 작업을 복원하지는 않습니다. 스킬이 현재 맥락으로 초안을 채웁니다. 인증정보와 원문 대화는 넣지 않습니다.

### Handoff와 retarget

| 인수·옵션 | 의미 |
|---|---|
| `handoff --agent <id>` | 받을 ID. 기본 `codex`; 대상 미지정 또는 해당 ID 대상 캡슐을 찾습니다. |
| `handoff --peek` | 소비 상태를 바꾸지 않고 최신 내 대상·대상 미지정 캡슐을 미리 봅니다. |
| `handoff --id <capsule-id>` | 그 대기 캡슐만 소비합니다. 다른 대상이어도 가져오며 `--force`보다 우선합니다. 소비용 옵션입니다. |
| `handoff --force` | 다른 대상도 후보에 넣고 최신 수신 가능 캡슐을 소비합니다. 대상 무시 이력이 남습니다. 여러 캡슐이 있으면 확인 후 `--id`를 권장합니다. 소비용입니다. |
| `retarget <capsule-id> --to <id>` | 대기 캡슐 대상을 바꿉니다. `--to`를 빼면 대상 미지정으로 만듭니다. |

```sh
ai-handoff handoff --agent codex --peek
ai-handoff handoff --agent codex
```

ID를 확인한 뒤 잘못된 대상을 고치는 예:

```sh
ai-handoff retarget <capsule-id> --to codex
```

소비 응답에는 `hookSpecificOutput.additionalContext`, `consumed: true`가 있습니다. 종료 코드가 0일 때 `{}`는 맞는 대기 캡슐이 없음, `pending: false`와 `others`는 다른 대상의 목록입니다. 실패해도 `{}`가 나올 수 있으므로 종료 코드를 확인합니다. `--peek`는 `preview`를 반환하며 `--id` / `--force`는 적용하지 않습니다. 특정 ID 미리보기로 간주하지 않습니다. 세션 시작은 대기 알림만 하며 실제 수신은 명시적 handoff입니다. 다른 에이전트를 실행·전환하지 않고 실행한 에이전트에 저장 맥락을 전달합니다.

### 설정과 진단

| 명령·옵션 | 의미 |
|---|---|
| `config list` | CLI에서 수정 가능한 키·적용값 나열. |
| `config get <key>` | 미설정 키의 기본값을 포함한 적용값 읽기. |
| `config set <key> <value>` | 지원 설정 저장. 관련 없는 TOML 키는 보존합니다. |
| `doctor --json` | 기계가 읽을 수 있는 진단 결과. |
| `doctor --fix` | 실행 디렉터리 복구·데몬 시작을 시도하고 남은 수동 조치를 안내합니다. `--json`과 함께 쓸 수 있습니다. |

```sh
ai-handoff config list
ai-handoff config get triggers.five_hour.mode
ai-handoff config set triggers.five_hour.threshold_percent 80
ai-handoff config set triggers.five_hour.mode ask
ai-handoff config set capsule.language ko
ai-handoff doctor --json
```

5시간 한도 모드는 `off`, `ask`, `auto`이며 기본은 활성화·80%·`ask`입니다. `ask`는 저장 여부를 묻고 `auto`는 묻지 않고 checkpoint 작성을 요청하며 `off`는 트리거를 끕니다. 저장 후 원래 작업을 이어갑니다. 에이전트가 초안을 작성·저장해야 하므로 대화 백업이나 자동 에이전트 전환과는 다릅니다.

훅이 활성화된 대화형 CLI와 로컬 데스크톱에서는 `ask`가 세션별로 같은 5시간 한도 안에서 한 번만 **예 / 아니오 / 기타**를 표시합니다. 기타에는 원하는 지시를 입력합니다. 선택 창을 지원하지 않는 환경에서는 채팅으로 묻습니다. 답하거나 창을 닫으면 제공자의 초기화 시각이 지난 뒤 새 한도에서 사용량이 다시 임계치에 도달할 때 묻습니다. 초기화 시각을 알 수 없으면 첫 질문부터 5시간을 기준으로 합니다.

다른 CLI 설정에는 `daemon.idle_timeout_seconds`, `autostart.enabled`, `statusline.show`, UI `language`, 캡슐 형식·항목 수 제한, 소진 속도 설정, 터미널 `theme.*` 색상이 있습니다. 정확한 키는 `config list`로 확인하며 GUI 전용 설정은 데스크톱 설정 화면에서 수정합니다. 실제 자동 실행 등록은 `autostart on/off`를 사용합니다. boolean만 수정하는 것과는 별개입니다.

### 계정과 사용량

| 명령·옵션 | 의미 |
|---|---|
| `account list [--json]` | 두 에이전트의 저장 계정·활성 표시. |
| `account status [--json]` | 추가된 활성 계정과 로컬 표본의 플랜·한도. |
| `account status --fetch [--json]` | 활성 저장 계정의 인증정보로 Claude 사용량도 조회합니다. OAuth 갱신 가능. Codex 상태는 로컬 그대로입니다. |
| `account doctor [--json]` | 로그인·저장 계정·PATH의 공식 CLI·실행 중 에이전트 점검. |
| `usage --group-by <day\|model\|project\|source>` | 사용량 분류. 생략하면 전체·에이전트별 요약입니다. |
| `usage --source <claude\|codex>` | 하나로 제한합니다. 여기서는 `claude-code`가 아니라 `claude`입니다. |
| `usage --since <YYYY-MM-DD>` | 해당 날짜부터 포함합니다. |
| `usage --json` | JSON 출력. 위 필터와 조합 가능합니다. |

```sh
ai-handoff account list
ai-handoff account status --json
ai-handoff account doctor
ai-handoff usage --group-by model --source codex --since 2026-01-01 --json
```

계정 추가·전환·삭제는 로컬 TUI/GUI의 계정 화면에서 합니다. 선택한 계정으로 에이전트 CLI를 실행하려면 TUI의 계정 상세에서 `l`을 누릅니다. 저장·현재 로그인 인증정보를 바꿀 수 있으며 스킬은 계정을 전환하지 않습니다. 로컬 로그 기반 `usage` 토큰·비용 추정과 제공자의 계정 한도는 별개입니다. 추정치는 공식 청구서·할당량이 아닙니다.

### 훅, 데몬, 상태줄

| 명령·옵션 | 의미 |
|---|---|
| `hook <session-start\|user-prompt\|post-tool-use\|stop>` | 이벤트 처리. 보통 설치된 훅이 stdin JSON과 함께 호출합니다. |
| `hook ... --agent <codex\|claude-code>` | 연동 에이전트. 기본 `codex`; 캡슐 명령과 달리 두 값만 허용합니다. |
| `daemon run` | 포그라운드 실행. 요청이 없으면 기본 60초 뒤 종료합니다. |
| `daemon run --stay-alive` | 수동 진단용으로 멈출 때까지 계속 실행합니다. |
| `daemon <run\|status> --home <path>` | 이번 실행의 `AI_HANDOFF_HOME` 지정. 호스트 관리·격리 환경용입니다. |
| `daemon status` | `reachable`/`unreachable` 출력. 전체 건강 점검은 아닙니다. |
| `statusline` | Claude가 stdin을 전달합니다. 전용 옵션은 없으며 기록된 기존 상태줄 명령도 연결합니다. |

훅이 필요할 때 데몬을 시작하며 유휴 종료는 정상입니다. Windows에서는 설치된 백그라운드 호스트가 샌드박스 밖에서 작업자를 시작합니다. 로그인 자동 실행은 별개이며 기본 꺼짐입니다.

수동 진단은 한 터미널에서 `ai-handoff daemon run --stay-alive`, 다른 터미널에서 `ai-handoff doctor`를 실행합니다. 확인 후 포그라운드 작업자는 `Ctrl+C`로 종료합니다. 훅 JSON은 stdout, 진단은 stderr이며 시험 입력에 인증정보를 넣지 않습니다.

### 설치, 업데이트, 제거, 자동 실행

| 명령·옵션 | 효과 |
|---|---|
| `install --dry-run` | 연동 설정 변경을 적용하지 않고 미리 봅니다. |
| `install --yes` | 연동 질문 없이 적용합니다. |
| `install --agents <ids>` | `codex,claude-code`처럼 쉼표 구분. `claude`도 허용하며 생략하면 감지합니다. |
| `install --no-plugin` | 기본 플러그인 대신 기존 직접 훅 방식. 호환성·진단용입니다. |
| `update` | 최신 안정 CLI/TUI용 공식 설치기를 다운로드·실행합니다. 버전 고정은 설치 스크립트를 사용합니다. |
| `update --gui` | Windows GUI도 업데이트. 다른 플랫폼에서는 CLI/TUI만 업데이트합니다. |
| `uninstall` | 관리 연동·실행 등록·런처 제거. 저장소·로그는 기본 보존하며 바이너리 제거는 위치에 따라 달라집니다. |
| `uninstall --keep-store` | 데이터 보존 명시. 재설치에 유용합니다. |
| `uninstall --purge-store` | 확인 후 `store/`, `logs/`, `ipc/`도 삭제합니다. |
| `uninstall --gui` | Windows GUI도 제거하며 CLI/TUI 제거를 포함합니다. |
| `uninstall --all` | GUI/CLI/TUI 제거와 저장소·로그·IPC 삭제를 함께 요청합니다. |
| `uninstall --yes` | 데이터 삭제 확인 생략. 먼저 선택 옵션의 효과를 확인합니다. |
| `autostart on` | Windows 사용자 로그인 작업 등록·설정 활성화. 실패 시 HKCU Run 대체 등록. |
| `autostart off` | 작업·Run 등록 제거와 설정 비활성화. |
| `autostart status` | 설정값과 실제 작업·Run 등록 확인. |

`--keep-store`는 `--purge-store`, `--all`과 함께 쓸 수 없습니다. 삭제 대상은 캡슐·인계 기록을 포함한 저장소·로그·IPC이며 `accounts/`와 `config.toml`까지 지우지는 않습니다. 삭제 옵션 없는 uninstall도 제거 작업이며 미리보기가 아닙니다. CLI 자기 삭제는 실행 데이터 루트의 `bin/`에서 실행할 때만 시도합니다. macOS/Linux 셸 설치기의 별도 기본 bin에는 바이너리가 남을 수 있으므로 설치기가 출력한 위치를 확인한 뒤 수동 정리합니다. 자동 실행 등록은 현재 Windows용이며 다른 플랫폼에서도 훅이 필요할 때 데몬을 시작할 수 있습니다.

플랫폼 한계: 현재 제거 경로는 Windows 작업·레지스트리 명령을 호출합니다. 소스상 Unix에서 `schtasks`가 없으면 연동 제거 전에 실패할 가능성이 있으며 이번 작업에서 Unix 실기 재현은 하지 않았습니다. 해당 오류는 제거 성공으로 간주하지 말고 원인을 확인·보고한 뒤 수동 정리를 검토합니다.

## TUI와 데스크톱

터미널에서 `aho` 또는 `ai-handoff`로 TUI를 엽니다. `aho`는 Windows 설치가 제공하는 `aho.cmd` 단축 명령이며, 다른 환경에서는 `ai-handoff`를 사용합니다. 대체 명령으로 `ai-handoff tui`도 사용할 수 있습니다. Windows GUI는 시작 메뉴에서 **aho**를 검색해 실행합니다. `ai-handoff dashboard`는 해당 CLI 옆의 `AI Handoff.exe`만 찾으므로 별도 폴더에 설치한 GUI는 시작 메뉴로 열리더라도 이 명령이 실패할 수 있습니다.

### TUI 키

해당 영역에 초점이 있을 때 적용되며, 입력 중인 필드는 저장·취소할 때까지 키 입력을 처리합니다.

| 화면 | 키 |
|---|---|
| 공통 탭 | `Tab` / `Shift+Tab` 또는 `1`–`6`: 개요, 캡슐, 사용량, 계정, 연동, 설정. |
| 탭 표시줄 | `←/→`: 탭 선택. `↓` / `Enter` / `Space`: 내용 진입. |
| 뒤로·종료 | `q` / `Esc`: 내용·하위 화면에서 나옴. 탭 표시줄의 종료 질문 후 다시 누르면 종료. |
| 캡슐 트리 | `↑/↓` 또는 `k/j`: 선택. `→` / `Enter` / `Space`: 펼치기·열기. `←`: 접기. `r`: 다시 읽기. |
| 캡슐 상세 | `↑/↓` 또는 `k/j`: 필드. `e` / `Enter`: 수정. 수정 중 `Enter`: 저장, `Esc`: 취소. `s`: 생명주기 상태 변경. `d` 후 `d` / `y`: 삭제. `←`: 트리로. |
| 사용량 | `g`: 분류 변경. `→` / `Enter` / `Space`: 상세. `←`: 차트. |
| 계정 | `↑/↓` 또는 `k/j`: 선택. 트리 `a` / `+`: 추가. `Enter` / `→` / `Space`: 상세. 상세 `s`: 전환, `l`: 실행, `a`: 추가, `d` 후 `d` / `y`: 삭제, `←`: 트리. `r`: 사용량 새로고침. |
| 연동 | `d`: 진단. `l`: 로그. `r`: 복구. `Enter` / `Space`: 상세. 복구 `↑/↓` 또는 `k/j`: 선택, `Enter` / `y`: 실행·확인, `n`: 확인 취소. |
| 설정 | `↑/↓` 또는 `k/j`: 선택. 분류 `Enter` / `→` / `Space`: 상세. 상세 `←` / `-`: 이전 값, `→` / `+` / `=`: 다음 값, `Enter` / `Space`: 전환·수정, `r`: 초기화. `/`: 검색. |

생명주기 수정은 저장 상태를 바꾸며 에이전트 대화에 내용을 주입하지 않습니다. 삭제는 캡슐을 지웁니다. 계정 새로고침은 제공자 API를 호출할 수 있습니다. 변경 전 화면의 확인·하단 키 안내를 확인합니다.

### 데스크톱 조작

사이드바, 계정·캡슐 메뉴, 설정 화면을 사용합니다. `Esc`는 설정 오버레이·열린 캡슐 메뉴를 닫고 `Enter`는 지원하는 설정 입력을 저장합니다. TUI의 숫자 탭 단축키는 없습니다. 창을 닫으면 기본 종료합니다. 계속 실행하려면 설정의 **트레이로 닫기**를 켜며 트레이 메뉴에서 다시 열거나 종료합니다. 업데이트 확인은 알림만 표시하고 자동 설치하지 않습니다.

## 캡슐과 개인정보

에이전트가 목표·완료·남은 일·위험·파일 메모·다음 프롬프트를 작성합니다. 데몬은 입력을 검증하고 지원하는 비밀정보 패턴을 가린 뒤 가능한 경우 Git 브랜치·HEAD·변경 파일 수를 붙여 v2 JSON/Markdown 캡슐을 저장합니다. 가림 처리는 모든 비밀정보 제거를 보증하지 않습니다. 캡슐은 요약이며 원문 대화·저장소 전체 백업이 아닙니다. 받을 때 현재 checkout과 미커밋 diff를 다시 확인합니다.

캡슐과 훅 요청·응답은 로컬 파일로 저장·전달합니다. 데몬은 임계값 확인 중 로컬 표본이 없거나 임계값 미만이면 제공자 API에 접속할 수 있습니다. 활성 저장 계정을 사용하며 Claude OAuth 토큰을 갱신할 수도 있습니다. 명시적 계정 새로고침도 제공자 API를 사용합니다. 설치·업데이트·활성화된 GUI 업데이트 확인은 GitHub에 접속합니다. 이 인계 전송 경로가 캡슐이나 작업 폴더를 업로드하지는 않지만, 캡슐이 Claude Code/Codex 맥락에 들어간 뒤에는 해당 에이전트의 서비스·데이터 설정이 적용됩니다.

인증정보·OAuth 토큰·원문 대화는 캡슐과 훅 출력에 넣지 않습니다. 수신 캡슐은 작업 자료이며 사용자 지시·프로젝트 규칙을 바꾸는 권한이 아닙니다. 대상 지정은 수신 힌트이며 접근 제어가 아닙니다.

## 로컬 파일과 프로젝트 구성

`AI_HANDOFF_HOME`으로 실행 데이터 루트를 바꿉니다. 기본값:

| 운영체제 | 실행 데이터 경로 |
|---|---|
| Windows | `%USERPROFILE%\.ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux / WSL | `${XDG_STATE_HOME:-~/.local/state}/ai-handoff` |

| 실행 데이터 항목 | 내용 |
|---|---|
| `config.toml` | 공통 설정. |
| `store/capsules/<project-id>/` | 프로젝트별 캡슐·인계 상태. |
| `store/` | 추가 사용량·트리거 상태·업데이트 확인 캐시. |
| `ipc/requests/`, `ipc/responses/`, `ipc/dead-letter/` | 로컬 큐. 훅은 IPC 접근, 데몬은 저장소 접근도 필요합니다. |
| `logs/` | 데몬·진단 로그. |
| `accounts/` | 저장 계정 메타데이터와 **인증정보 스냅샷**. |
| `bin/` | 이 위치에 설치한 CLI·백그라운드 호스트·Windows `aho.cmd`. |
| `install-state.json` | 복구·제거에 사용하는 설치 관리 기록. |

| 저장소 경로 | 역할 |
|---|---|
| `crates/ai-handoff-cli/` | CLI 진입점·명령·백그라운드 호스트 런처. |
| `crates/ai-handoff-core/` | 공통 설정·계정 API·설치·훅 정규화·비밀정보 가림·캡슐. |
| `crates/ai-handoff-daemon/` | 훅 처리·임계값/checkpoint 상태·캡슐 저장. |
| `crates/ai-handoff-ipc/` | 파일 IPC 프로토콜·클라이언트·서버. |
| `crates/ai-handoff-tui/` | 터미널 대시보드. |
| `crates/ai-handoff-usage/` | 로컬 사용량 로그 분석·비용 추정. |
| `apps/desktop/` | 공통 Rust 구현을 사용하는 React/Tauri 앱. |
| `skills/`, `schemas/`, `locales/` | 에이전트 절차·캡슐 스키마·번역. |
| `scripts/`, `.github/workflows/` | 설치·패키지 검증·CI·릴리스. |

## 개발

먼저 [공통 개발·검토 규칙](../agent-workflow.md)을 읽습니다. Rust stable(최소 1.89)을 사용합니다. 프런트엔드 CI는 Node.js 22/24, 패키지 검증은 20/22입니다. 릴리스 사용자에게는 이 도구가 필요하지 않습니다.

소스에서 CLI/TUI와 전체 데스크톱을 실행합니다. 데스크톱은 Rust·Node.js·Tauri 시스템 의존성이 필요합니다.

```sh
cargo run -p ai-handoff-cli --bin ai-handoff
npm --prefix apps/desktop ci
npm run desktop:dev
```

저장소 루트에서 변경 영역에 맞는 검사를 선택합니다.

```sh
git diff --check
cargo fmt --all -- --check
cargo test --workspace
npm run validate:package
npm --prefix apps/desktop ci
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

훅·IPC 변경 시:

```sh
cargo test -p ai-handoff-cli --test e2e_hook_daemon --test hook_offline
```

CLI와 백그라운드 호스트를 함께 빌드합니다.

```sh
cargo build --release -p ai-handoff-cli --bins
```

Linux workspace 빌드는 Tauri GTK/WebKit 의존성이 필요합니다. CI 패키지 목록은 [ci.yml](../../.github/workflows/ci.yml)에 있습니다. 데스크톱 네이티브 변경에는 `cargo test -p ai-handoff-desktop`도 실행합니다. 명령·옵션 소스는 [CLI enum](../../crates/ai-handoff-cli/src/lib.rs)입니다.

Windows에서는 빌드 산출물을 실행하는 TUI·작업자를 종료한 뒤 해당 EXE를 다시 빌드합니다. 포그라운드 데몬은 `Ctrl+C`, 백그라운드 프로세스는 실행 파일 경로를 확인하고 종료합니다. 테스트는 임시 경로·가짜 계정을 사용하며 실제 계정 저장소를 대상으로 삼지 않습니다. 문서만 바꿨다면 링크·경로·명령과 `git diff --check`를 확인하고 관련 없는 전체 빌드를 강제하지 않습니다.

## 문제 해결

| 증상 | 확인·다음 조치 |
|---|---|
| CLI·`aho` 없음 | 터미널 재시작·설치기가 출력한 bin 경로 확인. `aho.cmd`는 Windows용이며 다른 환경에서는 `ai-handoff`를 사용합니다. |
| `No matching Codex or Claude config directory found` | 선택한 에이전트를 설치하고 한 번 실행한 뒤 재시도합니다. `--agents` / `-Only`는 감지 결과를 고르는 옵션이며 에이전트 설정을 만들지는 않습니다. |
| `dashboard` 실행 불가 | Windows 시작 메뉴에서 `aho` 검색. `dashboard`는 그 CLI 옆의 `AI Handoff.exe`가 필요합니다. |
| Codex 훅 오류·신뢰 요청 | `/hooks`에서 신뢰하고 재시작 후 `ai-handoff doctor`. |
| 오래된·중복 훅 | doctor로 v1 Node 훅·이전 플러그인을 확인하고 `install --dry-run` 후 필요하면 연동 재설치. |
| 유휴 뒤 데몬 offline | 기본 60초 후 종료는 정상. 일반 훅 또는 `doctor --fix`; 수동 진단은 포그라운드 `--stay-alive`. |
| IPC·저장소 권한 | 샌드박스 밖에서 doctor 실행. `doctor --fix`는 지원하는 실행 환경 문제를 복구합니다. Windows IPC ACL 상속을 보존합니다. |
| 대기 캡슐 없음 | 프로젝트 폴더·`--agent`·`--peek`·종료 코드 확인. 다른 대상은 `others`에 나오며 ID 확인 후 retarget/수신합니다. |
| 임계값 질문·사용량 없음 | 표본의 부재·노후, 모드·임계값·활성 저장 계정 확인. 제공자 조회도 실패할 수 있고 `usage` 추정치가 제공자 한도를 증명하지는 않습니다. |
| 계정 상태가 비어 있음 | 로컬 계정 화면에서 추가 후 `account doctor`. 로컬 표본과 제공자 조회의 새로고침 경로는 다릅니다. |
| Windows EXE 교체 불가 | 해당 빌드 산출물의 정확한 프로세스를 확인·종료하고 재빌드. |
| Linux `GLIBC_x.y not found` | GNU Linux 릴리스와 시스템 호환성을 확인하거나 소스 빌드합니다. 공식 musl/Alpine 파일은 없으며 정확한 최소 glibc 버전은 이번 작업에서 검증하지 않았습니다. |
| 업데이트 다운로드 실패 | 공식 릴리스 파일·네트워크 확인 또는 설치 스크립트로 알려진 버전 고정. |
