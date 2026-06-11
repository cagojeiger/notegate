# REST Identity

`/me`는 현재 caller identity와 전역 capability만 반환한다. Space 목록은 `/api/v1/spaces`에서 조회한다.

```http
GET /api/v1/me
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

## Current user API keys

```http
GET    /api/v1/me/keys?limit=50&cursor=...
POST   /api/v1/me/keys
POST   /api/v1/me/keys/{key_id}
DELETE /api/v1/me/keys/{key_id}
```

User caller만 가능하다. User account당 live key 최대 2개, TTL 최대 30일이다.
