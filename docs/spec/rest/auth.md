# REST Auth

## Auth

notegate는 자체 password login API를 두지 않는다. 사람 사용자는 authgate OAuth/OIDC로
로그인하고, API key는 agent credential로 취급한다.

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

Validates state/nonce, exchanges the code with authgate, upserts/activates the local user account
according to identity policy, issues the browser session cookie, and redirects to `/auth/success`.

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

- Browser UI uses the secure session cookie issued by `/auth/callback`.
- REST API also accepts `Authorization: Bearer ...` for non-browser clients.
- MCP accepts bearer credentials only; browser cookies are not accepted by MCP.
- Device flow is an authgate flow and resolves to a `user` account.
- API key / agent key authentication resolves to an `agent` account.
