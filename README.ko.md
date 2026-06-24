<img width="1008" height="508" alt="Main_Image" src="https://github.com/user-attachments/assets/a9c741a2-0e24-403f-9f19-d3f6f6a2b86c" />

[English](README.md) | **한국어** | [日本語](README.ja.md) | [中文](README.zh.md)

# claude-codex-auto-handoff

Claude Code와 Codex 사이에서 작업을 이어 주는 플러그인입니다.

한쪽의 5시간 사용 한도가 가까워지면, 지금 작업 상태를 **캡슐(capsule)** 이라는 작은 파일로 저장합니다. 그다음 다른 도구가 이 캡슐을 읽고 같은 작업을 이어갑니다.

플러그인 내부 이름은 `ai-handoff`입니다.

도움이 필요하거나 자세한 정보를 보고 싶으면 [해당 글자를 클릭하세요](docs/advanced/README.ko.md).

## 왜 필요한가요?

Claude Code와 Codex는 각각 5시간 사용 한도가 있습니다. 작업 중 한쪽 한도가 차면, 보통 다른 도구에서 목표, 변경한 파일, 남은 일 등을 다시 설명해야 합니다.

이 플러그인은 그 설명을 대신 준비합니다.

## 캡슐에 들어가는 것

- 지금 작업의 목표
- 완료한 일
- 남은 일
- 변경한 파일
- 현재 Git 브랜치와 커밋
- 다음 도구가 먼저 확인해야 할 내용

캡슐은 한 번 사용되면 소비됨으로 표시됩니다.

## 준비물

- Node.js 18 이상
- Claude Code 또는 Codex
- 두 도구를 모두 쓰면 양방향 인계가 됩니다

Node 버전 확인:

```bash
node --version
```

## 설치

### Claude Code

Claude Code 안에서 실행:

```text
/plugin marketplace add Lumisia/claude-codex-auto-handoff
/plugin install ai-handoff@claude-codex-auto-handoff
```

또는 터미널에서 실행:

```bash
claude plugin marketplace add Lumisia/claude-codex-auto-handoff
claude plugin install ai-handoff@claude-codex-auto-handoff
```

설치 후 `/reload-plugins`를 실행하거나 Claude Code를 다시 시작하세요.

### Codex

```bash
codex plugin marketplace add Lumisia/claude-codex-auto-handoff
codex plugin add ai-handoff@claude-codex-auto-handoff
```

## Claude Code statusline 센서

Claude Code 사용량은 Claude Code status line 입력에서 읽습니다.

별도 setup 명령을 직접 실행할 필요는 없습니다. 플러그인을 설치하거나
`/reload-plugins`한 뒤 첫 Claude Code 세션이 시작되면, 플러그인이 안정적인
로컬 statusline runner를 자동으로 설치합니다.

자동 설치가 실패했을 때만 아래 명령을 실행하세요.

```bash
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --plugin-root "$PLUGIN_ROOT"
```

이전 statusline으로 복원하려면:

```bash
node "$PLUGIN_ROOT/core/cli.mjs" setup:claude-statusline --restore
```

자동 설치를 끄려면 Claude Code 실행 환경에 아래 환경 변수를 설정하세요.

```bash
AI_HANDOFF_NO_AUTO_STATUSLINE=1
```

Codex는 별도 센서 설정이 필요 없습니다.

## 어떻게 동작하나요?

1. Claude Code 또는 Codex가 사용량을 확인합니다.
2. 기본값 80%에 가까워지면 캡슐을 만들 준비를 합니다.
3. `ask` 모드에서는 사용자에게 만들지 물어봅니다.
4. `auto` 모드에서는 자동으로 캡슐을 만듭니다.
5. 다른 도구를 열면 캡슐을 읽고 이어서 작업합니다.

Claude Code에서는 plugin monitor가 자동으로 사용량을 지켜볼 수 있습니다. 사용자가 `scripts/usage-monitor.mjs`를 직접 실행하지 않아도 됩니다.

monitor가 동작하려면 Claude Code v2.1.105 이상, interactive CLI 세션, user/personal 범위의 플러그인 설치가 필요합니다. monitor를 쓸 수 없는 환경에서는 Stop hook이 대신 동작합니다.

## 기본 사용법

가장 자주 쓰는 명령:

| 명령 | 설명 |
|---|---|
| `/handoff` | 기다리는 캡슐을 이어받습니다 |
| `/handoff status` | 현재 상태를 봅니다 |
| `/handoff preview` | 캡슐 내용을 미리 봅니다 |
| `/handoff checkpoint` | 지금 상태를 수동 저장합니다 |
| `/handoff history` | 현재 프로젝트의 인계 기록을 봅니다 |
| `/handoff recent` | 모든 프로젝트의 최근 캡슐을 봅니다 |
| `/handoff clear <this_project, used, consume, pending, expired>` | 다양한 인자 값으로 삭제할 범위를 정합니다. [자세한 설명을 보시기 바랍니다.](docs/advanced/README.ko.md#handoff-clear-arguments) |
| `/handoff doctor` | 설정이나 캡슐 문제를 진단합니다 |
| `/handoff config` | 설정을 봅니다 |

Claude Code에서는 명령이 `/ai-handoff:handoff-...`처럼 보일 수 있습니다. 문서에서는 읽기 쉽게 `/handoff`로 적었습니다.

## 설정

설정 파일은 운영체제별로 아래 위치에 둡니다.

- Windows: `%LOCALAPPDATA%\ai-handoff\config.json`
- macOS: `~/Library/Application Support/ai-handoff/config.json`
- Linux: `~/.local/state/ai-handoff/config.json`

가장 많이 바꾸는 설정 예시:

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

중요한 값:

| 키 | 기본값 | 설명 |
|---|---:|---|
| `triggers.five_hour.threshold_percent` | `80` | 몇 %에서 인계를 준비할지 |
| `triggers.five_hour.mode` | `ask` | `ask`, `auto`, `off` 중 하나 |
| `approval.ttl_ms` | `900000` | 질문 응답이 유효한 시간, 기본 15분 |
| `sensors.claude.freshness_ms` | `10000` | Claude 사용량 샘플 유효 시간, 기본 10초 |
| `realtime.enabled` | `true` | Claude Code monitor 사용 여부 |
| `realtime.poll_interval_ms` | `1000` | monitor 확인 주기, 기본 1초 |

설정을 바꾼 뒤에는 새 세션을 시작하세요.

## 주의할 점

- 캡슐과 메모리는 내 컴퓨터 안에만 저장됩니다.
- API 키나 토큰 같은 비밀값은 저장 전에 가려집니다.
- 캡슐은 참고 자료입니다. 실제 파일, Git 상태, 테스트 결과가 더 중요합니다.
- monitor는 실행 중인 답변을 중간에 끊지 않습니다. 현재 답변이 끝난 뒤 반응할 수 있습니다.

## 개발자용 테스트

```bash
npm test
npm run validate:package
```

## 라이선스

[MIT](LICENSE)
