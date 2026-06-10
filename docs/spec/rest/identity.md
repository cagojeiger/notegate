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

- `can_create_workspace`: caller가 user account로 workspace를 생성할 수 있다. Workspace 생성 side effect는 `docs/spec/lifecycle.md`를 따른다. `user` account는 `true`, `agent` account는 `false`다.
- `can_manage_agents`: caller가 user-only agent management endpoint로 agent list/create/delete와 key mint/revoke를 수행할 수 있다. `user` account는 `true`, `agent` account는 `false`다.

초기 진입 흐름은 `/me`로 identity를 확인한 뒤 `GET /api/v1/workspaces`로 workspace를 선택한다. 최초 user 생성과 재로그인 동작은 `docs/spec/lifecycle.md`의 Local user 최초 생성/User 재로그인 정책을 따른다. 신규 user나 모든 workspace를 삭제한 user는 `POST /api/v1/workspaces`로 workspace를 명시적으로 생성한다.

### List current user API keys

```http
GET /api/v1/me/keys?limit=50&cursor=...
```

현재 user account에 연결된 live API key metadata(`revoked_at IS NULL AND expires_at > now()`)를 keyset pagination으로 반환한다. revoked/expired key는 list에 포함하지 않으며 평문 token도 반환하지 않는다. Agent caller는 user key를 관리할 수 없고 `403 forbidden`을 반환한다. 응답은 `keys`와 공통 `page`를 포함한다.

### Create current user API key

```http
POST /api/v1/me/keys
```

```json
{
  "name": "local-cli",
  "expires_at": "<created_at + 30d 이내 RFC3339>",
  "scopes": []
}
```

현재 user account로 인증되는 API key를 만든다. 평문 token은 생성 응답에서 한 번만 반환하고 저장하지 않는다. 한도와 만료 정책은 아래 branching을 따른다.

Branching 규칙:

```text
live keys < 2              -> key 생성
live keys >= 2             -> 409 conflict
name empty or >63 chars    -> 400 invalid input
scopes omitted or []       -> 허용
scopes non-empty           -> 400 invalid input
expires_at future <= 30d   -> 허용
expires_at omitted         -> 400 invalid input
expires_at past/now or >30d -> 400 invalid input
```

### Rotate current user API key

```http
POST /api/v1/me/keys/{key_id}
```

같은 user account에 old key의 `expires_at`을 상속한 new key를 만들고 old key를 revoke한다. New plaintext token은 응답에서 한 번만 반환한다.

### Revoke current user API key

```http
DELETE /api/v1/me/keys/{key_id}
```

대상 key에 `revoked_at`/`revoked_by`를 설정한다. `revoked_reason`은 rotation/system revoke처럼 사유가 있는 경우에만 설정한다. Revoke된 key는 인증에 사용할 수 없다.

### Delete current user account

```http
DELETE /api/v1/me
```

현재 user account를 비활성화하고 개인정보를 익명화한다. 이 endpoint는 사람 사용자 설정/탈퇴용 REST endpoint이며 MCP tool로는 제공하지 않는다. Agent caller는 자기 계정을 삭제할 수 없고 `403 forbidden`을 반환한다.

처리 결과는 `docs/spec/lifecycle.md`의 User 탈퇴 정책과 `docs/spec/security.md`의 탈퇴/익명화 정책을 따른다.

`created_by`, `updated_by`, `deleted_by` attribution은 UUID 참조를 유지한다. 같은 authgate subject로 다시 로그인하더라도 anonymized account는 재활성화하지 않고 local user account 생성 흐름을 탄다.
