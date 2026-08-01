# MCP Auth

MCP는 bearer 인증만 사용하며 사용자와 Agent의 transport endpoint를 분리한다. 두 endpoint는
같은 tool schema와 service invariant를 사용한다.

```text
/mcp     + OAuth/AuthGate bearer -> user account
/mcp/v2  + ngk_v2_ Agent API key -> agent account
```

`/mcp`는 Agent API key를 허용하지 않는다. OAuth 인증 실패 응답은 protected-resource metadata를 포함한
`WWW-Authenticate` challenge를 반환한다.

`/mcp/v2`는 `ngk_v2_` Agent API key만 허용하고 OAuth bearer와 browser session cookie를 허용하지 않는다. 인증 실패 응답은
`Bearer realm="notegate-agent-mcp-v2"` challenge를 반환한다.

```text
missing/malformed token         -> 401
invalid token                   -> 401
valid authgate token, no user   -> 403 not_registered
inactive OAuth/AuthGate account -> 403 inactive_account
inactive API key account        -> 401 invalid_token
```

MCP auth error는 bearer token, OAuth code, PKCE verifier, API key plaintext를 반환하지 않는다.
