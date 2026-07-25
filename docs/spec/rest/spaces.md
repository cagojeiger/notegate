# REST Spaces

Space는 user가 소유한 중앙 저장 범위다.

## List spaces

```http
GET /api/v1/spaces?limit=50&cursor=...
```

- User caller: 자신이 소유한 live space 목록.
- Agent caller: 자신에게 연결된 live space 목록.
- `pinned`는 owner user의 MCP 공개 상태다. REST 목록에는 Pinned와 Unpinned를 모두 반환한다.
- 정렬: `sort_order ASC, name ASC, id ASC`.
- Pagination: opaque `cursor`; client는 해석하지 않고 다음 호출에 그대로 전달한다.

## Create space

```http
POST /api/v1/spaces
```

```json
{"name":"personal"}
```

User caller만 가능하다. 생성 side effect:

```text
spaces(owner_user_id=caller, sort_order=max(owner live sort_order)+1000)
root node '/'
space_usage(live_node_count=1, live_text_bytes=0, live_file_bytes=0)
```

즉 새 space는 기본적으로 현재 목록의 마지막에 Unpinned 상태로 추가된다.

Space name은 1~63자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, `:`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다. `:`는 MCP compact target(`<space>:/path`) 파싱을 위해 예약한다.

## Get space

```http
GET /api/v1/spaces/{space_id}
```

Caller가 볼 수 있는 space 하나를 반환한다.

## Update space

```http
PATCH /api/v1/spaces/{space_id}
```

Owner user만 가능하다.

```json
{"name":"personal","sort_order":0,"pinned":true}
```

`name`, `sort_order`, `pinned` 중 하나 이상을 보낸다. `sort_order`는 중복 가능하며 동률은 `name`, `id`로 안정 정렬한다.

- `pinned: true`: owner user의 MCP 목록과 접근 범위에 포함한다.
- `pinned: false`: owner user의 MCP에서 목록·조회·검색·쓰기·전송을 모두 숨긴다.
- Pin은 Agent 권한에 영향을 주지 않는다. Agent는 명시적인 Space connection만 따른다.

## Reorder spaces

```http
POST /api/v1/spaces:reorder
```

```json
{
  "updates": [
    {"space_id":"...","sort_order":1000},
    {"space_id":"...","sort_order":2000}
  ]
}
```

Owner user만 가능하다. 요청에 포함된 Space의 순서를 한 트랜잭션에서 변경하고 `204 No Content`를 반환한다. Space ID가 중복되거나 비어 있으면 `400`, 하나라도 caller 소유의 live Space가 아니면 전체 요청을 롤백하고 `404`를 반환한다.

## Delete space

```http
DELETE /api/v1/spaces/{space_id}
```

Owner user만 가능하다. Space는 soft delete 후 purge 대상이 된다.

## Request usage reconciliation

```http
POST /api/v1/spaces/{space_id}/usage/reconcile
```

Owner user만 가능하다. 요청은 해당 Space의 reconciliation job을 만들고 `202 Accepted`와 `{"status":"queued"}`를 반환한다. 같은 Space의 job이 이미 있으면 `409 usage_reconciliation_pending`, 최근 reconciliation 완료 후 1시간 cooldown이면 `409 usage_reconciliation_cooldown`을 반환한다. 실제 COUNT/SUM은 background worker가 순차적으로 실행한다.
