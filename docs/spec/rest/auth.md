# REST 인증

NoteGate는 자체 password login API를 두지 않는다. 사람 사용자는 authgate OAuth/OIDC로 로그인하고, API key는 Agent credential로만 발급한다.

Auth endpoint는 JSON resource API와 성격이 다르므로 `/api/v1` 아래에 넣지 않는다. Browser redirect/callback, session cookie 발급, OAuth protected-resource metadata를 담당한다.

## Browser login 시작

```http
GET /auth/login
```

authgate OAuth/OIDC authorization-code + PKCE login으로 redirect한다. Browser session 갱신을 위해 `offline_access` scope를 요청한다.

## OAuth callback

```http
GET /auth/callback?code=...&state=...
```

`state`/`nonce`를 검증하고 authgate token을 교환한 뒤 lifecycle 정책에 따라 active local user를 생성하거나 갱신한다. 최초 생성이면 `account.create`, browser session 생성에 성공하면 `session.login` audit event를 같은 transaction에서 기록한다. authgate가 발급한 refresh token은 NoteGate browser session 갱신용 credential로 암호화 저장한다. 그 다음 opaque browser session cookie를 발급하고 `/auth/success`로 redirect한다. Inactive local account는 자동 재활성화하지 않는다.

AuthGate는 refresh token의 canonical state, rotation, revoke 여부를 관리한다. NoteGate는 해당 refresh token 값을 브라우저 세션 갱신에 다시 제출할 수 있도록 저장할 뿐이며, browser client에는 refresh token을 노출하지 않는다.

## Login success

```http
GET /auth/success
```

Browser/MCP onboarding용 간단한 HTML 완료 화면을 보여준다. 사용자는 탭을 닫거나 MCP client를 다시 연결할 수 있다.

## Logout

```http
POST /auth/logout
```

notegate browser session cookie를 제거하고 해당 `browser_sessions` row를 revoke한다. 활성 session이 실제로 revoke되면 같은 transaction에서 `session.logout` audit event를 기록한다. 저장된 refresh token은 authgate revoke endpoint에 best-effort로 revoke 요청한다. Revoke 요청 실패는 logout 실패로 처리하지 않는다.

## Browser session renewal

Browser session cookie는 opaque token이다. Token 원문은 cookie에만 있고 DB에는 HMAC hash만 저장한다.

```text
request with browser session cookie
-> session token hash lookup
-> revoked or absolute expires_at reached: 401
-> validated_until still valid: resolve user
-> validated_until expired: claim this session refresh
   -> another request is already refreshing: 503, session remains live for retry
   -> claim acquired: refresh via authgate refresh_token grant
   -> success: rotate stored refresh token if authgate returned one, extend validated_until
   -> invalid_grant/sub mismatch: revoke local browser session, return 401
   -> transient authgate/userinfo failure: store rotated refresh token if token exchange returned one, clear refresh claim, return 503
```

Browser session absolute lifetime은 30일이다. Local validation lease는 1시간이다. 30일이 지나면 refresh token이 authgate에서 아직 유효하더라도 NoteGate browser session은 재로그인을 요구한다.

Agent API key와 browser session의 `last_used_at`은 인증 결과를 바꾸지 않는 관측 metadata다. 성공한 인증은 process-local write-behind buffer에 사용 시각을 기록하고 DB에는 batch로 반영한다. 같은 credential의 관측값은 가장 최신 시각으로 합치며 DB row write는 기존 값보다 1시간 이상 최신인 경우로 제한한다. 따라서 `last_used_at`은 실시간 접속 상태나 보안 판정에 사용하지 않는다.

## OAuth metadata

```http
GET /.well-known/oauth-authorization-server
GET /.well-known/oauth-protected-resource
GET /.well-known/oauth-protected-resource/mcp
```

MCP OAuth client가 authgate authorization server와 resource metadata를 discovery할 수 있게 한다. MCP OAuth public client id 기본값은 `notegate-mcp`이며 설정으로 바꿀 수 있다.
`/mcp/v2`는 Agent API key 전용이므로 OAuth protected-resource metadata를 제공하지 않는다.

MCP `401` 응답은 `WWW-Authenticate` header에 resource metadata와 scope를 포함한다.

## 인증 경계

- Browser UI는 `/auth/callback`이 발급한 browser session cookie를 사용한다.
- Browser session cookie는 `Path=/`, `HttpOnly`, `SameSite=Lax`로 발급한다. 운영 HTTPS 환경에서는 `Secure`를 붙인다.
- Browser session cookie는 opaque token이다. Cookie token hash와 encrypted refresh token은 `browser_sessions`에 저장한다.
- `/api/v1/*`는 browser session cookie만 허용한다. `Authorization` bearer를 보내면 인증을 거부한다.
- `/api/v2/*`는 Agent 소유 `ngk_v2_` API key만 허용한다. User 소유 API key, browser session cookie, OAuth JWT는 인증 수단으로 인정하지 않는다.
- Cookie 기반 browser session으로 `POST`, `PUT`, `PATCH`, `DELETE`를 호출하려면 same-origin `Origin` 또는 `Referer`가 필요하다.
- Swagger UI는 `/swagger-ui/v2`, OpenAPI JSON은 `/openapi/v2.json`에서 제공하며 browser session을 요구한다. 미로그인 브라우저는 로그인 후 요청했던 문서 경로로 복귀한다. 문서 열람 세션은 `/api/v2/*` 호출 권한이 아니며, Swagger API 호출은 별도의 API key를 사용한다.
- MCP는 bearer credential만 허용한다. Browser session cookie는 `/mcp`와 `/mcp/v2`에서 인증 수단으로 인정하지 않는다.
- `/mcp`는 user MCP OAuth bearer만 허용한다.
- `/mcp/v2`는 Agent 소유 `ngk_v2_` API key만 허용한다.
- Device flow는 authgate flow이며 user account로 resolve한다.
