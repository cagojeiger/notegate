# REST Identity

`/me`는 현재 caller identity와 전역 capability만 반환한다. Space 목록은 `/api/v1/spaces`에서 조회한다.

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

`DELETE /api/v1/me`는 user caller만 가능하다. Live owned space가 있으면 거부한다.

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
- `scopes`는 현재 빈 배열만 허용한다.
- User account당 live key 최대 2개다.
