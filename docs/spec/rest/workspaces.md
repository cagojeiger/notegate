# REST Workspaces

## Workspaces

### List workspaces

```http
GET /api/v1/workspaces?limit=50&cursor=...
```

호출자가 접근 가능한 live workspace 목록을 반환한다. Default limit은 `50`, max limit은 `100`이다.

### Create workspace

```http
POST /api/v1/workspaces
```

```json
{
  "name": "personal"
}
```

User caller만 workspace를 생성할 수 있다. 생성 side effect는 `docs/spec/lifecycle.md`의 Workspace 생성 정책을 따른다. Agent caller의 생성 요청은 `403 forbidden`으로 거부한다. 단일/default workspace 제한은 없다. Workspace name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이어야 한다. User creator account는 최대 `20`개의 live workspace를 생성할 수 있다.

### Get workspace

```http
GET /api/v1/workspaces/{workspace_id}
```

Workspace metadata, caller role, derive된 `root_node_id`를 반환한다.

### Rename workspace

```http
PATCH /api/v1/workspaces/{workspace_id}
```

```json
{
  "name": "personal"
}
```

`owner` role 권한이 필요하다.

### Delete workspace

```http
DELETE /api/v1/workspaces/{workspace_id}
```

`owner` role 권한이 필요하다. Workspace 삭제 side effect는 `docs/spec/lifecycle.md`의 Workspace 삭제 정책을 따른다.
