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

User caller만 workspace를 생성할 수 있다. 생성된 workspace의 `created_by` user가 영구 lifecycle owner이며, 별도 `workspace_access` owner row는 만들지 않는다. Agent caller의 생성 요청은 `403 forbidden`으로 거부한다. 단일/default workspace 제한은 없다. Workspace name은 `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$` 형식이어야 한다. User creator account는 최대 `20`개의 live workspace를 소유할 수 있다.

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

Lifecycle owner 권한이 필요하다.

### Delete workspace

```http
DELETE /api/v1/workspaces/{workspace_id}
```

Lifecycle owner 권한이 필요하다. Workspace 삭제는 soft delete이며 `deleted_at`, `deleted_by`, `purge_after`를 설정한다. 내부 access/node/document row는 즉시 갱신하지 않고, `purge_after` 이후 background purge가 hard delete할 때 DB cascade로 제거한다.
