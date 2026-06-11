# REST Auth

notegate는 자체 password login API를 두지 않는다. 사람 사용자는 authgate OAuth/OIDC로 로그인하고, API key는 연결된 account의 credential로 취급한다.

Auth endpoint는 JSON resource API와 성격이 다르므로 `/api/v1` 아래에 넣지 않는다. Browser redirect/callback, session cookie 발급, OAuth protected-resource metadata를 담당한다.

## Browser login 시작

```http
GET /auth/login
```

authgate OAuth/OIDC authorization-code + PKCE login으로 redirect한다.

## OAuth callback

```http
GET /auth/callback?code=...&state=...
```

`state`/`nonce`를 검증하고 authgate token을 교환한 뒤 lifecycle 정책에 따라 active local user를 생성하거나 갱신한다. 그 다음 browser session cookie를 발급하고 `/auth/success`로 redirect한다. Inactive local account는 자동 재활성화하지 않는다.

## Login success

```http
GET /auth/success
```

Browser/MCP onboarding용 간단한 HTML 완료 화면을 보여준다. 사용자는 탭을 닫거나 MCP client를 다시 연결할 수 있다.

## Logout

```http
POST /auth/logout
```

notegate browser session cookie를 제거한다. authgate upstream session은 revoke하지 않는다.

## OAuth metadata

```http
GET /.well-known/oauth-authorization-server
GET /.well-known/oauth-protected-resource
GET /.well-known/oauth-protected-resource/mcp
GET /.well-known/oauth-protected-resource/mcp/{path...}
```

REST/MCP bearer-token client가 authgate authorization server와 resource metadata를 discovery할 수 있게 한다. MCP OAuth public client id는 `notegate-mcp`다.

MCP `401` 응답은 `WWW-Authenticate` header에 resource metadata와 scope를 포함한다.

## Auth boundary

- Browser UI는 `/auth/callback`이 발급한 browser session cookie를 사용한다.
- Browser session cookie는 `Path=/`, `HttpOnly`, `SameSite=Lax`로 발급한다. 운영 HTTPS 환경에서는 `Secure`를 붙인다.
- REST API는 `Authorization: Bearer ...`도 허용한다.
- REST 인증 우선순위는 `ngk_v1_` API key, bearer JWT, browser session cookie 순서다.
- Bearer가 있으면 cookie fallback을 하지 않는다.
- Cookie 기반 browser session으로 `POST`, `PUT`, `PATCH`, `DELETE`를 호출하려면 same-origin `Origin` 또는 `Referer`가 필요하다.
- Swagger UI는 같은 origin에서 열리므로 browser session cookie 샘플 호출이 가능하다.
- MCP는 bearer credential만 허용한다. Browser session cookie는 `/mcp`에서 인증 수단으로 인정하지 않는다.
- Device flow는 authgate flow이며 user account로 resolve한다.
