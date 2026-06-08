# REST Documents

## Documents

Document APIs operate on a document node id.

### Read document

```http
GET /api/v1/workspaces/{workspace_id}/documents/{node_id}?start_line=1&max_lines=200&max_bytes=65536&if_none_match_sha256=...
```

Returns document metadata and a bounded content slice.

```json
{
  "node": {"id": "node-id", "path": "/projects/note.md", "kind": "document"},
  "document": {
    "node_id": "node-id",
    "content_md": "# Note\n",
    "content_sha256": "sha256...",
    "byte_len": 7,
    "line_count": 1,
    "start_line": 1,
    "end_line": 1,
    "truncated": false,
    "next_start_line": null,
    "updated_by": {"id": "account-id", "kind": "user", "display_name": "Kang"},
    "updated_at": "2026-01-01T00:00:00Z"
  }
}
```

`if_none_match_sha256` branch:

```text
missing or non-matching hash -> return bounded content
matching current hash        -> return metadata without content
```

Unchanged response:

```json
{
  "node": {"id": "node-id", "path": "/projects/note.md", "kind": "document"},
  "document": {
    "node_id": "node-id",
    "unchanged": true,
    "content_returned": false,
    "content_sha256": "sha256...",
    "byte_len": 7,
    "line_count": 1
  }
}
```

### Replace document

```http
PUT /api/v1/workspaces/{workspace_id}/documents/{node_id}
```

```json
{
  "content_md": "# Updated\n",
  "expected_sha256": "previous-sha256"
}
```

Rules:

- Full document replacement.
- Requires `editor`.
- Content over `524288` bytes or `2000` lines is rejected with a split-document hint.
- Writes that would make workspace live document content exceed `268435456` bytes are rejected.
- If `expected_sha256` is present and does not match current content, return `409 conflict`.

### Patch document

```http
PATCH /api/v1/workspaces/{workspace_id}/documents/{node_id}
```

```json
{
  "edits": [
    {"old_text": "before", "new_text": "after"}
  ],
  "expected_sha256": "previous-sha256"
}
```

Rules:

- Requires `editor`.
- `edits` must be non-empty.
- `old_text` must be non-empty.
- `old_text` and `new_text` must not be identical; no-op patches are rejected.
- Each `old_text` must match exactly once in the original document.
- Matching is exact against stored Markdown text; no fuzzy, whitespace-normalized, Unicode-normalized, or case-insensitive matching.
- If `old_text` has zero matches, return `409 conflict` and suggest re-reading/searching the current document.
- If `old_text` has multiple matches, return `409 conflict` and ask for more surrounding context.
- Multiple edits are matched against the original document, not incrementally.
- Overlapping or nested edit ranges are rejected.
- All edits apply atomically; if one edit is invalid, none are persisted.
- Line endings are preserved by substring replacement; the server does not globally normalize line endings during patch.
- Resulting content over `524288` bytes or `2000` lines is rejected.
- Patches that would make workspace live document content exceed `268435456` bytes are rejected.
- Successful response includes the new `content_sha256`, `byte_len`, and `line_count`.
