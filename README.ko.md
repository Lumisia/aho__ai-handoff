[English](README.md) | **한국어** | [日本語](README.ja.md) | [中文](README.zh.md)

# claude-codex-auto-handoff

> **Claude Code**와 **Codex** 중 하나가 5시간 사용 한도에 가까워지면, 하던 작업을 자동으로 다른 쪽에 넘겨줍니다 — 어디까지 했는지 다시 설명할 필요가 없습니다.

> 플러그인의 내부 이름(매니페스트·명령어에서 쓰는 이름)은 **`ai-handoff`** 입니다.

---

## 이 플러그인이 푸는 문제

Claude Code와 Codex는 각각 **5시간 사용 한도**가 있습니다. 작업에 한창 몰입했는데 한쪽 한도가 차면, 보통 다른 도구로 옮겨서 처음부터 다시 시작합니다. 목표가 뭐였는지, 이미 어떤 결정을 내렸는지, 어떤 파일을 건드렸는지, 무엇이 남았는지를 또 설명해야 하죠.

이 다시-설명하기는 느리고, 실수하기 쉽고, 틀리기도 쉽습니다.

## 무엇을 해주나요

**이어달리기(릴레이)** 를 떠올리면 됩니다. 앞 주자가 지치기 전에 다음 주자에게 바통을 넘기면, 다음 주자는 정확히 같은 지점부터 계속 달립니다.

1. **사용량을 지켜봅니다.** 작은 센서가 5시간 창을 얼마나 썼는지 읽습니다.
2. **한도에 가까워지면**(기본값 **80%**), 지금 어디까지 했는지 — 목표, 핵심 결정, 다음 할 일, 현재 Git 브랜치 — 를 **capsule(캡슐)** 이라는 작은 파일에 적어둡니다.
3. **다른 도구를 열면**, 그 캡슐을 읽어 새 에이전트에게 정확히 어디서부터 이어가면 되는지 보여줍니다.
4. **프로젝트의 검증된 사실도 기억**해 두었다가, 나중 세션에서 관련 있는 것만 다시 가져옵니다.

모든 일은 **내 컴퓨터 안에서** 일어납니다. 클라우드 서버도, 상주 데몬도, 따로 설치할 데이터베이스도 없습니다.

## 자주 나오는 용어, 쉬운 말로

| 용어 | 진짜 뜻 |
|---|---|
| **Capsule(캡슐)** | 지금 작업의 짧은 스냅샷(목표·결정·다음 할 일·브랜치). **한 번** 쓰고 나면 소비됨으로 표시됩니다. |
| **Handoff(인계)** | 그 스냅샷을 한 에이전트(Claude Code 또는 Codex)에서 다른 쪽으로 넘기는 것. |
| **Verified memory(검증된 메모리)** | 증거(통과한 테스트, 명령 실행 결과, 소스 파일)로 뒷받침되는 프로젝트의 지속적 사실 — 추측은 절대 저장하지 않습니다. |
| **Hook(훅)** | 에이전트가 특정 순간(시작할 때, 멈출 때, 프롬프트를 보낼 때)에 자동으로 실행하는 작은 스크립트. |
| **Marketplace(마켓플레이스)** | 에이전트가 플러그인을 찾아 설치하려고 읽는 카탈로그. 이 저장소 자체가 플러그인 하나짜리 마켓플레이스입니다. |

---

## 준비물

- **Node.js 18 이상** (이 도구 전체가 순수 Node이며 **npm 의존성 0개**입니다).
- **Claude Code 또는 Codex**(둘 중 하나만 있어도 단방향으로 동작하지만, 둘 다 있을 때 진가를 발휘합니다).
- 처음 설치할 때 **훅을 한 번 검토하고 신뢰**하기 ([`hooks/hooks.json`](hooks/hooks.json) 참고).

Node 버전 확인:

```bash
node --version
```

---

## 설치

플러그인을 추가하는 방법은 두 가지입니다. 일반 사용에는 **방법 A**(이 GitHub 저장소에서 설치)를 권장합니다. 코드를 먼저 읽거나 수정하고 싶다면 **방법 B**(로컬 폴더 로드)가 좋습니다.

### 방법 A — 플러그인으로 설치 (권장)

이 저장소 자체가 `claude-codex-auto-handoff`라는 **마켓플레이스**이고, 그 안의 플러그인 이름은 `ai-handoff`입니다. 각 에이전트에서 마켓플레이스를 추가한 뒤 플러그인을 설치하는 2단계입니다.

#### Claude Code

Claude Code 안에서(`/plugin ...` 형태) 또는 터미널에서(`claude plugin ...` 형태) 실행하세요:

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

그다음 `/reload-plugins` 를 실행(또는 Claude Code 재시작)해 활성화합니다.

#### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### 방법 B — 로컬 / 개발용

저장소를 클론하고 폴더를 직접 불러옵니다. `PATH/TO/claude-codex-auto-handoff` 는 클론한 위치로 바꾸세요.

```bash
git clone https://github.com/Lumisia/claude-codex-auto-handoff.git
```

Claude Code는 설치 없이 폴더를 불러올 수 있습니다:

```bash
claude --plugin-dir PATH/TO/claude-codex-auto-handoff
```

Codex는 로컬 클론을 마켓플레이스로 추가한 뒤 설치합니다:

```bash
codex plugin marketplace add PATH/TO/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

### Claude Code 센서를 위한 추가 1단계 (두 방법 공통)

Claude는 사용량을 **상태줄(status line)** 에서 읽는데, 플러그인이 그 자리를 혼자 차지할 수 없어서 이 명령을 한 번 실행합니다. 기존에 쓰던 상태줄이 있으면 안전하게 보존합니다.

> ⚠️ **`PATH/TO/claude-codex-auto-handoff` 를 실제 절대경로로 바꾸세요** — 그대로 붙여넣지 마세요(그러면 `Cannot find module ...\PATH\TO\...` 에러가 납니다). Windows 예: `C:\Users\you\claude-codex-auto-handoff`. 가장 안정적인 경로는 저장소를 로컬 클론한 폴더(방법 B)입니다 — 마켓플레이스로 설치했더라도, 클론 경로는 플러그인이 업데이트돼도 바뀌지 않습니다.

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --plugin-root "PATH/TO/claude-codex-auto-handoff"
```

나중에 되돌리려면:

```bash
node "PATH/TO/claude-codex-auto-handoff/core/cli.mjs" setup:claude-statusline --restore
```

(Codex는 사용량을 공식 App Server에서 읽으므로 **추가 센서 설정이 필요 없습니다**.)

### 설치 후 (두 방법 공통)

**새** 에이전트 세션을 시작하고, 안내가 뜨면 lifecycle 훅을 **검토하고 신뢰**하세요. 평소 사용에서는 "훅 신뢰 건너뛰기" 같은 플래그를 쓰지 마세요 — 직접 신뢰를 결정하는 것이 이 도구의 핵심입니다.

---

## 동작 방식 (자동으로 일어나는 세 순간)

플러그인은 안전한 순간에만 동작하며, 실행 중인 도구를 절대 중간에 끊지 않습니다.

- **에이전트가 멈출 때**(`Stop`): 사용량을 확인합니다. 선택한 모드에 따라:
  - `auto` → 묻지 않고 캡슐을 만들어 줍니다.
  - `ask` → 한 번 물어봅니다: *"캡슐을 만들까요? `/handoff create` | `/handoff skip`"*.
  - `off` → 아무 것도 하지 않습니다.
- **에이전트가 시작할 때**(`SessionStart`): 기다리는 캡슐이 있으면 검증(스키마, 파일 해시, 프로젝트 일치, 만료)한 뒤, 새 에이전트에게 작업 내용과 얇은 프로젝트 인덱스를 보여줍니다.
- **첫 프롬프트를 보낼 때**(`UserPromptSubmit`): 관련 있는 **검증된** 프로젝트 메모리만 작은 토큰 예산 안에서 다시 가져옵니다.

전형적인 이어달리기 모습:

```
Claude Code (80% 사용)  →  캡슐 작성  →  Codex 열기  →  Codex가 작업 이어받기
        ↑                                                          │
        └──────────────────  언제든 반대 방향으로도  ──────────────┘
```

---

## 기능 (각 기능 설명)

인계를 촉발하는 센서부터 그 주위의 안전장치까지, 기능별로 설명합니다.

### 1. 5시간 사용량 센서

플러그인은 사용량을 추측하지 않고, 각 도구의 실제 인터페이스에서 읽습니다.

- **Claude Code** → **상태줄(status line)** 브리지가 사용 퍼센트와 리셋 시각을 기록합니다. 데이터가 없거나 오래됐으면, 추측으로 행동하지 않고 조용히 멈춥니다.
- **Codex** → 공식 **App Server**(`account/rateLimits/read`)가 주센서이고, 세션 **JSONL**의 rate-limit 필드가 fallback입니다.

### 2. 자동 캡슐 인계

임계치를 넘으면 플러그인이 **캡슐**을 만듭니다: 목표·결정·제약·미해결 이슈·다음 할 일에 더해 실제 Git 브랜치/커밋과 변경된 파일까지. 캡슐은 atomic publish(임시 파일 → flush → rename)로 기록되어, 절반만 쓰인 캡슐이 읽히는 일이 없습니다. 캡슐은 **불변**이며 **무결성 검사**(해시가 바이트를 서명)됩니다. 받는 에이전트는 짧은 lease로 점유하고 검증·주입한 뒤에야 **소비됨**으로 표시합니다. 캡슐은 한 번만 사용됩니다.

### 3. 세 가지 트리거 모드

플러그인이 얼마나 적극적일지 전역 또는 프로젝트별로 선택합니다: `auto`(조용히 인계), `ask`(사용 창마다 한 번 질문), `off`. 기본 임계치는 **80%** 라서 여유가 있을 때 캡슐을 씁니다 — 의미 캡슐을 쓰는 행위 자체가 사용량을 약간 쓰기 때문입니다.

### 4. 검증된 메모리 recall

한 번 쓰는 캡슐과 별개로, 플러그인은 프로젝트에 대한 **장기 메모리**를 보관합니다 — 단, 증거(통과한 테스트, 명령 결과, 소스 파일)로 뒷받침되는 사실만. 세션의 첫 프롬프트에서, 관련 있고 증거 있는 메모리만 토큰 예산(기본 800) 안에서 가져옵니다. 추측·숨은 추론·전체 대화 기록은 절대 저장하지 않습니다.

### 5. 점진적 프로젝트 지식

캡슐과 함께 프로젝트 지침·양식·함정도 운반할 수 있습니다. 얇은 **INDEX** 와 **manifest**(파일 해시 + dirty 플래그)를 통해, 받는 에이전트가 전부 다시 읽지 않고 지난번 이후 **실제로 바뀐 것만** 읽습니다 — 토큰 절약.

### 6. 스킬과 명령어

세 스킬이 동작을 묶습니다: `handoff-ratelimit`(5시간 트리거), `handoff-session`(`/handoff` 명령군), `handoff-recover`(진단). 이들이 아래 `/handoff` 명령을 구동합니다.

### 7. 내장 안전장치

저장 전에 비밀값이 가려지고, 캡슐은 변조될 수 없으며, 캡슐은 항상 *참고* 자료로만 취급됩니다 — 현재 사용자 지시, 저장소 정책, 실제 파일, Git, 테스트가 모두 캡슐보다 우선합니다. [개인정보 & 안전](#개인정보--안전) 참고.

### 8. 의존성 0, 크로스플랫폼 코어

코어 전체가 순수 Node(기준 18)이며 **npm 의존성이 없습니다**. 컴파일할 것도, 업그레이드 때 깨질 것도 없습니다. Windows·macOS·Linux에서 Node 18/20/22로 테스트됩니다.

---

## 명령어

Claude Code나 Codex 안에서 입력하세요. 양쪽이 동일합니다.

| 명령어 | 하는 일 |
|---|---|
| `/handoff` | 기다리는 캡슐을 이어받습니다 (가장 흔한 동작). |
| `/handoff status` | 현재 인계 상태를 봅니다. |
| `/handoff preview` | 주입하기 전에 캡슐을 미리 봅니다. |
| `/handoff checkpoint` | 지금 바로 캡슐을 수동 저장합니다. |
| `/handoff create` | `ask` 모드에서 캡슐 생성을 승인합니다. |
| `/handoff skip` | `ask` 모드에서 이번 사용 창에 대해 건너뜁니다. |
| `/handoff recover` | 캡슐 / 훅 / 버전 문제를 진단합니다. |

메모리는 **명시적**입니다: 직접 선택할 때만, 그리고 실제 증거(통과한 테스트, 명령 결과, 소스 파일)가 있을 때만 사실을 저장합니다. 숨은 추론이나 전체 대화 기록은 절대 저장하지 않습니다.

---

## 설정

설정은 OS 데이터 폴더의 파일 하나에 들어 있습니다:

- **Windows:** `%LOCALAPPDATA%\ai-handoff\config.json`
- **macOS:** `~/Library/Application Support/ai-handoff/config.json`
- **Linux:** `~/.config/ai-handoff/config.json`

기본값([`config/defaults.json`](config/defaults.json) 참고):

```json
{
  "triggers": { "five_hour": { "enabled": true, "threshold_percent": 80, "mode": "ask" } },
  "capsule":  { "completed_autocreate": false, "semantic_retry_limit": 0 },
  "notification": { "method": "os", "fallback": "terminal" },
  "memory": { "auto_recall": true, "auto_recall_token_budget": 800 }
}
```

자주 바꾸는 값:

- **자동으로 인계:** `"mode": "auto"` 로 설정.
- **더 일찍/늦게 트리거:** `"threshold_percent"` 변경 (예: `70` 또는 `90`).
- **끄기:** `"mode": "off"` 로 설정.

프로젝트별로 설정을 따로 덮어쓸 수도 있습니다.

---

## 개인정보 & 안전

- **로컬 전용.** 캡슐과 메모리는 절대 내 컴퓨터를 벗어나지 않습니다. 클라우드도, 텔레메트리도 없습니다.
- **비밀값은 가려집니다.** 무엇이든 저장되기 전에, 흔한 비밀 패턴(API 키, 토큰, bearer 헤더, 개인 키)을 `[REDACTED]` 로 바꿉니다.
- **캡슐은 변조 불가.** 한 번 발행된 캡슐은 불변이며 해시로 무결성을 검사합니다. 바뀌는 것은 전달 *상태*뿐이고, 검증에 실패한 캡슐은 거부됩니다.
- **항상 사용자 지시가 우선.** 캡슐은 참고 자료입니다. 현재 사용자 지시, 저장소 자체 정책, 실제 파일, Git, 테스트 결과가 모두 캡슐보다 우선합니다.

---

## 테스트 실행

```bash
npm test                 # 단위 + 통합 테스트
npm run validate:package # 플러그인 + 마켓플레이스 매니페스트 검사
```

테스트는 의존성 없는 순수 `node --test` 입니다. CI 매트릭스는 **Windows, macOS, Linux** 에서 **Node 18 / 20 / 22** 로 돌립니다.

실제 로컬 Codex App Server에 대고 라이브 end-to-end 테스트까지 돌리려면:

```bash
AH_E2E=1 npm test
```

---

## 라이선스

[MIT](LICENSE).
