# MCP Identity

## `me`

인증된 caller identity와 workspace에 묶이지 않는 전역 capability를 반환한다. `me`는 workspace 목록이나 workspace-specific role을 반환하지 않는다. 해당 정보는 `workspaces_list`를 사용한다. 이 tool은 secret, bearer token, OAuth code, PKCE verifier, API key plaintext를 반환하지 않는다.

Input:

```json
{}
```

User caller 출력:

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

Agent caller 출력:

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

Branching 규칙:

```text
missing/malformed bearer token         -> OAuth discovery challenge를 포함한 HTTP 401
invalid token                          -> HTTP 401
valid authgate token, no local account -> login_url/mcp_url을 포함한 HTTP 403 not_registered
inactive local account                 -> HTTP 403 inactive_account
user account                           -> user object 포함; can_manage_agents=true
agent account                          -> agent object 포함; can_manage_agents=false
```

REST `GET /api/v1/me`와 MCP `me`는 같은 identity shape을 사용한다. Workspace-specific role은 `me`가 아니라 `workspaces_list`에서 확인한다.

`can_manage_agents=true`는 caller가 user-only agent management endpoint로 agent list/create/delete와 key mint/revoke를 수행할 수 있음을 의미한다.
