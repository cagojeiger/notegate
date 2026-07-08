# REST Events

Event history는 self-review를 위한 이력이다. User caller는 자기 계정과 space에 어떤 관리 변경이 있었는지 확인한다. 스키마와 capture 계약은 `docs/spec/event-logging.md`가 정본이다.

## List my audit events

```http
GET /api/v1/me/audit-events?limit=50&cursor=...
```

User caller만 가능하다. Caller의 `owner_user_id` scope에 속한 `audit_events`를 `created_at desc, id desc` 순으로 반환한다.

```json
{
  "events": [
    {
      "id": 1042,
      "created_at": "2026-07-08T09:12:00Z",
      "actor_account_id": "account-id",
      "source": "rest",
      "op_type": "space.update",
      "resource_type": "space",
      "resource_id": "space-id",
      "metadata": {"changed_fields": ["name"]}
    }
  ],
  "page": {"limit": 50, "returned": 1, "has_more": false, "next_cursor": null}
}
```

- 기본 page size는 50, 최대 100이다.
- `metadata`는 `op_type`별 allowlist를 따르는 structural fact만 담는다.
