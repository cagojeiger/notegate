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

세션은 issuer에 연결된다. 활성 계정·철회·absolute expiry와 24시간 로컬 인증 기간을 요청마다 확인한다. 기간이 끝나면 OIDC 로그인으로 돌아가며 외부 refresh 호출은 하지 않는다. Cookie의 최대 수명은 기존 설정(기본 30일)을 따른다. 사용 시각은 기존 write-behind 관측 metadata로만 사용한다.

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

Settings → Account의 **Manage connected applications**는 `/oauth/connections`로 연결한다. 이 화면에서 현재 사용자의 활성 grant만 표시하고 same-origin POST로 개별 철회한다. 승인·철회 audit에는 사용자·grant ID와 사유만 남기며 credential은 기록하지 않는다.

## Discovery와 경계

`/.well-known/oauth-authorization-server`는 NoteGate issuer·authorize/token/revoke/Device endpoint·S256·CIMD·RFC 9207 issuer 응답 지원·`mcp`, `cli`, `offline_access` scope를 게시한다. 자체 OIDC ID token/JWKS를 제공하는 서버는 아니다.

`/.well-known/oauth-protected-resource[/mcp]`는 MCP resource와 NoteGate authorization server를 게시한다. MCP 401 challenge도 같은 metadata와 MCP scope를 가리킨다.

- `/api/v1/*`: 브라우저 cookie만 허용; mutation은 same-origin.
- `/api/v2/*`, `/mcp/v2`: Agent의 `ngk_v2_` key만 허용.
- `/mcp`: MCP resource용 user OAuth bearer만 허용.
- `/cli`: CLI resource용 user OAuth bearer만 허용.
- Swagger는 브라우저 세션으로 열지만 실제 V2 호출에는 별도 Agent key가 필요하다.

Auth endpoint는 IP별 10 req/s, burst 30 제한을 공유하며 기존 전체 ingress 제한도 적용한다. 일반 문서 API 요청에는 이 추가 auth 제한을 적용하지 않는다.

## 배포 설정

`oidc` 설정이 없으면 startup을 거부한다. 구형 `authgate_url`, `oauth_client_id`, `mcp_oauth_client_id`, `jwks_cache_ttl_secs`, `browser_session_ttl_secs`, `legacy_identity_issuer`는 제거했으므로 배포 환경에서도 삭제해야 한다. 기존 로컬 세션·OAuth grant·계정 연결·이력은 보존한다. 구형 AuthGate bearer와 refresh 기반 브라우저 세션은 허용하지 않는다.

0057 migration은 브라우저의 사용하지 않는 refresh 암호문·claim·갱신 시각 7개 컬럼을 제거한다. 구버전 API의 SELECT/INSERT와 호환되지 않으므로 rolling 배포하지 않는다. 복구 가능한 DB 백업을 확인하고 구 API 프로세스를 모두 중지한 뒤 신버전의 startup migration과 시작을 수행한다. 이후 readiness·기존 세션·새 로그인·로그아웃·OAuth 연결을 검증한다. 새 migration ledger와 schema를 구버전이 지원하지 않으므로 이전 이미지로만 되돌리는 rollback은 보장하지 않는다. 복구 시 백업 복원 또는 해당 schema와 ledger를 지원하는 복구 바이너리가 필요하다.

0058 migration은 모든 미익명화 계정의 external identity 연결을 요구한다. 삭제 보류 계정도 포함하며, 기존 subject hash가 남아 있으면 원래 issuer의 연결이 있어야 한다. 미연결 계정이 하나라도 있으면 schema 변경 전에 실패한다. subject hash로 원문 subject를 복원할 수 없으므로 정상 OIDC 로그인 또는 검증된 operator 매핑으로 먼저 연결한다. 이메일로 추정 연결하지 않는다.

미연결 삭제 보류 계정은 로그인할 수 없으므로 새 버전의 `import_identities` operator를 migration 전에 사용한다. API와 worker 프로세스를 모두 중지한 상태에서 검증된 manifest를 가져온다. 이 도구는 schema migration을 실행하지 않으며 활성 계정과 삭제 보류 계정의 신원만 연결한다. 삭제 상태·PII·소유권은 변경하지 않고, 이미 익명화한 계정은 거부한다. 같은 연결을 다시 가져오면 추가 Audit 없이 성공한다.

Import는 저장 전에 현재 ENC/LOOKUP 키 epoch의 ID·상태·version·verify tag를 검증한다. 같은 키 ID에 잘못된 secret을 지정하거나 epoch가 없으면 신원과 Audit을 쓰지 않고 실패하며, 새 epoch를 만들거나 교체하지 않는다.

연결을 확인한 뒤 users의 구형 subject hash 3개 컬럼과 issuer binding 테이블을 제거한다. `NOTEGATE_LEGACY_IDENTITY_ISSUER`도 배포 환경에서 제거한다. 구 API는 이 테이블·컬럼을 조회하므로 0057과 같은 백업·점검 중단 순서를 따르고, 구 프로세스가 남아 있는 동안 migration을 실행하지 않는다. 계정 ID·PII·문서·이력·현재 세션·OAuth grant는 보존한다. 이전 이미지 단독 rollback은 지원하지 않는다.

OIDC discovery cache는 1시간이며 콜백의 서명 키 갱신은 authorization code를 재교환하지 않는다. 로그인 시 ID token의 issuer·audience·서명·nonce·최근 auth_time과 userinfo subject를 검증한다.
