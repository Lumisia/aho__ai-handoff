[English](README.md) | **한국어** | [日本語](README.ja.md) | [中文](README.zh.md)

# 고급 도움말

이 문서는 ai-handoff가 기대처럼 동작하지 않을 때 보는 문서입니다.

## 목차

1. [먼저 확인할 것](#먼저-확인할-것)
2. [캡슐이 안 보여요](#캡슐이-안-보여요)
3. [Claude Code와 Codex가 서로 안 이어져요](#claude-code와-codex가-서로-안-이어져요)
4. [저장 위치와 AI_HANDOFF_ROOT](#저장-위치와-ai_handoff_root)
5. [고급 설정 키](#고급-설정-키)
6. [설정할 수 있는 인자 값](#handoff-clear-arguments)
7. [Codex 캡슐 동작](#codex-캡슐-동작)

## 먼저 확인할 것

- 같은 프로젝트 폴더에서 Claude Code와 Codex를 실행했는지 확인하세요.
- `/handoff status`로 현재 프로젝트에 기다리는 캡슐이 있는지 봅니다.
- `/handoff recent`로 다른 프로젝트에 저장된 캡슐이 있는지 봅니다.
- `/handoff doctor`로 저장 위치, 프로젝트 식별자, 캡슐 상태를 진단합니다.
- 설정을 바꾼 뒤에는 새 세션을 열거나 Claude Code에서 `/reload-plugins`를 실행하세요.

Claude Code monitor는 Claude Code v2.1.105 이상, interactive CLI 세션, user/personal 범위 플러그인 설치가 필요합니다. monitor를 쓸 수 없는 환경에서는 Stop hook이 현재 답변이 끝난 뒤 대체 동작합니다.

## 캡슐이 안 보여요

먼저 `/handoff doctor`를 실행하세요. 대부분은 아래 원인 중 하나입니다.

- 다른 폴더에서 실행해서 프로젝트 식별자가 달라졌습니다.
- 캡슐을 이미 한 번 이어받아서 consumed 상태가 됐습니다.
- Claude Code와 Codex가 서로 다른 저장 위치를 보고 있습니다.
- `ask` 모드에서 캡슐 생성을 아직 승인하지 않았습니다.

확인 순서:

```text
/handoff status
/handoff recent
/handoff history
/handoff doctor
```

`recent`에 보이는데 `status`에는 안 보이면, 현재 폴더와 캡슐이 저장된 프로젝트가 다를 가능성이 큽니다.

## Claude Code와 Codex가 서로 안 이어져요

- 두 도구에 모두 플러그인이 설치되어 있어야 합니다.
- 플러그인 내부 이름은 `ai-handoff`입니다.
- Claude Code는 사용량을 status line에서 읽으며, 플러그인 설치나 reload 후 첫 Claude Code 세션에서 statusline runner를 자동 설치합니다.
- Codex는 별도 status line 설정이 필요 없습니다.
- Windows의 Store/MSIX Claude 앱은 `%LOCALAPPDATA%`가 분리될 수 있습니다. 이 경우 `AI_HANDOFF_ROOT`를 같은 경로로 지정해야 합니다.

Windows에서 서로 캡슐을 못 보면 `AI_HANDOFF_ROOT`부터 확인하는 것이 가장 빠릅니다.

## 저장 위치와 AI_HANDOFF_ROOT

저장 루트는 `AI_HANDOFF_ROOT`가 있으면 그 값을 씁니다. 없으면 운영체제 기본 위치를 씁니다.

| OS | 기본 저장 루트 |
|---|---|
| Windows | `%LOCALAPPDATA%\ai-handoff` |
| macOS | `~/Library/Application Support/ai-handoff` |
| Linux | `$XDG_STATE_HOME/ai-handoff` 또는 `~/.local/state/ai-handoff` |

주요 하위 위치:

| 내용 | 위치 |
|---|---|
| 설정 | `<root>/config.json` |
| 프로젝트 데이터 | `<root>/projects/<fingerprint>` |
| 캡슐 | `<root>/projects/<fingerprint>/handoff` |
| 메모리 | `<root>/projects/<fingerprint>/memory` |
| Claude 사용량 샘플 | `<root>/sensors/claude` |

Windows에서 공유 저장소를 지정하는 예:

```powershell
[Environment]::SetEnvironmentVariable("AI_HANDOFF_ROOT", "C:\Users\<you>\ai-handoff-store", "User")
```

macOS/Linux 예:

```bash
export AI_HANDOFF_ROOT="$HOME/ai-handoff-store"
```

환경 변수를 바꾼 뒤에는 Claude Code와 Codex를 모두 다시 시작하세요.

## 고급 설정 키

`/handoff config`는 설정을 보여줍니다. 키를 바꿀 때는 타입과 범위를 맞춰야 합니다.

| 키 | 설명 |
|---|---|
| `triggers.five_hour.burn_rate.enabled` | 빠르게 사용량이 줄어들 때 더 일찍 인계를 준비할지 |
| `triggers.five_hour.burn_rate.runway_minutes` | 남은 시간이 몇 분 이하일 때 준비할지, 5-120 |
| `capsule.completed_autocreate` | 작업 완료 상태에서도 자동 캡슐을 만들지 |
| `codex.inline_final_capsule` | Codex auto 모드에서 최종 답변 fenced 캡슐 흐름을 사용할지, 기본값 `true` |
| `codex.stop_continuation_auto_summary` | 기존 Codex Stop `decision:block` 요약 continuation을 허용할지, 기본값 `false` |
| `codex.stop_continuation_ask` | 기존 Codex Stop `decision:block` ask continuation을 허용할지, 기본값 `false` |
| `codex.degraded_fallback_on_stop` | Codex가 Stop 시점에 처음 threshold를 넘으면 degraded 캡슐을 저장할지, 기본값 `true` |
| `clear.auto.enabled` | SessionStart 때 오래된 used 캡슐 자동 삭제를 켤지, 기본값 `false` |
| `clear.older_than_days` | used 캡슐 정리 기준일, 기본 30일 |
| `handoff.notify_newer_pending` | 더 새로운 대기 캡슐이 있으면 알려줄지 |
| `locale` | 메시지 언어, `en`, `ko`, `ja`, `zh` |
| `debug.stop_log` | Stop hook 판단 로그를 남길지 |
| `memory.auto_recall` | 대화 시작 때 검증된 메모리를 자동으로 불러올지 |
| `memory.auto_recall_token_budget` | 자동 메모리 불러오기에 쓸 토큰 예산 |
| `statusline.show_handoff` | Claude status line에 handoff 정보를 보일지 |
| `notification.fallback` | OS 알림 실패 시 terminal 알림을 쓸지 |

일반 사용자는 `threshold_percent`, `mode`, `realtime.enabled` 정도만 바꿔도 충분합니다.

<a id="handoff-clear-arguments"></a>

## 6. 설정할 수 있는 인자 값

`/handoff clear`는 첫 번째 인자 값으로 삭제 범위를 정합니다.

```text
/handoff clear <this_project, used, consume, pending, expired> [--older-than 7d] [-c]
```

| 인자 값 | 설명 |
|---|---|
| `this_project` | 현재 프로젝트 fingerprint의 ai-handoff 상태 폴더 전체를 삭제합니다. 소스 저장소는 삭제하지 않습니다. 별칭: `this-project`, `project`. |
| `used` | 사용이 끝난 터미널 상태의 캡슐을 삭제합니다. 대상 상태는 `CONSUMED`, `EXPIRED`, `REJECTED`, `SKIPPED`, `FAILED`입니다. |
| `consume` | 소비된 캡슐만 삭제합니다. `consumed`의 별칭입니다. |
| `consumed` | 소비된 캡슐만 삭제합니다. |
| `pending` | 대기 중인 캡슐을 삭제합니다. 대상 상태는 `AVAILABLE`, `DEGRADED_AVAILABLE`입니다. |
| `expired` | 만료된 캡슐만 삭제합니다. |

| 옵션 | 설명 |
|---|---|
| `--older-than 7d` | 지정한 기간보다 오래된 캡슐만 삭제합니다. `ms`, `m`, `h`, `d` 단위를 지원하며 숫자만 쓰면 일 단위입니다. |
| `-c`, `--confirm`, `--yes` | `this_project` 삭제를 즉시 승인합니다. 없으면 먼저 확인용 preview를 반환합니다. |

예시:

```text
/handoff clear used
/handoff clear used --older-than 7d
/handoff clear --older-than 7d
/handoff clear pending
/handoff clear this_project
/handoff clear this_project -c
```

scope 없이 `--older-than`만 쓰면 scope는 `used`로 처리됩니다. `--older-than`을 생략하면 used 계열 scope는 `clear.older_than_days` 설정값을 사용하며 기본값은 30일입니다.

자동 정리는 수동 명령과 별도입니다. `clear.auto.enabled`를 `true`로 설정하면 SessionStart 때 오래된 `used` 캡슐 정리를 실행합니다. 기본값은 꺼짐(`false`)이며, 백그라운드에서 계속 실행되는 방식이 아니라 SessionStart hook이 실행될 때만 동작합니다. 자동 정리 기준일은 `clear.older_than_days`를 사용하며 기본값은 30일입니다.

## Codex 캡슐 동작

Codex Stop hook은 캡슐 생성을 위해 기본값으로 `decision: "block"`을 쓰지 않습니다. Codex에서 이 값은 `reason`을 사용자에게 보이는 continuation prompt로 만들기 때문에, 숨은 내부 지시처럼 보이는 캡슐 생성 문구가 hook feedback과 일반 assistant 답변으로 노출될 수 있습니다.

기본 흐름은 다음과 같습니다.

```text
UserPromptSubmit 또는 PostToolUse
  -> threshold 감지
  -> 최종 답변에 ai-handoff-capsule footer를 붙이라는 developer context 주입
Assistant 최종 답변
  -> 사용자 요청을 정상 답변
  -> fenced ai-handoff-capsule JSON 추가
Stop
  -> 캡슐을 파싱하고 publish
  -> {"continue": true} 반환
```

Codex가 Stop 시점에 처음 threshold를 넘은 것을 알게 되면, 이미 답변이 끝난 뒤라 자연스럽게 footer를 붙일 수 없습니다. 기본값은 `DEGRADED_AVAILABLE` 캡슐을 저장하고 조용히 계속하는 것입니다. 예전처럼 보이는 continuation을 명시적으로 원할 때만 `codex.stop_continuation_auto_summary` 또는 `codex.stop_continuation_ask`를 `true`로 설정하세요.
