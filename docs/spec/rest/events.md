# REST Events

Event history는 self-review를 위한 이력이다. User caller는 자기 계정과 space에 어떤 관리 변경과 파일 변경이 있었는지 확인하고, MCP·Command API 호출 목적과 결과를 검토한다. 스키마와 capture 계약은 `docs/spec/event-logging.md`가 정본이다.

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

## List my command invocations

```http
GET /api/v1/me/command-invocations?surface=mcp&limit=50&cursor=...
```

User caller만 가능하다. `surface=mcp|cli`는 필수이며 한 surface의 caller 소유 호출만 `created_at desc, id desc` 순으로 반환한다. Cursor도 surface에 묶이므로 다른 tab에서 재사용할 수 없다. redacted `input`과 `response`, 짧은 `purpose`, tool/op, 성공 여부, 안정적인 error code, 실행 시간을 제공한다. `read op=changes`는 검증된 `space_name` summary도 반환한다. `me` 또는 입력 검증 실패는 `purpose`가 `null`일 수 있고, response snapshot이 없는 행은 `response=null`이다.

```json
{
  "command_invocations": [
    {
      "id": 3042,
      "created_at": "2026-08-02T09:12:00Z",
      "actor_account_id": "account-id",
      "actor": {"id": "account-id", "kind": "agent", "display_name": "Codex"},
      "caller_kind": "agent",
      "surface": "mcp",
      "tool": "read",
      "op": "changes",
      "purpose": "Review recent changes",
      "space_name": "Research",
      "input": {
        "purpose": "Review recent changes",
        "op": "changes",
        "target": "Research:/"
      },
      "response": {
        "kind": "complete",
        "is_error": false,
        "content_blocks_omitted": 1,
        "result": {
          "space": "Research",
          "events": [],
          "checkpoint_cursor": {
            "_redacted": true,
            "category": "opaque_cursor",
            "value_type": "string"
          }
        }
      },
      "outcome": "success",
      "error_code": null,
      "duration_ms": 17
    }
  ],
  "page": {"limit": 50, "returned": 1, "has_more": false, "next_cursor": null}
}
```

- 기본 page size는 50, 최대 100이다.
- `actor`는 현재 조회 가능한 account reference이며, account가 purge되었으면 `null`일 수 있다.
- `surface`는 `mcp` 또는 `cli`이며 요청한 surface와 항상 같다.
- `space_name`은 `read op=changes`에서만 값이 있고 다른 호출에서는 `null`이다. Target/path 등 허용된 입력 metadata는 `input`에 포함된다.
- 입력·응답의 본문, 검색어, cursor, credential, PII와 자유 형식 오류 문구는 marker로 대체되며 알 수 없는 field 값은 반환하지 않는다.
- 이 endpoint는 browser self-review용이다. MCP나 CLI에 호출 이력 조회 command는 추가하지 않는다.

## List my background jobs

```http
GET /api/v1/me/jobs?limit=50&cursor=...
GET /api/v1/me/jobs/{job_id}
```

User caller만 가능하다. 공통 queue envelope에서 `history_visibility=visible`, `history_owner_account_id=caller`로 등록된 job과 caller가 소유한 Space의 `link_graph_project_nodes` job을 `created_at desc, job_id desc` 순으로 반환한다. 링크 job의 Space 문맥은 job payload의 `space_id`와 현재 Space ownership으로 구한다. 목록은 상태와 안정적인 error code를 제공하고, 단건 조회는 attempt 이력을 함께 반환한다. 표시 문맥이 없는 job은 `context_kind`, `context_id`, `context_label`이 `null`이다.

- 기본 page size는 50, 최대 100이다.
- `queued`와 `running`이 활성 상태이며 UI는 활성 작업이 있을 때만 목록을 polling한다.
- Job은 `created_at`을 queued 시각, `completed_at`을 terminal 완료 시각으로 표시한다. 개별 attempt는 `started_at`과 `finished_at`으로 실제 실행 구간을 표시한다.
- `claimed_by`, `worker_id`, claim token, payload, 자유 형식 error message는 응답하지 않는다.
- purge, object cleanup, queue maintenance reconciliation, metadata write-behind, metrics upkeep처럼 공통 queue 밖에서 실행되는 운영 task는 이 이력에 포함하지 않는다.

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
      "subtree_changed": false,
      "write_lock_changed": false
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
- `parent_scope_known=false`는 parent 범위를 알 수 없어 children-family fallback이 필요함을 뜻한다.
- `path_changed`는 create/copy/rename/move/delete로 path resolution 결과가 바뀌었음을 뜻한다.
- `subtree_changed`는 folder rename/move 또는 recursive delete로 descendant cache도 바뀌었음을 뜻한다.
- `write_lock_changed`는 node의 직접 write lock 설정이 바뀌었음을 뜻한다. 대상이 folder이면 하위 node의 상속 lock source가 바뀌므로 client는 node detail cache를 무효화한다.
- token event가 더 이상 해당 Space에 없으면 event를 반환하지 않고 `resync_required=true`와 새 baseline을 반환한다.
