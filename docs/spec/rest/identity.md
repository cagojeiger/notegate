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

User caller만 가능하다. 현재 tier와 소유 Space별 저장 Text/File 용량 및 live item 수를 반환한다. `items`는 내부 Space root를 제외한다. `retained_text_bytes`/`retained_file_bytes`는 Space의 전체 used 중 휴지통·삭제 대기분이다. `spaces`는 최대 20개의 활성 Space만 반환하고, `deleted_spaces`는 삭제된 Space 중 보관량이 남은 것들의 count/Text/File bytes 합계다. 삭제된 Space별 목록을 반환하지 않으므로 응답 크기가 증가하지 않는다. 원본 본문 대신 owner counter를 집계한다.

```json
{
  "tier": "tier0",
  "deleted_spaces": {"count": 0, "text_bytes": 0, "file_bytes": 0},
  "spaces": [
    {
      "id": "space-id",
      "name": "Personal",
      "retained_text_bytes": 0,
      "retained_file_bytes": 0,
      "items": {"used": 319, "limit": 1999},
      "text_bytes": {"used": 48120320, "limit": 134217728},
      "file_bytes": {"used": 80000000, "limit": 134217728},
      "reconciliation": {
        "status": "idle",
        "availability": {
          "can_trigger": false,
          "reason": "cooldown",
          "retry_at": "2026-08-19T10:00:00Z"
        }
      }
    }
  ]
}
```

Account, Agent, Agent API key, connection limit은 각 리소스 API에서 별도로 검사하며 이 응답에 포함하지 않는다. 사용량의 계산 기준과 reconciliation 동작은 `../usage-and-quotas.md`를 따른다.

## Current user event history

`GET /api/v1/me/audit-events`는 caller의 audit event 이력을, `GET /api/v1/me/command-invocations`는 caller 소유 범위의 MCP·Command API 호출 이력을 반환한다. 계약은 `events.md`에 둔다.
