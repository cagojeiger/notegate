# REST Spaces

Space는 user가 소유한 중앙 저장 범위다.

## List spaces

```http
GET /api/v1/spaces?limit=50&cursor=...
```

- User caller: 자신이 소유한 live space 목록.
- Agent caller: 자신에게 연결된 live space 목록.
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
```

즉 새 space는 기본적으로 현재 목록의 마지막에 추가된다.

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
{"name":"personal","sort_order":0}
```

`name` 또는 `sort_order` 중 하나 이상을 보낸다. `sort_order`는 중복 가능하며 동률은 `name`, `id`로 안정 정렬한다.

## Delete space

```http
DELETE /api/v1/spaces/{space_id}
```

Owner user만 가능하다. Space는 soft delete 후 purge 대상이 된다.

## Request usage reconciliation

```http
POST /api/v1/spaces/{space_id}/usage/reconcile
```

Owner user만 가능하다. 요청은 해당 Space의 `next_reconcile_at`을 현재 시각으로 앞당기고 `202 Accepted`와 `{"status":"queued"}`를 반환한다. 같은 Space의 중복 요청과 cooldown 위반은 거부한다. 실제 COUNT/SUM은 background reconciler가 순차적으로 실행한다.
