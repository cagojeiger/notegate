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

### Delete current user account

```http
DELETE /api/v1/me
```

현재 user account를 비활성화하고 개인정보를 익명화한다. 이 endpoint는 사람 사용자 설정/탈퇴용 REST endpoint이며 MCP tool로는 제공하지 않는다. Agent caller는 자기 계정을 삭제할 수 없고 `403 forbidden`을 반환한다.

처리 결과:

```text
accounts.is_active = false
accounts.deleted_at/deleted_by 설정
user PII ciphertext/hash 제거
account_encryption_keys.wrapped_dek 제거 및 destroyed_at 설정
caller가 소유한 live workspace soft delete
caller가 생성한 active agent soft deactivate
caller가 생성한 agent key revoke
caller/owned-agent/owned-workspace 관련 live workspace_access revoke
```

기존 `created_by`, `updated_by`, `deleted_by` attribution은 UUID 참조를 유지한다. 이후 같은 authgate subject로 다시 로그인하더라도 이전 account는 재활성화하지 않고, 새 local user account 생성 흐름을 탄다.

