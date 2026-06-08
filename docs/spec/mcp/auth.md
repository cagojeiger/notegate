# MCP Auth


이 문서는 notegate MCP tool의 request/response 계약을 정의한다. MCP tools는 LLM/CLI 친화 surface이며, 가능하면 `node_id` workflow를 숨기고 path-first 입력을 사용한다.

Surface:

```text
/mcp
```

Auth:

```text
Bearer token only
```

MCP accepts bearer credentials only; browser/session cookies are not accepted.

Identity mapping:

```text
MCP OAuth 2.1 via authgate -> user account
API key / agent key        -> agent account
```

Device flow through authgate is also a user login. API keys are always agent
credentials, even when they were created by a user.

## First-time user setup

MCP OAuth login proves an authgate identity. The authgate MCP client is `notegate-mcp`; clients must request the notegate MCP resource/audience (for local dev, `http://localhost:9191/mcp`). If the local notegate user/account does not exist yet, the caller must complete browser login once through `/auth/login`, wait for the `/auth/success` confirmation page, then reconnect the MCP client.

MCP OAuth discovery uses:

```text
/.well-known/oauth-authorization-server
/.well-known/oauth-protected-resource
/.well-known/oauth-protected-resource/mcp
```

Unauthenticated `/mcp` requests return `401` with a `WWW-Authenticate` challenge containing `resource_metadata` and the scope hint `openid offline_access`.

If an authenticated MCP caller has no local account, `/mcp` returns `403 not_registered` with
`login_url` and `mcp_url` onboarding hints.
