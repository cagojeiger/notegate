# REST 인증

NoteGate는 password login API를 두지 않는다. 브라우저의 신원 확인은 표준 OIDC 공급자가 담당한다. 현재 공급자는 AuthGate이며, MCP·CLI 접근 토큰의 발급 주체와는 별개다. API key는 Agent 전용이다.

## 계정 연결

`oidc.issuer`와 `oidc.client_id`는 필수다. 외부 OIDC 로그인 + 로컬 브라우저 세션 + NoteGate OAuth 서버만 사용한다.

로컬 계정 ID와 Space·문서·이력 소유권은 유지한다. 신원은 `external_identities`의 검증된 `issuer + sub` HMAC으로만 조회하며 email로 연결하지 않는다. 동일한 issuer와 subject는 하나의 계정에만 연결된다. 최초 로그인도 같은 scoped hash의 transaction lock으로 직렬화한다.

현재 AuthGate를 OIDC 공급자로 유지하며, 사전에 검증해 연결한 같은 subject로 기존 계정을 조회한다. 다른 공급자로 전환할 때는 검증된 `account_id / issuer / subject` 매핑을 operator 전용 `import_identities` example로 원자적으로 가져온다. 충돌은 전체 rollback하며, mapping 원문은 비밀 자료로 취급한다. `oidc.allow_signup`은 기본 `false`다. 연결이 끝난 후에만 신규 가입을 열 수 있다.

## 브라우저 로그인과 세션

```text
GET /auth/login?next=<내부 경로>
  → OIDC authorization code + S256 PKCE
  → GET /auth/callback?code=...&state=...
  → issuer / audience / signature / expiry / state / nonce / at_hash / UserInfo subject 검증
  → 같은 로컬 계정 조회·갱신
  → HttpOnly browser session cookie
```

로그인 상태는 5분 동안 DB에 암호화 저장한다. PKCE verifier·nonce·next는 cookie에 저장하지 않는다. 각 flow에 독립적인 browser-binding cookie를 발급하고 콜백에서 한 번만 소비한다. `next`는 내부 경로만 허용한다.

`max_age=0`과 `auth_time`을 요청하며, 검증된 `auth_time`이 현재 시각과 60초를 초과해 차이 나면 거부한다. 공급자의 재인증 정책이 적용되며, 이것이 특정 MFA 수행을 보장한다는 의미는 아니다. 외부 access/refresh token은 로컬 세션에 보관하지 않는다.

세션은 issuer에 연결된다. 활성 계정·철회·`idle_expires_at`·절대 만료 `expires_at`을 요청마다 확인한다. 신규 세션은 미사용 14일, 절대 수명은 설정값(기본·최대 30일)이다. HttpOnly cookie는 절대 만료까지 유지하며, 서버의 미사용 만료가 먼저 오면 401로 OIDC 재로그인을 요구한다. 외부 refresh token은 요구하거나 저장하지 않는다.

`POST /auth/session/activity`는 same-origin Origin/Referer와 유효한 browser cookie를 요구하며 bearer는 거부한다. DB에서 계정 활성·issuer·철회·두 만료를 확인하고 `idle_expires_at`만 현재 시각 + 14일과 절대 만료 중 이른 값까지 연장한다. 응답은 `204`, `private, no-store`이며 token·cookie·`created_at`은 바꾸지 않는다. 따라서 OAuth 연결 승인에 필요한 최근 5분 내 로그인은 세션 연장으로 충족되지 않는다. 만료·철회된 세션은 연장하지 않는다.

Web은 인증된 페이지를 표시하거나 다시 보이게 할 때와 전경의 입력·스크롤 시 탭당 분당 최대 한 번 활동을 기록한다. `/me`나 문서 동기화·백그라운드 조회 자체는 만료를 연장하지 않는다. 활동 UPDATE는 `last_used_at`도 함께 기록한다. 일반 인증 요청의 사용 관측은 기존 write-behind 경로로 시간당 갱신하며 보안 만료의 근거로 사용하지 않는다.

`POST /auth/logout`은 same-origin Origin/Referer와 로컬 세션을 사용해 해당 세션을 철회한다. 외부 token revoke를 호출하지 않는다. 다른 기기의 OAuth 연결 철회는 별도다.

## MCP 연결 승인

```text
MCP client
  → resource / authorization-server metadata
  → GET /oauth/authorize
  → OIDC login 필요 여부 확인
  → GET /oauth/approve: 사용자·client ID·redirect URI·resource·scope 표시
  → POST /oauth/approve: 명시적 승인 또는 거절
  → 검증된 redirect URI로 code + state + iss 반환
  → POST /oauth/token: code + PKCE verifier 교환
```

Authorize는 `response_type=code`, `code_challenge_method=S256`, 정확한 MCP resource와 `mcp [offline_access]` scope를 요구한다. 공개 HTTPS CIMD URL을 client ID로 사용한다. 메타데이터 조회는 public IP만 허용하고 DNS 결과를 연결에 고정하며 proxy와 redirect를 사용하지 않는다. 응답은 64 KiB, 요청은 5초, 동시 조회는 8개로 제한한다. Positive cache는 HTTP no-store/no-cache와 더 짧은 max-age를 존중하며 최대 5분·256개다.

Redirect는 등록 값과 일치해야 한다. HTTP loopback redirect는 port만 달라질 수 있으며 승인 화면에서 로컬 앱으로 시작한 로그인인지 확인하도록 안내한다. GET은 승인하지 않는다. POST는 browser-bound flow, 활성 로컬 세션, 5분 이내의 로그인, same-origin Origin/Referer를 요구한다. 매 연결에 동의를 받으며 자동 승인을 하지 않는다.

## 토큰과 CLI

| 항목 | 정책 |
|---|---|
| Authorization code | 승인 후 60초, 한 번 교환, client/resource/redirect/PKCE에 연결 |
| Access token | 256-bit opaque secret, 최대 10분, resource + scope + 활성 계정 + grant 철회 확인 |
| Refresh token | `offline_access`에서만 발급, 매번 회전, idle 7일 / absolute 30일 |
| Code/refresh 재사용 | 보존된 사용 기록으로 확인하면 해당 grant와 연결된 모든 token을 철회 |
| Scope | MCP는 `mcp`, CLI는 `cli`, 선택적 `offline_access`; 기존 Space·node 권한도 적용 |
| 연결 수 | 사용자당 활성 grant 최대 100개; 동시 승인에도 같은 한도 적용 |

토큰 원문은 응답에서만 전달하고 DB에는 별도 목적 키로 HMAC 저장한다. 외부 OIDC 토큰을 API bearer로 받지 않는다. Refresh 요청은 원래 scope를 반복할 수 있으며 scope 변경은 거부한다. 토큰·철회·실패 응답은 no-store다. 승인 화면의 Referrer-Policy는 `same-origin`으로 동일 출처 form POST의 Origin 검증을 유지하면서 외부 redirect에는 Referer를 보내지 않는다.

CLI는 정적으로 설정한 public client ID로 `POST /oauth/device/authorize`를 호출한다. `cli offline_access`를 요청하고 browser에서 `/oauth/device`에 표시된 요청을 승인한다. Device code는 5분, 기본 poll 간격은 5초다. 빠른 poll은 `slow_down`과 간격 증가를 반환한다. 승인·거절·만료·소비 상태를 DB에서 판정한다. CLI resource는 NoteGate public URL의 `/cli`로 고정한다.

`POST /oauth/token`은 code, refresh, Device grant를 제공한다. Public client 인증 방식은 `none`이며 client secret/HTTP Authorization을 받지 않는다. `POST /oauth/revoke`는 해당 client의 token이 속한 grant를 철회한다. 계정 삭제는 grant도 같은 삭제 트랜잭션에서 철회한다. 만료 판정은 요청 시 수행하고 background purge는 물리 정리만 담당한다.

## 로그인·연결 관리

Settings → Account에서 브라우저 세션·CLI 연결·MCP 연결을 별도 목록으로 관리한다. CLI와 MCP의 전체 철회도 각각 독립적이며 다른 종류의 연결과 브라우저 세션을 유지한다. Agent API key는 기존 Agents 탭에서 개별 철회한다.

| Endpoint | 동작 |
|---|---|
| `GET /api/v1/me/sessions` | 현재 세션을 항상 포함하고 나머지는 최신순으로 활성 세션 최대 100개 |
| `DELETE /api/v1/me/sessions/{id}` | 소유한 세션 하나 종료; 현재 세션이면 cookie도 제거 |
| `DELETE /api/v1/me/sessions/others` | 현재 세션을 제외한 모든 브라우저 세션 종료 |
| `DELETE /api/v1/me/sessions` | 현재 세션을 포함한 모든 브라우저 세션 종료 |
| `GET /api/v1/me/oauth-grants` | 활성 CLI·MCP 연결, 발급·만료 시각, access/refresh 권한 |
| `DELETE /api/v1/me/oauth-grants/{id}` | 소유한 연결의 access/refresh token 모두 철회 |
| `DELETE /api/v1/me/oauth-grants?kind=cli` | 모든 CLI 연결 철회; MCP·브라우저·Agent key는 유지 |
| `DELETE /api/v1/me/oauth-grants?kind=mcp` | 모든 MCP 연결 철회; CLI·브라우저·Agent key는 유지 |

위 경로는 browser cookie 전용이며 mutation은 same-origin을 요구한다. 일괄 OAuth 철회에는 `kind=cli|mcp`가 필수이며 누락·다른 값은 400으로 거부한다. 다른 계정의 대상을 조회하거나 철회하지 않는다. 철회와 Audit은 같은 transaction으로 commit하며 반복 철회는 Audit을 중복 생성하지 않는다. 세션·grant의 철회는 다음 인증 요청부터 적용하고 이미 실행 중인 작업을 취소하지 않는다. OAuth refresh와 철회는 계정 → grant 순서로 잠가 경합 시 새 토큰이 철회를 우회하지 못한다. `/oauth/connections`의 독립 OAuth 관리 화면도 같은 grant·철회 경로를 사용한다.

새 로그인은 요청의 User-Agent에서 브라우저·OS의 제한된 환경 표시만 추출해 저장한다. CLI는 자신의 OS·architecture를 User-Agent에 보내며, OAuth 연결의 환경은 승인 브라우저가 아니라 token 교환 요청에서 기록한다. Raw User-Agent·IP·정밀 위치·장비 지문은 저장하지 않는다. Client 제공값은 참고 표시이며 인증 근거나 물리 장비 식별자로 사용하지 않는다. 기존 연결의 알 수 없는 환경은 추정하지 않는다. `last_token_at`은 token 발급·refresh 시각이며 MCP 요청별 활동 시각이 아니다. 로그인·OAuth 승인 Audit에는 수집한 환경 표시를, 철회 Audit에는 사유를 기록하며 credential은 남기지 않는다.

브라우저 로그아웃은 NoteGate 세션만 종료한다. CLI·MCP 연결 철회나 외부 OIDC 공급자의 SSO 로그아웃과는 별개다.

## Discovery와 경계

`/.well-known/oauth-authorization-server`는 NoteGate issuer·authorize/token/revoke/Device endpoint·S256·CIMD·RFC 9207 issuer 응답 지원·`mcp`, `cli`, `offline_access` scope를 게시한다. 자체 OIDC ID token/JWKS를 제공하는 서버는 아니다.

`/.well-known/oauth-protected-resource[/mcp]`는 MCP resource와 NoteGate authorization server를 게시한다. MCP 401 challenge도 같은 metadata와 MCP scope를 가리킨다.

- `/api/v1/*`: 브라우저 cookie만 허용; mutation은 same-origin.
- `/api/v2/*`, `/mcp/v2`: Agent의 `ngk_v2_` key만 허용.
- `/mcp`: MCP resource용 user OAuth bearer만 허용.
- `/cli`: CLI resource용 user OAuth bearer만 허용.
- Swagger는 브라우저 세션으로 열지만 실제 V2 호출에는 별도 Agent key가 필요하다.

로그인·콜백·로그아웃과 OAuth endpoint는 프로세스별 10 req/s, burst 30 제한을 공유하며 기존 전체 ingress 제한도 적용한다. 일반 문서 API와 `/auth/session/activity`는 별도의 browser V1 제한을 공유하므로 활동 기록이 로그인 제한의 용량을 소모하지 않는다.

## 설정과 운영

`oidc` 설정은 필수다. OIDC discovery cache는 1시간이며 콜백의 서명 키 갱신은 authorization code를 재교환하지 않는다. 로그인 검증과 세션·토큰 경계는 위 계약을 따른다.

신원 import는 검증된 manifest만 사용하는 offline operator다. API와 worker를 중지한 상태에서 현재 ENC/LOOKUP epoch의 ID·상태·version·verify tag를 확인하고 원자적으로 연결한다. 충돌·잘못된 키·익명화 계정은 거부한다. 삭제 보류 계정을 재활성화하거나 계정·PII·소유권을 변경하지 않으며, 같은 연결을 다시 가져와도 Audit을 중복 기록하지 않는다.

기존 DB 업그레이드는 v0.1.112/schema 58을 기준으로 한다. 그보다 오래된 DB의 사전 신원 연결 절차는 [해당 버전의 업그레이드 안내](https://github.com/cagojeiger/notegate/blob/v0.1.112/docs/spec/rest/auth.md#배포-설정)를 따른다. Schema 변경의 점검 중단·백업·복구 기준은 [DB 배포 계약](../db.md#deployment-and-rollback)을 따른다.

## 세션 스키마 업그레이드

Migration 61은 `validated_until`을 `idle_expires_at`으로 이름을 바꾸고 browser session과 OAuth grant에 nullable 환경 표시, grant에 nullable 최근 token 발급 시각을 추가한다. 기존 session ID·HMAC·issuer·생성 시각·두 만료·철회 상태와 계정·문서·OAuth 연결은 그대로 보존한다. 기존 유효 세션은 첫 활동 시 새 정책으로 연장하며, 이미 만료된 세션은 재로그인이 필요하다. 과거 환경·발급 시각을 backfill하거나 추정하지 않는다.

백업 후 기존 API·worker를 모두 종료한 상태에서 새 binary의 embedded migration을 적용한다. 이전 binary는 바뀐 컬럼을 읽지 못하므로 버전을 섞어 실행하거나 이전 이미지만으로 rollback하지 않는다.
