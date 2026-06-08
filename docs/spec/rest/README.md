# notegate REST API contract

REST is the browser/UI resource API. It uses `node_id` for tree operations so UI selection remains stable across rename and move.

Base paths:

```text
/api/v1      resource API
/auth/*      browser OAuth session flow
/.well-known OAuth discovery metadata
```

## Category map

| Category | Scope | Base path | Contract file |
|---|---|---|---|
| Auth | global | `/auth/*`, `/.well-known/*` | [auth.md](auth.md) |
| Identity | global | `/api/v1/me` | [identity.md](identity.md) |
| Workspaces | global | `/api/v1/workspaces` | [workspaces.md](workspaces.md) |
| Access | workspace | `/api/v1/workspaces/{workspace_id}/access` | [access.md](access.md) |
| Nodes | workspace | `/api/v1/workspaces/{workspace_id}/nodes` | [nodes.md](nodes.md) |
| Documents | workspace | `/api/v1/workspaces/{workspace_id}/documents` | [documents.md](documents.md) |
| Search | workspace | `/api/v1/workspaces/{workspace_id}/search` | [search.md](search.md) |
| Agents | global | `/api/v1/agents` | [agents.md](agents.md) |
| Errors | global | all REST endpoints | [errors.md](errors.md) |

## Request identity

Contract:

```text
browser login via authgate -> user account
bearer OAuth token         -> user account
API key / agent key        -> agent account
```

Branching:

```text
missing/invalid auth              -> 401
valid authgate token, no account  -> 403
inactive account                  -> 403
client-supplied caller id         -> ignored; caller comes from auth
```

## Workspace authorization

Contract:

```text
viewer = list/stat/read/find/grep
editor = viewer + write/patch/mkdir/touch/move/delete/restore
owner  = editor + workspace access management
```

Branching:

```text
workspace_id not visible to caller -> 404
node belongs to another workspace  -> 404
role below required level          -> 403
```

## File tree contract

Contract:

```text
workspace root node exists for every workspace
root_node_id is returned on workspace output
/files/root endpoint does not exist
file/folder ACL does not exist; role is workspace-scoped
nodes/documents/search implement the product files concept
```

Branching:

```text
invalid workspace/node name -> 400
root move/delete/rename     -> 409 or 400 by operation contract
```

## Pagination contract

Shape:

```json
{
  "page": {
    "limit": 100,
    "returned": 100,
    "has_more": true,
    "next_cursor": "opaque-cursor"
  }
}
```

Branching:

```text
missing limit    -> endpoint default
limit < 1        -> 1
limit > max      -> max
malformed limit  -> 400
malformed cursor -> 400
```

Cursor contract:

```text
client receives cursor -> pass it back unchanged
client changes cursor  -> invalid cursor or undefined page position
```

## Common output shapes

### Account ref

```json
{
  "id": "account-id",
  "kind": "user",
  "display_name": "Kang"
}
```

### Workspace output

```json
{
  "id": "workspace-id",
  "name": "personal",
  "role": "owner",
  "root_node_id": "root-node-id",
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```

### Node output

```json
{
  "id": "node-id",
  "workspace_id": "workspace-id",
  "parent_id": "parent-id-or-null",
  "name": "note.md",
  "kind": "document",
  "path": "/projects/note.md",
  "sort_order": 0,
  "has_children": false,
  "created_by": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "updated_by": {"id": "account-id", "kind": "agent", "display_name": "research-agent"},
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```

## Error contract

```text
401 -> missing/invalid auth
403 -> authenticated but not allowed or inactive/not registered
404 -> not found or cross-workspace hidden resource
400 -> invalid field/name/path, malformed limit, malformed cursor
409 -> state conflict, quota conflict, stale hash, duplicate destination, subtree too large
500 -> redacted internal error
```
