# MCP Auth

MCP는 bearer 인증만 사용한다. Endpoint별 credential과 caller mapping은 다음과 같고, 두 endpoint는 같은 tool schema와 service invariant를 사용한다.

| Endpoint | Credential | Caller |
|---|---|---|
| `/mcp` | OAuth/AuthGate bearer | User account |
| `/mcp/v2` | `ngk_v2_` Agent API key | Agent account |

`/mcp`의 인증 실패 응답은 protected-resource metadata를 포함한 `WWW-Authenticate` challenge를 반환한다. `/mcp/v2`의 인증 실패 응답은 `Bearer realm="notegate-agent-mcp-v2"` challenge를 반환한다. Endpoint에 매핑되지 않은 credential은 인증되지 않는다.

```text
missing/malformed token         -> 401
invalid token                   -> 401
valid authgate token, no user   -> 403 not_registered
inactive OAuth/AuthGate account -> 403 inactive_account
inactive API key account        -> 401 invalid_token
```

MCP auth error는 bearer token, OAuth code, PKCE verifier, API key plaintext를 반환하지 않는다.
