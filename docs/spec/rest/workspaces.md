# REST Workspaces

## Workspaces

### List workspaces

```http
GET /api/v1/workspaces?limit=50&cursor=...
```

호출자가 live access를 가진 workspace 목록을 반환한다. Default limit은 `50`, max limit은 `100`이다.

### Create workspace

```http
POST /api/v1/workspaces
```

```json
{
  "name": "personal"
}
```

Workspace를 생성하고, 생성자에게 `owner` access를 부여하며, canonical root node `/`를 만든다. 단일/default workspace 제한은 없다. Workspace name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이어야 한다. Owner account는 최대 `20`개의 active workspace를 소유할 수 있다.

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

`owner` 권한이 필요하다.

### Delete workspace

```http
DELETE /api/v1/workspaces/{workspace_id}
```

`owner` 권한이 필요하다. Workspace 삭제는 정상 지원되는 작업이며 workspace boundary, access row, node, document를 DB cascade로 삭제한다.
