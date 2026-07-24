# REST Events

Event history는 self-review를 위한 이력이다. User caller는 자기 계정과 space에 어떤 관리 변경과 파일 변경이 있었는지 확인한다. 스키마와 capture 계약은 `docs/spec/event-logging.md`가 정본이다.

## List my audit events

```http
GET /api/v1/me/audit-events?limit=50&cursor=...
```

User caller만 가능하다. Caller의 `owner_user_id` scope에 속한 `audit_events`를 `created_at desc, id desc` 순으로 반환한다. `actor`는 현재 조회 가능한 account reference이며, account가 purge되었으면 `null`일 수 있다.

```json
{
  "events": [
    {
      "id": 1042,
      "created_at": "2026-07-08T09:12:00Z",
      "actor_account_id": "account-id",
      "actor": {"id": "account-id", "kind": "user", "display_name": "Kang"},
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

## List space file change events

```http
GET /api/v1/spaces/{space_id}/file-change-events?node_id=...&limit=50&cursor=...
```

Space read/stat 권한이 필요하다. `node_id`를 생략하면 space 전체 파일/폴더/문서 변경 이력을 `created_at desc, id desc` 순으로 반환한다. `node_id`를 주면 해당 node의 이력만 반환한다. `actor`는 현재 조회 가능한 account reference이며, account가 purge되었으면 `null`일 수 있다.

```json
{
  "events": [
    {
      "id": 2048,
      "created_at": "2026-07-08T09:15:00Z",
      "space_id": "space-id",
      "node_id": "node-id",
      "actor_account_id": "account-id",
      "actor": {"id": "account-id", "kind": "agent", "display_name": "Codex"},
      "op_type": "text.write",
      "metadata": {
        "item_kind": "text",
        "item_name": "roadmap.md",
        "byte_len_before": 120,
        "byte_len_after": 180,
        "line_count_before": 8,
        "line_count_after": 12
      }
    }
  ],
  "page": {"limit": 50, "returned": 1, "has_more": false, "next_cursor": null}
}
```

- 기본 page size는 50, 최대 100이다.
- `metadata`는 content body를 담지 않고, id/count/metric 같은 structural fact만 담는다.

## Sync space file changes

```http
GET /api/v1/spaces/{space_id}/file-change-sync?after_id=2048&limit=100
```

UI 동기화 전용 forward stream이다. `after_id`를 생략한 첫 요청은 현재 latest event를 baseline으로 설정하고 과거 이력을 반환하지 않는다. 이후 요청은 해당 ID 뒤의 event를 `id asc`로 반환한다.

```json
{
  "changes": [
    {
      "id": 2049,
      "node_id": "node-id",
      "op_type": "text.write",
      "item_kind": "text",
      "affected_parent_ids": ["parent-node-id"],
      "parent_scope_known": true,
      "path_changed": false,
      "subtree_changed": false
    }
  ],
  "next_after_id": 2049,
  "has_more": false,
  "resync_required": false
}
```

Rules:

- 기본 page size는 50, 최대 100이다.
- Space file mutation은 event insert와 commit까지 같은 Space lock으로 직렬화되므로 `id asc`가 해당 Space의 commit 순서다.
- `has_more=true`이면 `next_after_id`로 다음 page를 이어서 읽는다.
- 모든 page를 적용한 뒤 client token을 전진시킨다.
- `affected_parent_ids`는 metadata를 해석하지 않아도 되는 typed cache invalidation 범위다.
- `parent_scope_known=false`는 과거 event에 parent 정보가 없어 children-family fallback이 필요함을 뜻한다.
- `path_changed`는 create/copy/rename/move/delete로 path resolution 결과가 바뀌었음을 뜻한다.
- `subtree_changed`는 folder rename/move 또는 recursive delete로 descendant cache도 바뀌었음을 뜻한다.
- token event가 더 이상 해당 Space에 없으면 event를 반환하지 않고 `resync_required=true`와 새 baseline을 반환한다.
