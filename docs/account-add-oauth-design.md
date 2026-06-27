# Account 탭 — "계정 추가하기" (OAuth 로그인) 설계

상태: 설계(미구현). TUI Account 탭의 계정 추가 동작을 **OAuth 로그인 방식**으로 정의한다.
대상 플랫폼: Windows (파일 기반 자격증명). macOS/Keychain은 별도 절에 주석.

---

## 1. 배경 / 결정

`+` 동작을 "현재 로그인된 계정 스냅샷"이 아니라 **공식 OAuth 로그인으로 새 계정을 인증해 추가**하는 방식으로 한다.

- 라벨: `계정 불러오기` → **`계정 추가하기`** 로 변경.
- 조사 결과(아래) 업계 표준 계정 관리는 두 경로가 있다:
  - (A) 자격증명 파일 스냅샷/복원 — 지금 로그인된 계정을 캡처.
  - (B) **OAuth 로그인** — 아직 로그인 안 된 새 계정을 인증해 추가. ← **이번 선택.**
- 전환/삭제는 기존 **파일 스왑 풀** 그대로 유지.

### 조사 근거 (소스코드 직접 확인)

| 툴 | 추가/로그인 메커니즘 | 자격증명 위치 |
|---|---|---|
| **coding_agent_account_manager** (Go) | PTY로 `codex login` / `claude`+`/login` 구동 | Codex `~/.codex/auth.json`; Claude `.credentials.json`(+`~/.claude.json`, `settings.json`, `$CLAUDE_CONFIG_DIR/auth.json`) |
| **cc-switch** (Rust) | Codex OAuth device-code 매니저 내장(`auth_start_login`→device code→`poll_for_token`), 토큰 자동 refresh | 동일 |
| **claude-swap** (Python) | 활성 자격증명 캡처/복원(+macOS Keychain) | `~/.claude/.credentials.json` + `~/.claude.json` 수술적 갱신 |
| **codex-rs** (공식) | `codex login`: 로컬 콜백 브라우저 OAuth(`run_login_server`) 또는 `run_device_code_login` | `$CODEX_HOME/auth.json` |

핵심: "계정 = 자격증명". 로그인 완료 시 **공식 CLI가** 자격증명 파일을 기록하고, 우리는 그 결과 파일을 풀에 보관한다.

---

## 2. "계정 추가하기" 흐름 (OAuth)

```
[+ 계정 추가하기] 선택
   │
   ├─ TUI alternate screen 일시중단 (ratatui suspend, raw mode 해제)
   │
   ├─ 공식 로그인 서브프로세스 실행 (stdio 상속 → 사용자가 직접 OAuth 진행)
   │     Codex : `codex login`
   │     Claude: `claude` 실행 후 사용자가 `/login`
   │
   ├─ 서브프로세스 종료 대기
   │
   ├─ TUI 재초기화 (alternate screen 복귀)
   │
   └─ 새 자격증명을 풀에 캡처 → 목록 갱신 (감지된 이메일 라벨)
```

### 2.1 Codex
- 명령: **`codex login`** (벤더 공식 서브커맨드).
- 기본: 로컬 콜백 서버 + 브라우저 OAuth. 헤드리스/브라우저 불가 시 `codex login --device-code`(device-code) 대안. *(정확한 플래그명은 구현 시 `codex login --help`로 재확인 — 추측 금지.)*
- 결과: `~/.codex/auth.json` 기록(`tokens{ id_token, access_token, refresh_token, account_id }`).
- 완료 신호(참고): "authentication complete" / "login successful".

### 2.2 Claude
- 로그인은 **앱 내부 슬래시 명령 `/login`** 이다 (독립 셸 명령 아님).
- 흐름: `claude` 실행 → 사용자가 `/login` 입력 → 브라우저 OAuth → 종료.
- 결과: `~/.claude/.credentials.json`(claudeAiOauth 토큰) 기록, `~/.claude.json` `oauthAccount` 갱신.
- 대안 조사 필요: 버전에 따라 비대화형 토큰 발급 경로(`claude setup-token` 등) 존재 여부 확인 후 채택. *(미확인 → 구현 전 확인.)*

### 2.3 왜 OAuth를 직접 재구현하지 않나
- 토큰 엔드포인트 / client secret / PKCE 흐름을 **추측해 구현하면 보안 규칙(절대 추측 금지) 위반**.
- 공식 CLI가 검증된 OAuth 흐름을 수행하고 자격증명을 기록 → 우리는 **결과 파일만** 다룬다.
- 우리 코드가 raw OAuth 비밀을 다루지 않음 = 자격증명 노출면 최소.

---

## 3. 자격증명 파일 세트 (캡처 / 전환)

로그인 성공 후 풀에 보관하고, 전환 시 라이브 위치로 되돌려쓴다.

### Codex (단일)
- `~/.codex/auth.json`  (`$CODEX_HOME` 우선)

### Claude (세트)
- `~/.claude/.credentials.json`  — 토큰(핵심)
- `~/.claude.json`  — 계정 메타(`oauthAccount.emailAddress`). **전면 교체 금지** — 큰 공유 설정이라 `oauthAccount` 필드만 **수술적 갱신**.
- (선택) `~/.claude/settings.json`
- (선택) `$CLAUDE_CONFIG_DIR/auth.json` 또는 `~/.config/claude-code/auth.json`

> macOS: Claude 토큰이 **Keychain**(`Claude Code-credentials` 서비스)에 있을 수 있음 → 파일 대신 `security` 명령으로 읽기/쓰기 필요. **Windows는 파일 기반이라 해당 없음.**

---

## 4. 전환 / 삭제 (기존 유지)

- **전환(`s`)**: 풀 슬롯의 자격증명 파일(세트)을 라이브 위치로 원자적 복원(tmp+rename). Claude는 세트 전부, `~/.claude.json`은 `oauthAccount`만 갱신.
- **삭제(`d`)**: 풀 슬롯 제거(확인 후). 라이브 자격증명은 안 건드림.
- 활성 표시: 슬롯 자격증명 == 라이브 자격증명(바이트/식별자 비교).

---

## 5. 보안

- raw OAuth 비밀은 **우리가 다루지 않음**. 공식 CLI가 발급/기록.
- 풀은 자격증명 파일 **복사본**을 `<AI_HANDOFF_HOME>/accounts/<agent>/` 에 보관(유저 승인된 파일 스왑 방식). 평문 토큰 포함 → 디렉터리 권한 주의, 절대 로그/화면/에이전트로 노출 안 함.
- 토큰을 직접 쓰는 곳은 단 하나: 초기화권 개수 조회(`GET /backend-api/wham/usage`)의 `Authorization` 헤더. 그 외 어디에도 노출 금지.
- 모든 파일 읽기/쓰기는 **컴파일된 앱 런타임**에서만(에이전트 transcript 아님).

---

## 6. 범위 밖 (확정)

- **초기화권 지급일/만료일**: 공식 API에 없음. `RateLimitStatusPayload`는 plan/rate_limit/credits/spend_control뿐, 소비응답은 `{code, windows_reset}`뿐. → **개수(available_count)만** 표시 가능.
- **macOS Keychain**: 본 설계는 Windows 파일 기반. macOS 지원은 별도.

---

## 7. 구현 단계 (제안)

1. 라벨 변경: `계정 불러오기` → `계정 추가하기` (en/ko/ja/zh). ← 본 변경에 포함.
2. TUI suspend/resume 헬퍼: alt screen 빠져나가 서브프로세스 stdio 상속 실행 후 복귀.
3. `codex login` 서브프로세스 연동 + 종료 후 `~/.codex/auth.json` 캡처.
4. Claude 로그인 연동(`claude`+`/login` 또는 비대화형 경로 확인 후) + `.credentials.json` 캡처.
5. Claude 자격증명을 **세트**로 캡처/전환 + `~/.claude.json` `oauthAccount` 수술적 갱신.
6. 실패/취소 처리(로그인 중단, 자격증명 미기록) — 풀 변화 없음 + 상태바 사유.

---

## 8. 미해결 / 리스크

- Claude `/login`이 앱 내부 대화형 → 깔끔한 일회성 셸 호출이 어려울 수 있음. 비대화형 경로(`claude setup-token` 등) 존재 여부 구현 전 확인.
- `codex login`은 브라우저 의존(헤드리스 시 `--device-code`). 플래그명 `--help`로 확정.
- TUI suspend 중 로그인 출력이 터미널에 직접 노출 — 정상(사용자 상호작용 필요).

---

## 9. 출처

- openai/codex `codex-rs/cli/src/login.rs`, `login/src/token_data.rs`, `backend-client/src/client/rate_limit_resets.rs`, `codex-backend-openapi-models` (rate_limit_status_payload / credit_status_details)
- Dicklesworthstone/coding_agent_account_manager `internal/handoff/{codex,claude}.go`, `internal/authfile/authfile.go`, `internal/identity/{codex,claude}.go`
- farion1231/cc-switch `src-tauri/src/commands/{auth,codex_oauth}.rs`
- realiti4/claude-swap `src/claude_swap/{paths,credentials,switcher}.py`
