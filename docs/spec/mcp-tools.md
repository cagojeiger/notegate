# notegate MCP tool contract

이 문서는 notegate MCP tool의 request/response 계약을 정의한다. MCP tools는 LLM/CLI 친화 surface이며, 가능하면 `node_id` workflow를 숨기고 path-first 입력을 사용한다.

Surface:

```text
/mcp
```

Auth:

```text
Bearer token only
```

MCP accepts bearer credentials only; browser/session cookies are not accepted.

Identity mapping:

```text
MCP OAuth 2.1 via authgate -> user account
API key / agent key        -> agent account
```

Device flow through authgate is also a user login. API keys are always agent
credentials, even when they were created by a user.

## First-time user setup

MCP OAuth login proves an authgate identity. If the local notegate user/account does not exist yet,
the caller must complete browser login once through `/auth/login`, then reconnect the MCP client.

If an authenticated MCP caller has no local account, `/mcp` returns `403 not_registered` with
`login_url` and `mcp_url` onboarding hints.

## Tool set

```text
me
workspaces_list
workspaces_get
files_ls
files_stat
files_mkdir
files_touch
files_read
files_write
files_patch
files_mv
files_rm
files_find
files_grep
```


## Workspace selection

MCP는 여러 workspace에 접근할 수 있어야 한다. File tools are therefore workspace-scoped.

MCP/CLI callers should not need to know UUIDs in the normal path. The canonical MCP selector is a
human-friendly workspace name.

Common file tool input fields:

```json
{
  "workspace": "personal",
  "path": "/projects/note.md"
}
```

MCP tools may also accept a compact target string:

```json
{
  "target": "personal:/projects/note.md"
}
```

`target` parses as `<workspace>:/<path-inside-workspace>`.

Rules:

- `workspace` is the canonical MCP/CLI selector and normally resolves to `workspaces.name`.
- `target` is syntactic sugar for `workspace + path`; structured fields are preferred for API clients, target strings are convenient for LLM/CLI prompts.
- Workspace names are unique per owner account. For a user's own workspaces, name lookup is stable and unambiguous.
- Agents may have access to workspaces from multiple owners. If the same name is visible more than once, return an ambiguity error with matching workspaces and `workspaces_list` guidance.
- `workspace_id` may be accepted as an explicit fallback for ambiguity/debugging, but MCP examples and LLM workflows should prefer `workspace`.
- If the caller has exactly one accessible workspace, `workspace` may be omitted and the server may use that workspace.
- MCP should not rely on a mutable server-side "selected workspace" session. Tool calls should be stateless and replayable.
- Paths are resolved inside the selected workspace only. Cross-workspace path movement is not supported by file tools.


## Name and target grammar

Workspace names and node names are intentionally restricted so MCP/CLI paths remain unambiguous.

```text
workspace name:              ^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$
folder name:                 ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$
document filename:           ^[A-Za-z0-9][A-Za-z0-9._-]{0,124}\.md$
document title stem length:  <= 125 chars, excluding .md
target:                      <workspace>:/<absolute-path-inside-workspace>
```

Examples:

```text
personal:/
personal:/notes/test.md
research-2026:/papers/llm.md
```

Disallowed in workspace/node names:

```text
/    path separator
:    workspace/path separator in target strings
space and control characters
. and .. as full node names
```

Document node names must end with `.md`; folder node names must not end with `.md`. The document title is currently the filename stem, not a separate mutable title field.

## Common output rules

- Paths are canonical absolute paths.
- Owner accounts are limited to `20` workspaces.
- Workspace active access accounts are limited to `20`.
- Creator accounts are limited to `50` active agents; each agent is limited to `10` active keys.
- File tree depth is limited to `5`; live direct children per folder are limited to `200`; live nodes per workspace are limited to `10000`.
- Workspaces are limited to `5000` live documents and `268435456` bytes of live document content.
- Markdown documents are limited to `524288` bytes and `2000` lines.
- List/search tools return page metadata.
- Read/search tools may return `truncated=true` and a next cursor/range hint.
- Internal errors are redacted.
- Error data should include actionable hints when possible.
- Secrets, bearer tokens, OAuth codes, PKCE verifiers, API key plaintext after creation, and raw Authorization headers are never returned or logged.


## `me`

Return the authenticated caller identity. This tool returns no service capabilities, secrets, or
tokens.

Input:

```json
{}
```

Output:

```json
{
  "account": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "user": {"sub": "authgate-subject", "email": "user@example.com"},
  "workspaces": [
    {"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
  ]
}
```

Auth/error contract:

- Missing or malformed bearer token: HTTP `401` with `WWW-Authenticate` resource metadata discovery.
- Invalid token: HTTP `401`.
- Valid authgate token but no local notegate account: HTTP `403 not_registered` with `login_url` and `mcp_url`.
- Inactive local account: HTTP `403 inactive_account`.

`me` should use the same identity builder as REST `GET /api/v1/me`; REST and MCP identity shapes
should stay aligned.


## `workspaces_list`

List workspaces accessible to the authenticated caller. Use this before file tools when the caller
has more than one workspace. The default limit is `50`; max limit is `100`.

Input:

```json
{
  "limit": 50,
  "cursor": "optional"
}
```

Output:

```json
{
  "workspaces": [
    {"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
  ],
  "page": {"limit": 50, "returned": 1, "has_more": false}
}

`root_node_id` is derived from the workspace root node lookup; it is not stored on the workspace row.
```

## `workspaces_get`

Return one workspace by name. If the name is ambiguous for the caller, the response is an ambiguity
error containing matching workspaces.

Input:

```json
{"workspace": "personal"}
```

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
  "page": {"limit": 100, "returned": 0, "has_more": false}
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

If `if_none_match_sha256` equals the current content hash, the tool may return
metadata without content:

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

`files_write` and `files_patch` are mutating tools. They must fail closed when the target is stale,
ambiguous, or unsafe to edit.

Common mutation rules:

- Mutations run in one transaction per document/node operation.
- If `expected_sha256` is present and does not match current document content, return conflict before applying changes.
- Successful mutations return the new `content_sha256`, `byte_len`, `line_count`, and current `path`.
- Failed mutations do not partially modify the document.
- Error responses should include actionable hints such as `read the document again`, `old_text matched multiple times`, or `use files_write for full rewrite`.
- Server should verify the persisted content hash after mutation before returning success.

`files_patch` deliberately uses exact matching, not fuzzy matching. Hermes-style fuzzy matching is useful
for local code agents, but notegate should keep MCP document edits predictable and auditable.

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
- `create=true` may create a missing document parented by `dirname(path)`.
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
- Destination parent must exist and be a folder.
- Existing destination sibling with the same name is conflict.
- Moving a folder into itself or a descendant is conflict.
- Folder move/rename must not rewrite descendant paths; paths are derived from the parent chain.

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
- Large subtree deletion may be rejected with a narrowing/async hint.

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
