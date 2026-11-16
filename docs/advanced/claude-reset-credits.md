# Claude 리셋 크레딧 조사와 구현 범위

조사일: 2026-11-16. 실제 계정 인증정보를 읽거나 리셋을 소비하지 않고 공개 자료를 확인했다.

## 확인한 근거

- [Anthropic 공식 리셋 안내](https://support.claude.com/en/articles/17007452-what-is-a-limit-reset): 리셋은 사용 크레딧 잔액과 별개이며, 종류와 만료가 있고 사용 후 취소할 수 없다. 공식 사용 경로는 Claude의 Settings → Usage이다.
- [공식 모델별 리셋 설명](https://support.claude.com/en/articles/17506494-claude-fable-5-2-launch-offers): 특정 모델의 주간 사용량 복구와 전체 5시간 복구가 함께 제공될 수 있다.
- [CodexBar의 직접 관찰](https://github.com/steipete/CodexBar/issues/3895), [구현 문서](https://github.com/steipete/CodexBar/blob/main/docs/claude.md): 기존 OAuth usage GET에 `cedar_ember=1`을 추가하면 선택적 grant 목록을 받을 수 있다. 횟수는 목록 길이가 아니라 `resets_left`의 합계다. 클라이언트 종류·버전에 따라 `eligible:false`와 `surface` 또는 `cli_version`으로 제한될 수 있다.
- [Comradex](https://github.com/nicosuave/comradex#usage-and-reset-credit-management): 같은 사용량 응답에 리셋 정보를 포함하며 조회로 크레딧을 소비하지 않는다.
- [공개 파서와 소비 요청의 분리](https://github.com/router-for-me/Cli-Proxy-API-Management-Center/blob/main/src/services/api/claudeResetGrants.ts): `eligible`, `grants`, `resets_left`, `resets_total`, `starts_at`, `ends_at`, `paused`, `clears`를 확인할 수 있다.
- [RimZ의 직접 관찰](https://github.com/rimio-ai/rimz/blob/main/docs/externals/agent-adapter/claude-reference.md): `type:full`만으로 전체 초기화를 판정할 수 없다. 모델별 grant도 같은 type을 사용하며 `gives_back_weekly`와 `clears`가 다르다.
- [OpenUsage 요청 구현](https://github.com/robinebers/openusage/blob/main/Sources/OpenUsage/Providers/Claude/ClaudeUsageClient.swift): `skip_spend`는 기존 비용 표시를 생략할 수 있으므로 통합 조회에서는 사용하지 않는다.
- [Anthropic 인증정보 지침](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use): OAuth는 Anthropic 앱을 위한 인증이며 제삼자의 자격정보 수집·중계를 제한한다. 위 조회 스키마는 공개 지원 API 계약이 아니다.

공개 프로젝트의 관찰은 Anthropic의 지원 보장을 뜻하지 않는다. 공식 도움말과 커뮤니티 구현의 CLI 지원 설명도 일치하지 않아, 소비는 공식 사용량 화면에 맡긴다.

## 설계와 선택

기존 저장 계정의 사용량 조회에서 선택적 리셋 정보를 함께 읽는다. 별도 로그인, 쿠키 수집, 두 번째 리셋 조회, 자동 소비를 추가하지 않는다. 대안인 브라우저 세션 수집과 비공개 소비 요청은 요청한 표시 기능에 필요하지 않아 제외한다. 제한된 응답은 0개와 구분한다. 기존 클라이언트 이름 유지와 공개 앱들의 CLI 호환 User-Agent 방식의 장단점을 설명한 뒤, 사용자는 공개 구현을 바탕으로 지원하도록 요청했다. 이에 조회 요청에 검증된 CLI 호환 User-Agent를 사용한다. 비공개 API 변경과 클라이언트 제한 위험은 남아 있다.

GUI에는 안전하게 정규화한 종류, 잔여 횟수, 시작·만료 시각만 전달한다. 토큰, grant ID, 원문 라벨, 전체 응답은 전달·기록하지 않는다. 만료·미시작·일시정지·소진 grant를 제외하고, 잘못된 inventory는 조회 불가로 처리하면서 정상 사용량 창을 보존한다. 종류는 `clears`와 모델별 복구 여부로 판정한다. 후속 사용자 요청에 따라 GUI의 공식 사용량 화면 링크와 브라우저 계정 확인 안내는 제거했으며, 조회 불가 상태는 그대로 구분해 표시한다.

강제 새로고침은 기존 한 번의 사용량 응답 전체를 `active_usage`로 전달해 리셋 정보가 유실되지 않게 한다. 조회를 시작한 슬롯의 식별 정보와 응답 조립 시 현재 활성 슬롯을 비교하고, 계정 전환·같은 이름의 계정 교체가 감지되면 이전 계정의 사용량과 리셋 정보를 버린다. 기존 프런트엔드의 이전 응답 무시 로직도 유지한다.

호환 User-Agent는 OpenUsage에서 확인한 `claude-cli/2.1.294 (external, cli)`를 사용한다. 설치된 CLI 실행이나 버전 탐지를 새로 추가하지 않는다. 조회는 고정 HTTPS 주소에서만 수행하고 리디렉션을 허용하지 않는다. 선택적 조회가 HTTP 400/403으로 거부되면 같은 계정으로 일반 사용량 조회를 한 번 수행하고, HTTP 401/429·네트워크 실패에는 이 재시도를 하지 않는다. 리셋 조회는 계정 화면의 조회 경로에 한정하며 데몬 훅·자동 한도 알림의 요청에는 추가하지 않는다.

## 구현·검증 순서

1. 공개 응답을 가짜 데이터로 재구성한 파서 회귀 테스트를 먼저 실행하고, 날짜·횟수·종류 검증과 안전한 DTO를 구현한다.
2. Tauri의 강제 새로고침 전달과 GUI의 계정별 표시를 구현하고, 두 제공자의 새로고침·지연 응답·만료·조회 불가를 검사한다.
3. core/desktop crate 테스트, 프런트엔드 테스트·빌드, 포맷과 최종 diff를 확인한다. 실제 사용자 인증정보나 리셋 소비를 검증에 사용하지 않는다.

실행 결과와 실제 계정 조회 여부는 작업 완료 보고에 별도로 남긴다. 커밋·푸시는 수행하지 않는다.

## 검증 결과

- `cargo test -p ai-handoff-core -p ai-handoff-desktop -p ai-handoff-cli -p ai-handoff-daemon --quiet`: 성공. 기존 ignored 테스트 5개는 실행하지 않았다. CLI의 훅·오프라인 통합 테스트도 포함한다.
- 계정 전환 경쟁 조건 수정 후 `cargo test -p ai-handoff-desktop --quiet`: 24개 통과.
- `npm --prefix apps/desktop test`: 20개 통과. Windows 샌드박스에서 최초 실행은 자식 프로세스 생성 EPERM으로 실패했으며, 승인된 권한 환경에서 원래 명령으로 재실행했다.
- `npm --prefix apps/desktop run build`: 성공. 500 kB 초과 번들 크기 경고가 남아 있다.
- `cargo fmt --all -- --check`, `git diff --check`: 성공.
- 독립 검토에서 발견한 계정 전환 중 정보 오표시와 자동 알림의 선택적 조회 문제를 수정하고 재검토했다.

위 자동 회귀 테스트는 공개 관찰을 재구성한 가짜 데이터로 정상 응답 스키마와 오류 처리를 검증했다. 공개 API 지원·장기 호환성·모든 계정의 제공 여부는 보장할 수 없다.

## 실제 계정·GUI 확인 (2026-11-16)

후속 사용자 요청에 따라 `npm --prefix apps/desktop run tauri -- build --no-bundle`로 프런트엔드가 포함된 릴리스 GUI를 빌드하고 `target/release/AI Handoff.exe`를 실행했다. 빌드는 성공했고 번들 크기 경고가 남아 있다.

기존 저장 계정의 실제 사용량 조회에서 리셋 잔여 횟수·종류·만료일이 표시되는 것을 확인했다. Claude 공식 앱의 사용량 화면과 만료 시각을 대조했으며, AHO의 강제 새로고침 완료 후에도 같은 리셋 정보가 유지됐다. 확인한 값은 사용자에게 보고하고 계정 식별 정보·인증정보·원문 응답은 이 기록에 저장하지 않았다. 리셋을 소비하지 않았고 새 GUI는 계정 화면을 연 상태로 유지했다.

후속 표시 수정에서는 두 제공자의 리셋 제목에 각각 테마 색상을 적용하고, 계정의 인증 출처 문구와 사용량 영역의 중복 플랜을 제거했다. 이름 아래 플랜은 해당 계정의 조회 결과와 저장된 플랜만 우선 사용하며, 계정 식별이 없는 로컬 사용량 로그의 플랜으로 다른 계정의 플랜을 추정하지 않는다. 확인할 수 없는 플랜은 `unknown` 문구 대신 숨긴다.
