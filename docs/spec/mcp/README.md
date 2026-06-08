# notegate MCP tool contract

MCP is the LLM/CLI command surface. It uses path-first inputs and hides `node_id` except where path targeting cannot work.

Surface:

```text
/mcp
```

## Authentication

Contract:

```text
OAuth bearer token via authgate -> user account
API key / agent key bearer     -> agent account
browser/session cookie         -> rejected
```

Branching:

```text
missing/malformed bearer -> 401 with WWW-Authenticate resource_metadata and openid offline_access scope hint
invalid bearer           -> 401
valid auth, no account   -> 403 not_registered with login_url and mcp_url
inactive account         -> 403 inactive_account
```

Discovery endpoints:

```text
/.well-known/oauth-authorization-server
/.well-known/oauth-protected-resource
/.well-known/oauth-protected-resource/mcp
```

MCP-specific onboarding and discovery details live in [auth.md](auth.md).

## Tool set

```text
me
workspaces_list
workspaces_create
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
files_restore
files_find
files_grep
```

## Workspace selection

Selector fields:

```json
{
  "workspace": "personal",
  "workspace_id": "optional-uuid",
  "path": "/projects/note.md",
  "target": "personal:/projects/note.md"
}
```

Branching:

```text
target present                         -> parse workspace + path from target
workspace_id present                   -> select that accessible workspace
workspace present                      -> select accessible workspace by name
no workspace and exactly one visible   -> select that workspace
no workspace and zero visible          -> invalid params; call workspaces_create first
no workspace and multiple visible      -> invalid params; pass workspace
same visible name matches more than one -> invalid params with ambiguity data
workspace_id inaccessible              -> invalid params
```

Path scope:

```text
paths resolve inside selected workspace only
file tools do not move nodes across workspaces
```

## Name and target grammar

```text
workspace name:              ^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$
folder name:                 ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$
document filename:           ^[A-Za-z0-9][A-Za-z0-9._-]{0,124}\.md$
document title stem length:  <= 125 chars, excluding .md
target:                      <workspace>:/<absolute-path-inside-workspace>
```

Branching:

```text
invalid workspace/name grammar -> invalid params or invalid input
folder name ending .md         -> invalid input
document name not ending .md   -> invalid input
path not starting with /        -> invalid params
```

## Pagination and range contract

List/search page shape:

```json
{
  "limit": 50,
  "returned": 50,
  "has_more": true,
  "next_cursor": "opaque-cursor"
}
```

`next_cursor` is `null` when `has_more=false`.

Branching:

```text
missing limit    -> tool default
limit < 1        -> 1
limit > max      -> max
malformed cursor -> invalid params
```

Read branching:

```text
content fits range -> truncated=false
content exceeds range -> truncated=true and next_start_line
matching if_none_match hash -> unchanged response without content
```

## Output and error contract

```text
paths                    -> canonical absolute paths
internal errors           -> redacted
bearer/API key plaintext  -> never returned except newly-created API key plaintext once
OAuth code/PKCE verifier  -> never returned
raw Authorization header  -> never returned
```

Service error mapping:

```text
not_found     -> invalid params with kind=not_found
invalid_input -> invalid params with kind=invalid_input
forbidden     -> invalid request with kind=forbidden
conflict      -> invalid request with kind=conflict
internal      -> internal error with redacted message
```

## Category documents

- [Auth](auth.md)
- [Identity](identity.md)
- [Workspaces](workspaces.md)
- [Files](files.md)
- [Search](search.md)
