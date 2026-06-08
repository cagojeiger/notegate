# REST Identity

## Identity

### Get current caller

```http
GET /api/v1/me
```

`/me`는 identity-only endpoint다. 현재 요청을 보낸 account가 누구인지와, workspace에
묶이지 않는 전역 capability만 반환한다. Workspace 목록, `viewer/editor/owner` role,
`root_node_id`는 포함하지 않는다. 해당 정보가 필요하면 `GET /api/v1/workspaces`를 호출한다.

User caller output:

```json
{
  "account": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "user": {"sub": "authgate-sub", "email": "user@example.com"},
  "capabilities": {
    "can_create_workspace": true,
    "can_manage_agents": true
  }
}
```

Agent caller output:

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

Capability semantics:

- `can_create_workspace`: caller can create a workspace as owner.
- `can_manage_agents`: caller can create/delete agents and mint/revoke agent keys. This is `true`
  for `user` accounts and `false` for `agent` accounts.

Bootstrap flow: call `/me` for identity, then `GET /api/v1/workspaces` to choose or
create an initial workspace.
