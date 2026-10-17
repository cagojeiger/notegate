# REST 인증

NoteGate는 password login API를 두지 않는다. 브라우저의 신원 확인은 표준 OIDC 공급자가 담당한다. 현재 공급자는 AuthGate이며, MCP·CLI 접근 토큰의 발급 주체와는 별개다. API key는 Agent 전용이다.

## 모드와 계정 연결

- `oidc` 설정: 외부 OIDC 로그인 + 로컬 브라우저 세션 + NoteGate OAuth 서버.
- `oidc` 미설정: 기존 AuthGate 브라우저 refresh 및 MCP·CLI JWT 호환 모드.

로컬 계정 ID와 Space·문서·이력 소유권은 유지한다. 신원은 검증된 `issuer + sub` HMAC으로 조회하며 email로 연결하지 않는다. 기존 subject hash는 `authgate_url`로 한 번 고정한 원래 issuer에만 허용한다. 독립 모드에서 이 값은 기존 신원 연결용이며 외부 토큰 검증·갱신 URL로 사용하지 않는다. 기존 hash가 남은 DB에 issuer binding이 없으면 해당 값을 지정하기 전 startup을 거부한다.

현재 AuthGate를 OIDC 공급자로 유지하면 같은 subject로 기존 계정에 연결된다. 다른 공급자로 전환할 때는 검증된 `account_id / issuer / subject` 매핑을 operator 전용 `import_identities` example로 원자적으로 가져온다. 충돌은 전체 rollback하며, mapping 원문은 비밀 자료로 취급한다. `oidc.allow_signup`은 기본 `false`다. 연결이 끝난 후에만 신규 가입을 열 수 있다.

## 브라우저 로그인과 세션

```text
GET /auth/login?next=<내부 경로>
  → OIDC authorization code + S256 PKCE
  → GET /auth/callback?code=...&state=...
  → issuer / audience / signature / expiry / state / nonce / at_hash / UserInfo subject 검증
  → 같은 로컬 계정 조회·갱신
  → HttpOnly browser session cookie
```

독립 모드의 로그인 상태는 5분 동안 DB에 암호화 저장한다. PKCE verifier·nonce·next는 cookie에 저장하지 않는다. 각 flow에 독립적인 browser-binding cookie를 발급하고 콜백에서 한 번만 소비한다. `next`는 내부 경로만 허용한다.

`max_age=0`과 `auth_time`을 요청하며, 검증된 `auth_time`이 현재 시각과 60초를 초과해 차이 나면 거부한다. 공급자의 재인증 정책이 적용되며, 이것이 특정 MFA 수행을 보장한다는 의미는 아니다. 외부 access/refresh token은 로컬 세션에 보관하지 않는다. 기존 non-null encrypted refresh 필드는 빈 값을 암호화한 호환용 envelope다.

세션은 issuer에 연결된다. 활성 계정·철회·absolute expiry와 24시간 로컬 인증 기간을 요청마다 확인한다. 기간이 끝나면 OIDC 로그인으로 돌아가며 외부 refresh 호출은 하지 않는다. Cookie의 최대 수명은 기존 설정(기본 30일)을 따른다. 사용 시각은 기존 write-behind 관측 metadata로만 사용한다.

`POST /auth/logout`은 same-origin Origin/Referer와 로컬 세션을 사용해 해당 세션을 철회한다. 독립 모드에서는 외부 token revoke를 호출하지 않는다. 다른 기기의 OAuth 연결 철회는 별도다.

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

`/.well-known/oauth-authorization-server`는 독립 모드에서 NoteGate issuer·authorize/token/revoke/Device endpoint·S256·CIMD 지원·`mcp`, `cli`, `offline_access` scope를 게시한다. 자체 OIDC ID token/JWKS를 제공하는 서버는 아니다.

`/.well-known/oauth-protected-resource[/mcp]`는 MCP resource와 NoteGate authorization server를 게시한다. MCP 401 challenge도 같은 metadata와 MCP scope를 가리킨다.

- `/api/v1/*`: 브라우저 cookie만 허용; mutation은 same-origin.
- `/api/v2/*`, `/mcp/v2`: Agent의 `ngk_v2_` key만 허용.
- `/mcp`: MCP resource용 user OAuth bearer만 허용.
- `/cli`: CLI resource용 user OAuth bearer만 허용.
- Swagger는 브라우저 세션으로 열지만 실제 V2 호출에는 별도 Agent key가 필요하다.

독립 모드의 auth endpoint는 IP별 10 req/s, burst 30 제한을 공유하며 기존 전체 ingress 제한도 적용한다. 일반 문서 API 요청에는 이 추가 auth 제한을 적용하지 않는다.

## 호환 모드

`oidc`가 없으면 기존 AuthGate OAuth 설정을 사용한다. 브라우저 로그인은 `offline_access`를 요청하고 외부 refresh token을 암호화 보관한다. 로컬 validation lease는 기본 1시간, absolute lifetime은 기본 30일이다. 갱신은 기존 claim/rotation/revoke 절차를 따르며 transient 실패는 retry 가능한 503, 영구 실패는 revoke 후 401이다. Logout의 외부 revoke는 best effort다.

MCP·CLI는 이 모드에서만 AuthGate JWT/JWKS를 검증한다. 두 경로가 JWKS cache를 공유하며 초기·unknown-kid 조회의 완료 시점부터 5초 동안 재조회를 억제한다. 브라우저 OIDC discovery cache는 1시간이며 콜백의 키 갱신은 code를 재교환하지 않는다. 두 모드의 브라우저 세션은 섞어 사용하지 않는다. 독립 모드 전환 시 기존 브라우저와 MCP·CLI 연결은 재로그인이 필요하다. CLI는 두 모드의 scope discovery를 지원하는 버전으로 먼저 갱신한다. 서버·CLI를 호환 모드로 배포한 뒤 전체 인스턴스의 설정을 일관되게 전환하며, 같은 endpoint에 서로 다른 인증 모드를 섞어 두지 않는다.
