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
spaces(owner_user_id=caller, sort_order=0)
root node '/'
```

Space name은 1~63자이며, 첫 글자는 영문/숫자이고 이후 글자는 영문/숫자/`.`/`_`/`-`만 허용한다.

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
