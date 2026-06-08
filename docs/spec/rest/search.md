# REST Search

## Search

Search is workspace-scoped. Authorization is checked once against `workspace_access`; search queries
then filter by `workspace_id` and exclude deleted nodes.

### Find nodes

```http
POST /api/v1/workspaces/{workspace_id}/search/find
```

```json
{
  "q": "note",
  "path": "/projects",
  "kind": "document",
  "limit": 50,
  "cursor": "opaque-cursor"
}
```

Returns node matches by name and optional kind. The `path` request field is a scope path, not a path substring query.

### Grep content

```http
POST /api/v1/workspaces/{workspace_id}/search/grep
```

```json
{
  "q": "auth",
  "path": "/projects",
  "context": 2,
  "limit": 20,
  "cursor": "opaque-cursor"
}
```

Returns line matches with `node_id`, current path, line number, and context lines.
