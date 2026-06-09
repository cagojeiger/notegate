# MCP Auth

MCP auth details for the `/mcp` surface. The common bearer-only identity mapping is defined in
[README.md](README.md#authentication).

## First-time user setup

MCP OAuth login proves an authgate identity, but bearer-token MCP calls only resolve already-created
local notegate user accounts.

Branching:

```text
local user exists     -> MCP OAuth bearer resolves to user caller
local user missing    -> 403 not_registered with login_url and mcp_url
browser login success -> local user account is upserted, then MCP reconnect can succeed
```

Onboarding flow:

```text
1. MCP client discovers protected resource metadata.
2. MCP client authenticates through authgate with client id notegate-mcp and resource/audience http://localhost:9191/mcp in local dev.
3. If not_registered, open /auth/login in a browser.
4. Wait for /auth/success.
5. Reconnect the MCP client.
```

## Discovery

MCP OAuth discovery uses:

```text
/.well-known/oauth-authorization-server
/.well-known/oauth-protected-resource
/.well-known/oauth-protected-resource/mcp
```

Unauthenticated `/mcp` requests return `401` with:

```text
WWW-Authenticate: Bearer resource_metadata="...", scope="openid offline_access"
```

Protected resource metadata returns the configured MCP public client id:

```json
{
  "mcp_client_id": "notegate-mcp"
}
```

## Credential boundary

```text
browser/session cookie on /mcp -> 401
OAuth bearer token             -> user account
user API key bearer           -> user account
agent API key bearer          -> agent account
raw bearer/API key plaintext   -> never returned by auth errors
```
