# MCP Search

MCP search는 path-first이다. Tool은 `workspace`, `workspace_id`, `target`, 또는 `README.md`의 single visible workspace fallback으로 workspace를 선택한다.

`path`는 path substring query가 아니라 scope path다. Folder scope는 해당 folder subtree를 검색하고, document scope는 해당 document 하나만 검색한다. Scope path가 live node로 해석되지 않으면 `invalid params`와 `data.kind=not_found`를 반환한다.

`target`은 `workspace`와 `path`를 함께 전달하는 축약형이다.

```json
{"target": "personal:/projects", "q": "note"}
```

## `files_find`

선택된 workspace 안에서 node name metadata를 검색한다.

Input:

```json
{
  "workspace": "personal",
  "q": "note",
  "path": "/projects",
  "kind": "document",
  "limit": 50,
  "cursor": "opaque-cursor"
}
```

`q`는 single-line, non-empty, 최대 256 characters다.

Output:

```json
{
  "workspace": "personal",
  "items": [
    {
      "path": "/projects/note.md",
      "name": "note.md",
      "kind": "document",
      "node_id": "node-id",
      "has_children": false,
      "sort_order": 0,
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

## `files_grep`

Markdown body line을 검색한다.

Input:

```json
{
  "workspace": "personal",
  "q": "auth",
  "path": "/projects",
  "context": 2,
  "limit": 20,
  "cursor": "opaque-cursor"
}
```

`q`는 single-line, non-empty, 최대 256 characters다.

Branching:

```text
missing context -> default context
context < 0     -> 0
context > max   -> max context
```

Output:

```json
{
  "workspace": "personal",
  "matches": [
    {
      "workspace": "personal",
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

`files_grep`은 `node_id`를 반환하지 않는다. MCP caller는 반환된 `path`를 다음 command target으로 사용한다.
