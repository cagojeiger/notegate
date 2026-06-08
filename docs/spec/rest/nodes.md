# REST Nodes

## Nodes

All node APIs require the `{workspace_id}` URL segment and validate that the node belongs to that
workspace. Node ids are stable across rename and move.

### Resolve path

```http
GET /api/v1/workspaces/{workspace_id}/paths/resolve?path=/projects/note.md
```

Used by command palette, deep links, breadcrumbs, and search-result navigation. The response is a
`Node output`. Deleted nodes are not resolved.

### Get node

```http
GET /api/v1/workspaces/{workspace_id}/nodes/{node_id}
```

Returns node metadata. Useful for refresh after optimistic UI updates. The `path` field is derived from the parent chain, not stored as canonical full path.

### List children

```http
GET /api/v1/workspaces/{workspace_id}/nodes/{node_id}/children?limit=100&cursor=...
```

Rules:

- `{node_id}` must be a folder.
- Only direct children are returned.
- Pagination is required.
- Default ordering is `(sort_order, name, id)`.

```json
{
  "parent": {"id": "folder-id", "path": "/projects"},
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false}
}
```

### Create node

```http
POST /api/v1/workspaces/{workspace_id}/nodes
```

```json
{
  "parent_id": "folder-id",
  "kind": "document",
  "name": "note.md",
  "content_md": "# optional initial content\n"
}
```

Rules:

- `kind='folder'` ignores `content_md`.
- Node name must match `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`; `/`, `:`, spaces, control characters, `.`, and `..` are rejected.
- Folder name length must be `<= 128` chars.
- Document filename length must be `<= 128` chars including `.md`; title stem length must be `<= 125` chars excluding `.md`.
- Resulting depth must be `<= 5`.
- Parent folder must have fewer than `200` live direct children before create.
- Workspace must have fewer than `10000` live nodes before create.
- `kind='document'` requires `.md` name and creates a `documents` row.
- Document create additionally requires fewer than `5000` live documents in the workspace.
- `kind='folder'` name must not end with `.md`.
- Same parent folder cannot contain duplicate live names.

### Update node metadata

```http
PATCH /api/v1/workspaces/{workspace_id}/nodes/{node_id}
```

```json
{
  "name": "renamed.md",
  "sort_order": 10
}
```

Use this for rename or custom ordering. Root rename is rejected.

### Move node

```http
POST /api/v1/workspaces/{workspace_id}/nodes/{node_id}/move
```

```json
{
  "new_parent_id": "folder-id",
  "new_name": "optional-renamed.md",
  "expected_parent_id": "old-parent-id"
}
```

Rules:

- Root move is rejected.
- Moving a folder into itself or descendants is rejected.
- Resulting subtree depth must be `<= 5`.
- Destination parent must have fewer than `200` live direct children unless the move stays within the same parent.
- Move/rename updates only the moved node parent/name metadata; descendant paths are derived and must not be rewritten.
- The destination sibling name must be unique.
- `expected_parent_id` is optional but recommended for optimistic UI safety.

### Delete node

```http
DELETE /api/v1/workspaces/{workspace_id}/nodes/{node_id}?recursive=true
```

Rules:

- Root delete is rejected.
- Document delete does not require `recursive=true`.
- Folder delete requires `recursive=true`.
- Delete is soft delete on `nodes`; deleted nodes disappear from normal list/search/resolve.
- Deleted nodes are not recoverable through the current REST contract.
- Deleted nodes are retained until `purge_after`; after that an internal purge job may hard-delete them.
