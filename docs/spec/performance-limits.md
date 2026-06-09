# notegate performance and resource limits

Every list, search, read, write, and subtree mutation is bounded.

## Runtime capacity config

Product defaults live in `core::limits`. Runtime overrides are loaded only through `Config.limits`.

Overridable capacity caps:

```text
NOTEGATE_LIMITS__WORKSPACE_MAX_NODES
NOTEGATE_LIMITS__WORKSPACE_MAX_DOCUMENTS
NOTEGATE_LIMITS__WORKSPACE_MAX_DOCUMENT_BYTES
NOTEGATE_LIMITS__FOLDER_MAX_CHILDREN
```

Branching:

```text
missing override -> product default
override = 0     -> configuration error
unknown limit key -> configuration error
```

## Account, workspace, and credential limits

```text
owned_workspaces_max = 20 live workspaces per user creator account
workspace_access_max_accounts = 20 active granted accounts per workspace, implicit owner 제외
agents_per_creator_max = 50 active agents per user creator account
agent_keys_per_agent_max = 10 live keys per agent
```

Branching:

```text
create/grant within cap -> allowed
create/grant over cap   -> 409 conflict
```

## Name and path limits

```text
workspace_name_max_len = 63 chars
folder_name_max_len = 128 chars
document_file_name_max_len = 128 chars, including .md
document_title_stem_max_len = 125 chars, excluding .md
max_path_len = 645 bytes
max_path_depth = 5 segments
```

Depth counts segments below workspace root:

```text
/                     depth 0
/notes                depth 1
/notes/daily          depth 2
/notes/daily/today.md depth 3
```

Branching:

```text
invalid workspace/name syntax -> 400 invalid input
non-absolute path            -> 400 invalid input
unresolved live path         -> 404 not found
resulting depth > 5          -> 400 invalid input or 409 conflict by operation context
resulting path length > 645  -> 400 invalid input
```

## Workspace file-tree capacity limits

```text
workspace_max_nodes = 10000 live nodes
workspace_max_documents = 5000 live documents
workspace_max_document_bytes = 268435456 bytes  # 256 MiB
folder_max_children = 200 live direct children per folder
```

Branching:

```text
create node over workspace_max_nodes                    -> 409 conflict
create document over workspace_max_documents            -> 409 conflict
write/patch/create over workspace_max_document_bytes    -> 409 conflict
create/move over folder_max_children                    -> 409 conflict
```

## List pagination limits

```text
children_default_limit = 100
children_max_limit = 200
workspaces_default_limit = 50
workspaces_max_limit = 100
access_default_limit = 100
access_max_limit = 100
agents_default_limit = 100
agents_max_limit = 100
```

Branching:

```text
missing limit   -> endpoint default
limit < 1       -> 1
limit > max     -> max
malformed limit -> 400 invalid input
malformed cursor -> 400 invalid input
```

Cursor tuples:

```text
children   -> sort_order ASC, name ASC, id ASC
workspaces -> created_at ASC, id ASC
access     -> account id over the service-materialized, stable access list
agents     -> agent id over the service-materialized, stable agent list
```

Cursor format은 opaque이며 service-owned다. REST/MCP surface는 cursor 문자열을 해석하지 않고 그대로 전달한다.

## Search limits

```text
find_default_limit = 50
find_max_limit = 100
grep_default_limit = 20
grep_max_limit = 100
grep_default_context = 2
grep_max_context = 5
search_query_max_chars = 256
```

Branching:

```text
missing limit      -> endpoint default
limit < 1          -> 1
limit > max        -> max
missing context    -> grep_default_context
context < 0        -> 0
context > max      -> grep_max_context
empty query        -> 400 invalid input
multiline query    -> 400 invalid input
query > 256 chars  -> 400 invalid input
malformed cursor   -> 400 invalid input
tampered cursor    -> 400 invalid input
missing scope path -> 404 not found / MCP invalid params kind=not_found
```

Cursor tuples:

```text
find -> name ASC, id ASC
grep -> updated_at DESC, node_id ASC, match_offset
```

`match_offset`은 한 document 안의 match 수가 page limit보다 많을 때 같은 document 내부에서 이어 읽기 위한 값이다.

## Read limits

```text
read_default_max_lines = 200
read_max_lines = 1000
read_default_max_bytes = 65536   # 64 KiB
read_max_bytes = 262144          # 256 KiB
```

Branching:

```text
missing max_lines -> read_default_max_lines
max_lines < 1     -> 1
max_lines > max   -> read_max_lines
missing max_bytes -> read_default_max_bytes
max_bytes > max   -> read_max_bytes
truncated read    -> truncated=true and next_start_line
unchanged hash    -> metadata without content
```

## Document write limits

```text
document_max_bytes = 524288  # 512 KiB per document
document_max_lines = 2000    # per document
```

Branching:

```text
document bytes > document_max_bytes -> 400 invalid input
document lines > document_max_lines -> 400 invalid input
workspace live document bytes > max  -> 409 conflict
hash mismatch on guarded write/patch -> 409 conflict
```

## Subtree mutation limits

```text
subtree_delete_max_nodes = 1000
```

Branching:

```text
folder delete with recursive=false        -> 409 conflict
recursive delete subtree > max            -> 409 conflict
folder move/rename                        -> update moved node only
folder delete                             -> soft-delete every live descendant in the bounded subtree
soft-deleted nodes                        -> excluded from list/search/live counts
deleted node/workspace retention          -> 30 days before purge eligibility
purge job                                 -> may hard-delete rows whose purge_after has passed
purge worker concurrency                  -> PostgreSQL advisory transaction lock; one active purge per DB
purge batch                               -> workspaces <= 100, selected nodes <= 1000 per run
```
