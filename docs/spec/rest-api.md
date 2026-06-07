# notegate REST API contract

이 문서는 notegate HTTP REST endpoint의 URL, request/response, pagination, status code 계약을 정의한다. 상위 API category와 layer 원칙은 `api.md`를 따른다.

REST API의 primary client는 브라우저 UI다. UI는 tree를 펼치고 선택 상태를 오래 유지해야 하므로 파일 조작은 `node_id` 중심으로 한다.

MCP/CLI는 path 중심이어도 되지만, REST/UI는 rename/move 이후에도 같은 node를 계속
가리켜야 하므로 id 기반을 기본으로 한다.

Resource API base path:

```text
/api/v1
```

Auth redirect endpoints are intentionally outside this base path under `/auth/*`.

## Design decisions

- REST client는 `workspace_id`를 URL에 명시한다.
- `workspace_id`는 secret이 아니며, 모든 요청은 서버에서 `workspace_access`로 다시 검증한다.
- 호출자 `user_id`/`account_id`는 클라이언트가 보내지 않는다. 인증된 caller에서 결정한다. 관리 API는 대상 account를 지정해야 할 때만 target `account_id`를 URL에 포함할 수 있다.
- `/files/root`는 두지 않는다. root node는 workspace representation에 포함된다.
- workspace마다 DB root node `/`는 반드시 존재한다.
- file/folder별 ACL은 없다. 권한은 workspace 단위 `viewer`/`editor`/`owner`다.
- 목록/검색 응답은 항상 pagination을 가진다.
- document write/patch는 `expected_sha256` 기반 optimistic concurrency를 지원한다.
- Workspace/node names use a CLI-safe restricted grammar; invalid names return `400`.
- Folder names are limited to `128` chars; document filenames are limited to `128` chars including `.md`.
- Document title stem is currently the filename stem and is limited to `125` chars excluding `.md`.
- Owner accounts are limited to `20` workspaces.
- Workspace active access accounts are limited to `20`.
- Creator accounts are limited to `50` active agents; each agent is limited to `10` active keys.
- File tree depth is limited to `5`; live direct children per folder are limited to `200`; live nodes per workspace are limited to `10000`.
- Workspaces are limited to `5000` live documents and `268435456` bytes of live document content.
- Documents are limited to `524288` bytes and `2000` lines.


## REST categories

REST는 top-level `files` category를 쓰지 않고, 기능과 권한 경계를 기준으로 나눈다.

| Category | Scope | Base path | Purpose |
|---|---|---|---|
| Auth | global | `/auth/*`, `/.well-known/oauth-protected-resource*` | browser login/logout, OAuth callback, bearer metadata |
| Identity | global | `/api/v1/me` | 현재 caller와 접근 가능한 workspace bootstrap |
| Workspaces | global | `/api/v1/workspaces` | workspace 생성/선택/관리 |
| Nodes | workspace | `/api/v1/workspaces/{workspace_id}/nodes` | folder/document tree metadata |
| Documents | workspace | `/api/v1/workspaces/{workspace_id}/documents` | Markdown content read/write/patch |
| Search | workspace | `/api/v1/workspaces/{workspace_id}/search` | find/grep |
| Access | workspace | `/api/v1/workspaces/{workspace_id}/access` | workspace role grant/revoke |
| Agents | global | `/api/v1/agents` | agent account and key lifecycle |

Module boundary should follow the same categories:

```text
auth
rest/me
rest/workspaces
rest/nodes
rest/documents
rest/search
rest/access
rest/agents
```

`nodes`, `documents`, and `search` together implement the product concept of files. They are split
in REST because their DTOs, permissions, pagination, and performance characteristics differ.

## Auth and authorization

인증 방식은 account kind를 결정한다.

```text
browser login via authgate       -> user account
MCP OAuth 2.1 via authgate        -> user account
device flow via authgate          -> user account
API key / agent key               -> agent account
```

권한은 요청 시작 시 workspace 단위로 확인한다.

```text
viewer = list/stat/read/find/grep
editor = viewer + write/patch/mkdir/touch/move/delete
owner  = editor + workspace access management
```

권한 없는 workspace나 cross-workspace node 접근은 `404`로 숨긴다.

## Common shapes

### Page

```json
{
  "page": {
    "limit": 100,
    "returned": 100,
    "has_more": true,
    "next_cursor": "opaque-cursor"
  }
}
```

`cursor`는 opaque token이며 클라이언트가 해석하지 않는다.

### Account ref

```json
{
  "id": "account-id",
  "kind": "user",
  "display_name": "Kang"
}
```

### Workspace output

```json
{
  "id": "workspace-id",
  "name": "personal",
  "role": "owner",
  "root_node_id": "root-node-id",
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```

`root_node_id` is derived from the workspace root node lookup; it is not stored on the workspace row.

### Node output

```json
{
  "id": "node-id",
  "workspace_id": "workspace-id",
  "parent_id": "parent-id-or-null",
  "name": "note.md",
  "kind": "document",
  "path": "/projects/note.md",
  "sort_order": 0,
  "has_children": false,
  "created_by": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "updated_by": {"id": "account-id", "kind": "agent", "display_name": "research-agent"},
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```


## Auth

notegate는 자체 password login API를 두지 않는다. 사람 사용자는 authgate OAuth/OIDC로
로그인하고, API key는 agent credential로 취급한다.

Auth endpoint는 JSON resource API와 성격이 다르므로 `/api/v1` 아래에 억지로 넣지 않는다.
브라우저 redirect/callback, session cookie 발급, OAuth protected-resource metadata를 담당한다.

### Start browser login

```http
GET /auth/login
```

Starts authgate OAuth/OIDC authorization-code + PKCE login. The response is a redirect to authgate.

### OAuth callback

```http
GET /auth/callback?code=...&state=...
```

Validates state/nonce, exchanges the code with authgate, upserts/activates the local user account
according to identity policy, and issues the browser session cookie.

### Logout

```http
POST /auth/logout
```

Clears the notegate browser session cookie. This does not revoke authgate's upstream session.

### OAuth protected resource metadata

```http
GET /.well-known/oauth-protected-resource
GET /.well-known/oauth-protected-resource/mcp
```

Advertises authgate as the authorization server for REST/MCP bearer-token clients.

### Auth boundary

- Browser UI may use the secure session cookie issued by `/auth/callback`.
- REST API may also accept `Authorization: Bearer ...` for non-browser clients.
- MCP accepts bearer credentials only; browser cookies are not accepted by MCP.
- Device flow is an authgate flow and resolves to a `user` account.
- API key / agent key authentication resolves to an `agent` account.

## Identity

### Get current caller

```http
GET /api/v1/me
```

Returns the authenticated account, optional user/agent detail, and accessible workspaces.

```json
{
  "account": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "user": {"sub": "authgate-sub", "email": "user@example.com"},
  "workspaces": [
    {"id": "workspace-id", "name": "personal", "role": "owner", "root_node_id": "root-node-id"}
  ]
}
```

The UI should use this response to choose an initial workspace. Workspaces are regular user/agent
resources: callers may create more than one workspace and may delete workspaces they own.

## Workspaces

### List workspaces

```http
GET /api/v1/workspaces?limit=50&cursor=...
```

Returns workspaces where the caller has non-revoked `workspace_access`. Default limit is `50`; max limit is `100`.

### Create workspace

```http
POST /api/v1/workspaces
```

```json
{
  "name": "personal"
}
```

Creates a workspace, grants the creator `owner`, and creates the canonical root node `/`. There is no single/default workspace restriction. Workspace name must match `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$`. The owner account may own at most `20` workspaces.

### Get workspace

```http
GET /api/v1/workspaces/{workspace_id}
```

Returns workspace metadata, caller role, and derived `root_node_id`.

### Rename workspace

```http
PATCH /api/v1/workspaces/{workspace_id}
```

```json
{
  "name": "personal"
}
```

Requires `owner`.

### Delete workspace

```http
DELETE /api/v1/workspaces/{workspace_id}
```

Requires `owner`. Workspace deletion is a normal supported operation and deletes the workspace boundary, access rows, nodes, and documents according to the DB cascade/retention policy.

## Access

Owner-only APIs for granting users or agents access to a workspace. A workspace may have at most `20` active access accounts.

### List access

```http
GET /api/v1/workspaces/{workspace_id}/access?limit=100&cursor=...
```

Default and max limit are `100`.

### Grant or change access

```http
PUT /api/v1/workspaces/{workspace_id}/access/{account_id}
```

```json
{
  "role": "viewer"
}
```

### Revoke access

```http
DELETE /api/v1/workspaces/{workspace_id}/access/{account_id}
```

Revokes access by setting `revoked_at`/`revoked_by`; current-state attribution fields remain valid.

## Nodes

All node APIs require the `{workspace_id}` URL segment and validate that the node belongs to that
workspace. Node ids are stable across rename and move.

### Resolve path

```http
GET /api/v1/workspaces/{workspace_id}/paths/resolve?path=/projects/note.md
```

Used by command palette, deep links, breadcrumbs, and search-result navigation. The response is a
`Node output`. Deleted nodes are not resolved.

### Get node

```http
GET /api/v1/workspaces/{workspace_id}/nodes/{node_id}
```

Returns node metadata. Useful for refresh after optimistic UI updates. The `path` field is derived from the parent chain, not stored as canonical full path.

### List children

```http
GET /api/v1/workspaces/{workspace_id}/nodes/{node_id}/children?limit=100&cursor=...
```

Rules:

- `{node_id}` must be a folder.
- Only direct children are returned.
- Pagination is required.
- Default ordering is `(sort_order, name, id)`.

```json
{
  "parent": {"id": "folder-id", "path": "/projects"},
  "children": [],
  "page": {"limit": 100, "returned": 0, "has_more": false}
}
```

### Create node

```http
POST /api/v1/workspaces/{workspace_id}/nodes
```

```json
{
  "parent_id": "folder-id",
  "kind": "document",
  "name": "note.md",
  "content_md": "# optional initial content\n"
}
```

Rules:

- `kind='folder'` ignores `content_md`.
- Node name must match `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`; `/`, `:`, spaces, control characters, `.`, and `..` are rejected.
- Folder name length must be `<= 128` chars.
- Document filename length must be `<= 128` chars including `.md`; title stem length must be `<= 125` chars excluding `.md`.
- Resulting depth must be `<= 5`.
- Parent folder must have fewer than `200` live direct children before create.
- Workspace must have fewer than `10000` live nodes before create.
- `kind='document'` requires `.md` name and creates a `documents` row.
- Document create additionally requires fewer than `5000` live documents in the workspace.
- `kind='folder'` name must not end with `.md`.
- Same parent folder cannot contain duplicate live names.

### Update node metadata

```http
PATCH /api/v1/workspaces/{workspace_id}/nodes/{node_id}
```

```json
{
  "name": "renamed.md",
  "sort_order": 10
}
```

Use this for rename or custom ordering. Root rename is rejected.

### Move node

```http
POST /api/v1/workspaces/{workspace_id}/nodes/{node_id}/move
```

```json
{
  "new_parent_id": "folder-id",
  "new_name": "optional-renamed.md",
  "expected_parent_id": "old-parent-id"
}
```

Rules:

- Root move is rejected.
- Moving a folder into itself or descendants is rejected.
- Resulting subtree depth must be `<= 5`.
- Destination parent must have fewer than `200` live direct children unless the move stays within the same parent.
- Move/rename updates only the moved node parent/name metadata; descendant paths are derived and must not be rewritten.
- The destination sibling name must be unique.
- `expected_parent_id` is optional but recommended for optimistic UI safety.

### Delete node

```http
DELETE /api/v1/workspaces/{workspace_id}/nodes/{node_id}?recursive=true
```

Rules:

- Root delete is rejected.
- Document delete does not require `recursive=true`.
- Folder delete requires `recursive=true`.
- Delete is soft delete on `nodes`; deleted nodes disappear from normal list/search/resolve.

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

If `if_none_match_sha256` equals the current hash, the server may return metadata without content:

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

## Agents

Agent APIs manage agent accounts and API keys. Workspace-specific permissions for agents are still granted through the Access category. API keys authenticate as `agent` accounts. Agent key lifecycle is governed by agent ownership/creator rules, not by workspace role; workspace owners only grant or revoke workspace access for agent accounts.

### List agents

```http
GET /api/v1/agents?limit=100&cursor=...
```

Returns agents created by or visible to the caller. Default and max limit are `100`.

### Create agent

```http
POST /api/v1/agents
```

```json
{
  "name": "research-agent"
}
```

Creates an `agent` account. Access to workspaces is granted separately through workspace access APIs. A creator account may have at most `50` active agents.

### Delete agent

```http
DELETE /api/v1/agents/{agent_id}
```

Soft-deactivates the underlying account, revokes active keys, and revokes workspace access.

### Create agent key

```http
POST /api/v1/agents/{agent_id}/keys
```

```json
{
  "name": "local-mcp",
  "expires_at": "2026-12-31T00:00:00Z",
  "scopes": []
}
```

Returns the plaintext key exactly once. An agent may have at most `10` active keys.

### Revoke agent key

```http
DELETE /api/v1/agents/{agent_id}/keys/{key_id}
```

Sets `revoked_at`/`revoked_by`.

## Error policy

- Missing/invalid auth: `401`
- Authenticated but no active local account: `403`
- Insufficient workspace role: `403`
- Not found or cross-workspace access: `404`
- Invalid field/name/path/limit/cursor: `400`
- Hash mismatch, root move/delete, duplicate destination, subtree too large: `409`
- Internal errors: `500` with redacted message
