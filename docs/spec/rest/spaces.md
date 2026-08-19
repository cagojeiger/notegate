# REST Spaces

Space는 user가 소유한 중앙 저장 범위다.

## List spaces

```http
GET /api/v1/spaces?limit=50&cursor=...
```

- User caller: 자신이 소유한 live space 목록.
- Agent caller: 자신에게 연결된 live space 목록.
- `navigation_pinned`는 Workbench 탐색 영역에 Space를 계속 표시할지 나타낸다.
- `user_mcp_enabled`는 owner user의 MCP 접근 범위에 Space를 포함할지 나타낸다.
- REST 목록은 두 상태와 관계없이 caller가 볼 수 있는 모든 live Space를 반환한다.
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

새 Space는 현재 목록의 마지막에 추가되며 탐색 영역에는 고정되고 User MCP에는 노출되지 않는다.

Space name은 1~63자 Unicode 문자열이다. 한글과 내부 공백은 허용한다. `/`, `:`, control char, 앞뒤 공백, `.`, `..`는 허용하지 않는다. `:`는 MCP compact target(`<space>:/path`) 파싱을 위해 예약한다.

## Get space

```http
GET /api/v1/spaces/{space_id}
```

Caller가 볼 수 있는 space 하나를 반환한다.

Space 응답은 다음 정책과 capability를 포함한다.

```ts
type SpacePolicy = {
  navigation_pinned: boolean
  user_mcp_enabled: boolean
  default_search_enabled: boolean
  default_text_encryption_enabled: boolean
  features: {
    text_encryption: boolean
  }
}
```

기본값은 새 node 생성 시에만 복사한다. `default_search_enabled`는 새 folder/text/file에 적용하고 `default_text_encryption_enabled`는 새 Text에만 적용한다.

## Update space

```http
PATCH /api/v1/spaces/{space_id}
```

Owner user만 가능하다.

```json
{
  "name":"personal",
  "sort_order":0,
  "navigation_pinned":true,
  "user_mcp_enabled":false,
  "default_search_enabled":true,
  "default_text_encryption_enabled":false
}
```

필드 하나 이상을 보낸다. `sort_order`는 중복 가능하며 동률은 `name`, `id`로 안정 정렬한다. 기본값 변경은 기존 node를 갱신하지 않는다. `default_text_encryption_enabled=true`는 Space owner의 `text_encryption` capability가 필요하다.

- `navigation_pinned`는 데스크톱 rail과 모바일 Space 전환 목록의 표시를 제어한다. `false`인 Space는 Library에서 열어도 탐색 영역에 임시로 추가하지 않는다.
- `user_mcp_enabled: true`는 owner user의 MCP 목록과 접근 범위에 포함한다.
- `user_mcp_enabled: false`는 owner user의 MCP에서 목록·조회·검색·쓰기·전송을 모두 숨긴다.
- 두 상태는 서로 독립적이며 Agent 권한에는 영향을 주지 않는다. Agent는 명시적인 Space connection만 따른다.

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

Owner user만 가능하다. 요청은 해당 Space의 reconciliation job을 만들고 `202 Accepted`와 `{"status":"queued","job_id":"..."}`를 반환한다. `job_id`는 `GET /api/v1/me/jobs/{job_id}`로 상태를 추적할 때 사용한다. 같은 Space의 job이 이미 있으면 `409 usage_reconciliation_pending`, 최근 reconciliation 완료 후 1시간 cooldown이면 `409 usage_reconciliation_cooldown`을 반환한다. Client는 `GET /api/v1/me/usage`의 `reconciliation_pending`과 `reconciliation_available_at`을 실행 가능 상태의 기준으로 사용한다. 실제 COUNT/SUM은 background worker가 순차적으로 실행한다.
