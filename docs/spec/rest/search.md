# REST Search

Search는 workspace-scoped다. Authorization은 요청 시작 시 live access 기준으로 한 번 검증하고, 검색 쿼리는 이후 `workspace_id`로 필터링하며 삭제된 node를 제외한다.

The `path` request field is a scope path, not a path substring query. A folder scope searches that folder subtree. A document scope searches that document only. An unresolved scope path returns `404 not_found`.

## Find nodes

```http
POST /api/v1/workspaces/{workspace_id}/search/find
```

Request:

```json
{
  "q": "note",
  "path": "/projects",
  "kind": "document",
  "limit": 50,
  "cursor": "opaque-cursor"
}
```

Returns node matches by name and optional kind. `q` is single-line, non-empty, and at most 256 characters.

Response:

```json
{
  "items": [
    {
      "id": "node-id",
      "workspace_id": "workspace-id",
      "parent_id": "parent-node-id",
      "name": "note.md",
      "kind": "document",
      "path": "/projects/note.md",
      "sort_order": 0,
      "has_children": false,
      "created_by": { "id": "account-id", "kind": "user", "display_name": "Kang" },
      "updated_by": { "id": "account-id", "kind": "user", "display_name": "Kang" },
      "created_at": "2026-06-08T00:00:00Z",
      "updated_at": "2026-06-08T00:00:00Z"
    }
  ],
  "page": {
    "limit": 50,
    "returned": 1,
    "has_more": false,
    "next_cursor": null
  }
}
```

## Grep content

```http
POST /api/v1/workspaces/{workspace_id}/search/grep
```

Request:

```json
{
  "q": "auth",
  "path": "/projects",
  "context": 2,
  "limit": 20,
  "cursor": "opaque-cursor"
}
```

Returns line matches with `node_id`, current path, line number, and context lines. `q` is single-line, non-empty, and at most 256 characters.

Response:

```json
{
  "matches": [
    {
      "node_id": "node-id",
      "path": "/projects/note.md",
      "line_no": 12,
      "line": "auth config",
      "before": ["previous line"],
      "after": ["next line"]
    }
  ],
  "page": {
    "limit": 20,
    "returned": 1,
    "has_more": false,
    "next_cursor": null
  }
}
```
