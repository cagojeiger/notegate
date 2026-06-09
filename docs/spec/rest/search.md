# REST Search

Search는 workspace 범위 API다. Authorization은 요청 시작 시 live workspace와 effective role 기준으로 한 번 검증하고, 검색 쿼리는 이후 `workspace_id`로 필터링하며 삭제된 node를 제외한다.

`path` request field는 path substring query가 아니라 검색 scope path다. Folder scope는 해당 folder subtree를 검색하고, document scope는 해당 document만 검색한다. Resolve되지 않는 scope path는 `404 not_found`를 반환한다.

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
