# REST Workspaces

## Workspaces

### List workspaces

```http
GET /api/v1/workspaces?limit=50&cursor=...
```

Returns workspaces where the caller has non-revoked `workspace_access`. Default limit is `50`; max limit is `100`.

### Create workspace

```http
POST /api/v1/workspaces
```

```json
{
  "name": "personal"
}
```

Creates a workspace, grants the creator `owner`, and creates the canonical root node `/`. There is no single/default workspace restriction. Workspace name must match `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$`. The owner account owns at most `20` active workspaces.

### Get workspace

```http
GET /api/v1/workspaces/{workspace_id}
```

Returns workspace metadata, caller role, and derived `root_node_id`.

### Rename workspace

```http
PATCH /api/v1/workspaces/{workspace_id}
```

```json
{
  "name": "personal"
}
```

Requires `owner`.

### Delete workspace

```http
DELETE /api/v1/workspaces/{workspace_id}
```

Requires `owner`. Workspace deletion is a normal supported operation and deletes the workspace boundary, access rows, nodes, and documents according to the DB cascade/retention policy.
