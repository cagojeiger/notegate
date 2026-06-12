# MCP Auth

MCP는 bearer 인증만 사용한다.

```text
OAuth/AuthGate bearer      -> user account
ngk_v1_ user API key       -> user account
ngk_v1_ agent API key      -> agent account
```

```text
missing/malformed token         -> 401
invalid token                   -> 401
valid authgate token, no user   -> 403 not_registered
inactive OAuth/AuthGate account -> 403 inactive_account
inactive API key account        -> 401 invalid_token
```

MCP auth error는 bearer token, OAuth code, PKCE verifier, API key plaintext를 반환하지 않는다.
