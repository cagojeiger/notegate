# MCP Identity

## `me`

Return the authenticated caller identity and global non-workspace capabilities. `me` does not list
workspaces or workspace-specific roles; use `workspaces_list` for that. This tool returns no secrets,
bearer tokens, OAuth codes, PKCE verifiers, or API key plaintext.

Input:

```json
{}
```

Output for a user caller:

```json
{
  "account": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "user": {"sub": "authgate-subject", "email": "user@example.com"},
  "capabilities": {
    "can_create_workspace": true,
    "can_manage_agents": true
  }
}
```

Output for an agent caller:

```json
{
  "account": {"id": "account-id", "kind": "agent", "display_name": "research-agent"},
  "agent": {"name": "research-agent"},
  "capabilities": {
    "can_create_workspace": true,
    "can_manage_agents": false
  }
}
```

Branching:

```text
missing/malformed bearer token       -> HTTP 401 with OAuth discovery challenge
invalid token                        -> HTTP 401
valid authgate token, no local account -> HTTP 403 not_registered with login_url and mcp_url
inactive local account               -> HTTP 403 inactive_account
user account                         -> include user object; can_manage_agents=true
agent account                        -> include agent object; can_manage_agents=false
```

REST `GET /api/v1/me` and MCP `me` use the same identity shape. Workspace-specific roles are listed
through `workspaces_list`, not `me`.
