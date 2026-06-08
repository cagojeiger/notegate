# MCP Files

## `files_ls`

List direct children of a folder.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects",
  "limit": 100
}
```

Output:

```json
{
  "workspace": "personal",
  "path": "/projects",
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false, "next_cursor": null}
}
```

## `files_stat`

Return metadata for a path.

Input:

```json
{"workspace": "personal", "path": "/projects/note.md"}
```

## `files_mkdir`

Create a folder at path.

Input:

```json
{"workspace": "personal", "path": "/projects/notes"}
```

Equivalent CLI intent:

```sh
mkdir /projects/notes
```

## `files_touch`

Create an empty Markdown document.

Input:

```json
{"workspace": "personal", "path": "/projects/note.md"}
```

Equivalent CLI intent:

```sh
touch /projects/note.md
```

## `files_read`

Read a Markdown document with range limits.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "start_line": 1,
  "max_lines": 200,
  "max_bytes": 65536,
  "if_none_match_sha256": "optional"
}
```

Output includes:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "content_md": "# Note\n",
  "content_sha256": "sha256...",
  "byte_len": 7,
  "line_count": 1,
  "start_line": 1,
  "end_line": 1,
  "returned_lines": 1,
  "truncated": false,
  "next_start_line": null
}
```

`if_none_match_sha256` branch:

```text
missing or non-matching hash -> return bounded content
matching current hash        -> return metadata without content
```

Unchanged output:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "unchanged": true,
  "content_returned": false,
  "content_sha256": "sha256..."
}
```

## Mutation safety contract

`files_write` and `files_patch` are mutating tools. They fail closed when the target is stale,
ambiguous, or unsafe to edit.

Common mutation contract:

```text
one document/node operation -> one transaction
expected_sha256 mismatch   -> conflict before mutation
successful mutation        -> return content_sha256, byte_len, line_count, current path
failed mutation            -> no partial persistence
persisted hash mismatch    -> internal error, not success
```

Error messages include actionable hints such as `read the document again`, `old_text matched multiple times`, or `use files_write for full rewrite`.

`files_patch` uses exact matching, not fuzzy matching, so MCP document edits stay predictable and auditable.

## `files_write`

Replace a Markdown document.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "content_md": "# Updated\n",
  "create": false,
  "expected_sha256": "optional"
}
```

Rules:

- `create=false` requires an existing document.
- `create=true` creates a missing document parented by `dirname(path)`.
- Content over `524288` bytes or `2000` lines is rejected with a split-document hint.
- Creating a new document is rejected if workspace live documents would exceed `5000`.
- Writing is rejected if workspace live document content would exceed `268435456` bytes.
- If `expected_sha256` is present and does not match the current document, return conflict.
- Successful writes return the new `content_sha256`, `byte_len`, and `line_count`.

## `files_patch`

Apply exact targeted replacements to one Markdown document.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "edits": [
    {
      "old_text": "before",
      "new_text": "after"
    }
  ],
  "expected_sha256": "optional"
}
```

Rules:

- `edits` must be non-empty.
- `old_text` must be non-empty.
- `old_text` and `new_text` must not be identical; no-op patches are rejected.
- Each `old_text` must match exactly once in the original document.
- Matching is exact against stored Markdown text; no fuzzy, whitespace-normalized, Unicode-normalized, or case-insensitive matching.
- If `old_text` has zero matches, return conflict and suggest re-reading or searching the current document.
- If `old_text` has multiple matches, return conflict and ask for more surrounding context.
- Multiple edits are matched against the original document, not incrementally against prior edit results.
- Overlapping or nested edit ranges are rejected.
- All edits apply atomically; if one edit is invalid, none are persisted.
- Line endings are preserved by substring replacement; the server does not globally normalize line endings during patch.
- Resulting content over `524288` bytes or `2000` lines is rejected.
- Patches that would make workspace live document content exceed `268435456` bytes are rejected.
- Use `files_write` for complete rewrites or when a stable unique anchor is hard to provide.
- If `expected_sha256` is present and does not match the current document, return conflict before matching.

Output includes:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md",
  "patched": true,
  "edits_applied": 1,
  "content_sha256": "sha256...",
  "previous_sha256": "sha256...",
  "byte_len": 5,
  "line_count": 1,
  "diff": "--- before\n+++ after\n..."
}
```

## `files_mv`

Move or rename a path.

Input:

```json
{
  "workspace": "personal",
  "source_path": "/projects/note.md",
  "destination_path": "/archive/note.md"
}
```

Rules:

- `source_path == destination_path` is a no-op success.
- Destination parent exists and is a folder -> continue; otherwise conflict/not_found.
- Existing destination sibling with the same name is conflict.
- Moving a folder into itself or a descendant is conflict.
- Folder move/rename updates only the moved node; descendant paths are derived from the parent chain.

## `files_rm`

Soft-delete a path.

Input:

```json
{
  "workspace": "personal",
  "path": "/projects/old",
  "recursive": true
}
```

Rules:

- Folder deletion requires `recursive=true`.
- Root deletion is forbidden.
- Subtree larger than the synchronous delete limit is rejected with a narrowing hint.

## `files_restore`

Restore a soft-deleted node by id.

Deleted nodes do not resolve by path, so restore is the one file tool that uses
`node_id` as its primary target.

Input:

```json
{
  "workspace": "personal",
  "node_id": "deleted-node-id"
}
```

Rules:

- Requires `editor`.
- Restores the deleted node and the deleted subtree beneath it.
- Ancestors live -> restore; deleted ancestor -> conflict with restore-ancestor hint.
- Restore re-checks sibling-name uniqueness, destination fanout, and max depth.
