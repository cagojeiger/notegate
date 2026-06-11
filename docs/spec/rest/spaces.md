# REST Spaces

Space는 user가 소유한 중앙 저장 범위다.

## List spaces

```http
GET /api/v1/spaces?limit=50&cursor=...
```

- User caller: 자신이 소유한 live space 목록.
- Agent caller: 자신에게 연결된 live space 목록.

## Create space

```http
POST /api/v1/spaces
```

```json
{"name":"personal"}
```

User caller만 가능하다. 생성 side effect:

```text
spaces(owner_user_id=caller)
root node '/'
```

## Get space

```http
GET /api/v1/spaces/{space_id}
```

Caller가 볼 수 있는 space 하나를 반환한다.

## Rename space

```http
PATCH /api/v1/spaces/{space_id}
```

Owner user만 가능하다.

## Delete space

```http
DELETE /api/v1/spaces/{space_id}
```

Owner user만 가능하다. Space는 soft delete 후 purge 대상이 된다.
