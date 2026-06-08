# MCP Search

MCP search is path-first. Tools select a workspace through `workspace`, `workspace_id`, `target`, or the single visible workspace fallback described in `README.md`.

The `path` field is a scope path, not a path substring query. A folder scope searches that folder subtree. A document scope searches that document only. An unresolved scope path returns `invalid params` with `data.kind=not_found`.

## `files_find`

Find nodes by name metadata under an optional scope path.

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

`q` is single-line, non-empty, and at most 256 characters.

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

Search Markdown body lines.

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

`q` is single-line, non-empty, and at most 256 characters.

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

`files_grep` does not return `node_id`; MCP callers should use the returned path as the next command target.
