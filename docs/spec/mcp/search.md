# MCP Search

## `files_find`

Find nodes by name metadata under an optional scope path.

Input:

```json
{
  "workspace": "personal",
  "q": "note",
  "path": "/projects",
  "kind": "document",
  "limit": 50
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
  "limit": 20
}
```

Output result item:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "line_no": 12,
  "line": "auth config",
  "before": ["..."],
  "after": ["..."]
}
```
