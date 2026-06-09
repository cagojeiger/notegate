# REST Identity

## Identity

### Get current caller

```http
GET /api/v1/me
```

`/me`는 identity-only endpoint다. 현재 요청을 보낸 account가 누구인지와, workspace에
묶이지 않는 전역 capability만 반환한다. Workspace 목록, effective `owner/viewer/editor` role,
`root_node_id`는 포함하지 않는다. 해당 정보가 필요하면 `GET /api/v1/workspaces`를 호출한다.

User caller 출력:

```json
{
  "account": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "user": {"email": "user@example.com"},
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
    "can_create_workspace": false,
    "can_manage_agents": false
  }
}
```

Capability 의미:

- `can_create_workspace`: caller가 user account로 workspace를 생성하고 lifecycle owner가 될 수 있다. `user` account는 `true`, `agent` account는 `false`다.
- `can_manage_agents`: caller가 user-only agent management endpoint로 agent list/create/delete와 key mint/revoke를 수행할 수 있다. `user` account는 `true`, `agent` account는 `false`다.

Bootstrap 흐름은 `/me`로 identity를 확인한 뒤 `GET /api/v1/workspaces`로 workspace를 선택하거나 초기 workspace를 생성한다.
