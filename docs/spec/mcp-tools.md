# Files MCP tools

MCP tools are LLM/CLI-friendly. They use path-first inputs and hide node_id workflow from the
model whenever possible.

Surface:

```text
/mcp
```

Auth:

```text
Bearer token only
```

Preview browser cookies are not accepted by MCP.

## Tool set

```text
files_ls
files_stat
files_mkdir
files_touch
files_cat
files_write
files_mv
files_rm
files_find
files_grep
```

## Common output rules

- Paths are canonical absolute paths.
- List/search tools return page metadata.
- Read/search tools may return `truncated=true` and a next cursor/range hint.
- Internal errors are redacted.
- Error data should include actionable hints when possible.

## `files_ls`

List direct children of a folder.

Input:

```json
{
  "path": "/projects",
  "limit": 100
}
```

Output:

```json
{
  "path": "/projects",
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false}
}
```

## `files_stat`

Return metadata for a path.

Input:

```json
{"path": "/projects/note.md"}
```

## `files_mkdir`

Create a folder at path.

Input:

```json
{"path": "/projects/notes"}
```

Equivalent CLI intent:

```sh
mkdir /projects/notes
```

## `files_touch`

Create an empty Markdown document.

Input:

```json
{"path": "/projects/note.md"}
```

Equivalent CLI intent:

```sh
touch /projects/note.md
```

## `files_cat`

Read a Markdown document with range limits.

Input:

```json
{
  "path": "/projects/note.md",
  "start_line": 1,
  "max_lines": 200,
  "max_bytes": 65536
}
```

Output includes:

```json
{
  "path": "/projects/note.md",
  "content_md": "# Note\n",
  "start_line": 1,
  "returned_lines": 1,
  "truncated": false
}
```

## `files_write`

Replace a Markdown document.

Input:

```json
{
  "path": "/projects/note.md",
  "content_md": "# Updated\n",
  "create": false
}
```

Rules:

- `create=false` requires an existing document.
- `create=true` may create a missing document parented by `dirname(path)`.
- Oversized content is rejected.

## `files_mv`

Move or rename a path.

Input:

```json
{
  "source_path": "/projects/note.md",
  "destination_path": "/archive/note.md"
}
```

Rules:

- `source_path == destination_path` is a no-op success.
- Destination parent must exist and be a folder.
- Existing destination sibling with the same name is conflict.
- Moving a folder into itself or a descendant is conflict.

## `files_rm`

Soft-delete a path.

Input:

```json
{
  "path": "/projects/old",
  "recursive": true
}
```

Rules:

- Folder deletion requires `recursive=true`.
- Root deletion is forbidden.
- Large subtree deletion may be rejected with a narrowing/async hint.

## `files_find`

Find nodes by name/path metadata.

Input:

```json
{
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
  "q": "auth",
  "path": "/projects",
  "context": 2,
  "limit": 20
}
```

Output result item:

```json
{
  "path": "/projects/note.md",
  "line_no": 12,
  "line": "auth config",
  "before": ["..."],
  "after": ["..."]
}
```
