# REST Auth

## Auth

notegate는 자체 password login API를 두지 않는다. 사람 사용자는 authgate OAuth/OIDC로
로그인하고, API key는 연결된 account의 credential로 취급한다.

Auth endpoint는 JSON resource API와 성격이 다르므로 `/api/v1` 아래에 억지로 넣지 않는다.
브라우저 redirect/callback, session cookie 발급, OAuth protected-resource metadata를 담당한다.

### Start browser login

```http
GET /auth/login
```

Starts authgate OAuth/OIDC authorization-code + PKCE login. The response is a redirect to authgate.

### OAuth callback

```http
GET /auth/callback?code=...&state=...
```

Validates state/nonce, exchanges the code with authgate, creates or updates an active local user account
according to lifecycle policy, issues the browser session cookie, and redirects to `/auth/success`. Inactive local accounts are not automatically reactivated.

### Login success

```http
GET /auth/success
```

Shows a small HTML confirmation page for browser/MCP onboarding: login is complete and the caller
can close the tab or reconnect the MCP client.

### Logout

```http
POST /auth/logout
```

Clears the notegate browser session cookie. This does not revoke authgate's upstream session.

### OAuth protected resource metadata

```http
GET /.well-known/oauth-authorization-server
GET /.well-known/oauth-protected-resource
GET /.well-known/oauth-protected-resource/mcp
GET /.well-known/oauth-protected-resource/mcp/{path...}
```

Advertises authgate as the authorization server for REST/MCP bearer-token clients. For MCP OAuth, the registered public authgate client id is `notegate-mcp`; clients request the advertised `resource`.
MCP `401` responses include `WWW-Authenticate: Bearer resource_metadata="...", scope="openid offline_access"` so OAuth-capable clients can discover the resource metadata and request the required scopes.

### Auth boundary

- Browser UI는 `/auth/callback`이 발급한 browser session cookie를 사용한다.
- Browser session cookie는 `Path=/`, `HttpOnly`, `SameSite=Lax`로 발급되며, 운영 HTTPS 환경에서는 `Secure`가 붙는다.
- REST API는 non-browser client를 위해 `Authorization: Bearer ...`도 허용한다. Bearer JWT는 `user`, `ngk_v1_` prefix API key는 `api_keys.account_id`에 연결된 account kind로 resolve한다.
- REST 인증 우선순위는 `ngk_v1_` API key, bearer JWT, browser session cookie 순서다. Bearer가 있으면 cookie fallback을 하지 않는다.
- Cookie 기반 browser session으로 `POST`, `PUT`, `PATCH`, `DELETE`를 호출하려면 same-origin `Origin` 또는 `Referer`가 필요하다. 이 값이 notegate public/resource origin과 맞지 않거나 없으면 `403 forbidden`으로 거부한다.
- Swagger UI는 같은 origin에서 열리므로 browser session cookie 샘플 호출이 가능하다. 별도 credential 테스트는 Swagger `Authorize`에 OAuth bearer JWT 또는 notegate API key credential을 넣어 수행한다.
- MCP는 bearer credential만 허용한다. Browser session cookie는 `/mcp`에서 인증 수단으로 인정하지 않는다.
- Device flow는 authgate flow이며 `user` account로 resolve한다.
