# REST Identity

`/me`는 caller identity와 전역 capability만 반환한다. Space 목록은 `/api/v1/spaces`에서 조회한다.

```http
GET    /api/v1/me
DELETE /api/v1/me
```

User caller:

```json
{
  "account": {"id":"account-id","kind":"user","display_name":"Kang"},
  "user": {"email":"user@example.com"},
  "capabilities": {"can_create_space":true,"can_manage_agents":true}
}
```

Agent caller:

```json
{
  "account": {"id":"account-id","kind":"agent","display_name":"research-agent"},
  "agent": {"name":"research-agent"},
  "capabilities": {"can_create_space":false,"can_manage_agents":false}
}
```

`DELETE /api/v1/me`는 user caller만 가능하다. Live owned space가 있으면 거부한다. 성공하면 owned agents를 deactivate하고, user/agent API key를 revoke하고, owned agent connection을 disconnect한 뒤 user account를 soft-delete한다.

## Current user usage

```http
GET /api/v1/me/usage
```

User caller만 가능하다. 현재 tier quota와 live usage를 반환한다. User당 live Space가 최대 20개이므로 pagination하지 않는다.

```json
{
  "tier": "tier0",
  "account": {
    "spaces": {"used": 1, "limit": 1},
    "agents": {"used": 2, "limit": 3},
    "api_keys": {"used": 1, "limit": 2}
  },
  "spaces": [
    {
      "id": "space-id",
      "name": "Daily",
      "nodes": {"used": 320, "limit": 2000},
      "content_bytes": {"used": 48120320, "limit": 134217728},
      "agent_connections": {"used": 2, "limit": 5}
    }
  ]
}
```

사용량의 계산 기준과 전체 재계산 중 read-only 동작은 `../usage-and-quotas.md`를 따른다.

## Current user API keys

```http
GET    /api/v1/me/keys?limit=50&cursor=...
POST   /api/v1/me/keys
POST   /api/v1/me/keys/{key_id}
DELETE /api/v1/me/keys/{key_id}
```

User caller만 가능하다.

```json
{"name":"local-cli","expires_at":"2026-07-01T00:00:00Z","scopes":[]}
```

- `expires_at`은 필수이며 최대 TTL은 30일이다.
- `scopes`는 빈 배열만 허용한다.
- User account당 live key 최대 2개다.

## Current user event history

`GET /api/v1/me/audit-events`는 caller의 audit event 이력을 반환한다. 계약은 `events.md`에 둔다.
