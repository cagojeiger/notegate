# notegate REST API contract

REST is the browser/UI resource API. It uses `node_id` for tree operations so UI selection remains stable across rename and move.

Base paths:

```text
/api/v1      resource API
/auth/*      browser OAuth session flow
/.well-known OAuth discovery metadata
```

## OpenAPI / Swagger 범위

`/openapi.json`과 `/swagger-ui`는 `/api/v1` JSON resource API만 문서화하고 샘플 호출 대상으로 삼는다.

OpenAPI에 포함한다:

```text
/api/v1/me
/api/v1/workspaces
/api/v1/workspaces/{workspace_id}/nodes
/api/v1/workspaces/{workspace_id}/documents
/api/v1/workspaces/{workspace_id}/search
/api/v1/workspaces/{workspace_id}/access
/api/v1/agents
```

OpenAPI에서 제외한다:

```text
/auth/*
/.well-known/*
/mcp
/health
/ready
```

판단 근거:

- `/api/v1/*`는 브라우저/UI resource API이며 JSON request/response 계약을 가진다.
- `/auth/*`는 redirect와 session cookie 발급 흐름이며 resource API가 아니다.
- `/.well-known/*`는 OAuth discovery metadata다.
- `/mcp`는 OpenAPI가 아니라 MCP tool schema로 정의한다.
- `/health`, `/ready`는 운영용 system endpoint다.

Swagger 샘플 호출 방법:

1. 브라우저 session 인증이 필요하면 별도 탭에서 `/auth/login`을 먼저 연다.
2. `/swagger-ui`를 연다.
3. 같은 origin browser session cookie로 `/api/v1/*`를 호출하거나, Swagger `Authorize`에 bearer token을 넣어 호출한다. Cookie 기반 mutation 요청은 same-origin `Origin`/`Referer`가 필요하다.
4. Auth redirect/session/discovery endpoint는 OpenAPI 범위 밖이며 [auth.md](auth.md)에만 정의한다.

## Category map

| Category | Scope | Base path | Contract file |
|---|---|---|---|
| Auth | global | `/auth/*`, `/.well-known/*` | [auth.md](auth.md) |
| Identity | global | `/api/v1/me` | [identity.md](identity.md) |
| Workspaces | global | `/api/v1/workspaces` | [workspaces.md](workspaces.md) |
| Access | workspace | `/api/v1/workspaces/{workspace_id}/access` | [access.md](access.md) |
| Nodes | workspace | `/api/v1/workspaces/{workspace_id}/nodes` | [nodes.md](nodes.md) |
| Documents | workspace | `/api/v1/workspaces/{workspace_id}/documents` | [documents.md](documents.md) |
| Search | workspace | `/api/v1/workspaces/{workspace_id}/search` | [search.md](search.md) |
| Agents | global | `/api/v1/agents` | [agents.md](agents.md) |
| Errors | global | all REST endpoints | [errors.md](errors.md) |

## Request identity

Contract:

```text
browser login via authgate -> user account
bearer OAuth token         -> user account
API key / agent key        -> agent account
```

Branching:

```text
missing/invalid auth              -> 401
valid authgate token, no account  -> 403
inactive account                  -> 403
client-supplied caller id         -> ignored; caller comes from auth
```

## Workspace authorization

Contract:

```text
viewer = list/stat/read/find/grep
editor = viewer + write/patch/mkdir/touch/move/delete
owner  = editor + workspace access management
```

Branching:

```text
workspace_id not visible to caller -> 404
node belongs to another workspace  -> 404
role below required level          -> 403
```

## File tree contract

Contract:

```text
workspace root node exists for every workspace
root_node_id is returned on workspace output
/files/root endpoint does not exist
file/folder ACL does not exist; role is workspace-scoped
nodes/documents/search implement the product files concept
```

Branching:

```text
invalid workspace/node name -> 400
root move/delete/rename     -> 409 or 400 by operation contract
```

## Pagination contract

Shape:

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

Branching:

```text
missing limit    -> endpoint default
limit < 1        -> 1
limit > max      -> max
malformed limit  -> 400
malformed/tampered cursor -> 400
```

Cursor contract:

```text
client receives cursor -> pass it back unchanged
client changes cursor  -> invalid cursor or undefined page position
```

## Common output shapes

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
  "content_sha256": "sha256...",
  "byte_len": 7,
  "line_count": 1,
  "created_by": {"id": "account-id", "kind": "user", "display_name": "Kang"},
  "updated_by": {"id": "account-id", "kind": "agent", "display_name": "research-agent"},
  "created_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z"
}
```

`content_sha256`, `byte_len`, `line_count`는 document node를 단건 조회하거나 path resolve할 때 포함될 수 있다. Folder node와 bulk children/search item에서 document metrics가 없으면 필드를 생략한다.

## Error contract

REST 오류 응답은 `error`, `kind`, `message`를 포함한다. `error`와 `kind`는 같은 값을 가지며, `kind`는 MCP `data.kind`와 같은 공통 오류 분류다.

```text
missing_token/invalid_token -> 401 auth failure
not_registered/inactive_account -> 403 auth registration/state failure
forbidden -> 403 authenticated but not allowed
not_found -> 404 not found or cross-workspace hidden resource
invalid_input -> 400 invalid field/name/path, malformed limit, malformed/tampered cursor
conflict -> 409 state conflict, quota conflict, stale hash, duplicate destination, subtree too large
internal_error -> 500 redacted internal error
```
