# notegate performance and resource limits

notegate data can grow without bound, so every create/list/search/read/subtree operation must have
explicit limits. These values are initial product defaults; they may later become config.

## Account, workspace, and credential limits

```text
owned_workspaces_max = 20 per owner account
workspace_access_max_accounts = 20 active accounts per workspace
agents_per_creator_max = 50 active agents per creator account
agent_keys_per_agent_max = 10 active keys per agent
```

These limits keep personal-workspace UX and permission checks bounded. They are product limits, not
security boundaries; authorization still checks every request.

## Path and name limits

```text
workspace_name_max_len = 63 chars
folder_name_max_len = 128 chars
document_file_name_max_len = 128 chars, including .md
document_title_stem_max_len = 125 chars, excluding .md
max_path_len = 768 bytes
max_path_depth = 5 segments
workspace_max_nodes = 10000 live nodes
workspace_max_documents = 5000 live documents
workspace_max_document_bytes = 268435456 bytes  # 256 MiB
```

Depth counts segments below workspace root:

```text
/                     depth 0
/notes                depth 1
/notes/daily          depth 2
/notes/daily/today.md depth 3
```

## Listing and folder fanout limits

```text
folder_max_children = 200
children_default_limit = 100
children_max_limit = 200
```

`folder_max_children` limits live direct children per folder. It forces users and agents to split
large collections into subfolders or separate workspaces before a single folder becomes too large.
Create, move, and restore operations must reject results that would exceed this limit.

Children listing uses keyset pagination with an opaque cursor over:

```text
sort_order, name, id
```

## Search limits

```text
find_default_limit = 50
find_max_limit = 100

grep_default_limit = 20
grep_max_limit = 100
grep_default_context = 2
grep_max_context = 5
```

LLM/MCP callers should prefer scoped search paths over workspace-wide grep.

## Read limits

`read/open` reads can be range-limited by line and byte count.

```text
read_default_max_lines = 200
read_max_lines = 1000
read_default_max_bytes = 65536      # 64 KiB
read_max_bytes = 262144             # 256 KiB
```

If a document is truncated, response includes:

```text
truncated = true
next_start_line
```

## Document creation and write limits

```text
document_max_bytes = 524288        # 512 KiB per document
document_max_lines = 2000          # per document
workspace_max_documents = 5000     # live documents per workspace
workspace_max_document_bytes = 268435456  # 256 MiB total live document bytes per workspace
```

Creating a document consumes both one `nodes` row and one `documents` row. Therefore document create
must satisfy `workspace_max_nodes`, `workspace_max_documents`, parent folder fanout, depth, path, and
name limits. Document write/patch must satisfy both per-document and workspace-total content limits.

Oversized or overlong documents are rejected. The product should nudge users/agents to split long
notes into smaller documents instead of silently allowing unbounded growth. If larger documents become
a product requirement, introduce chunked storage/indexing as a separate design rather than silently
raising these limits.

## Subtree mutation limits

Folder move/rename must not rewrite descendant paths. The canonical tree location is
`parent_id + name`, and display paths are derived from the parent chain. Therefore moving a large
folder should update only the moved node plus bounded validation state.

```text
subtree_delete_max_nodes = 1000
```

Deleting a folder still touches every live descendant because each node must be soft-deleted. If the
subtree exceeds the delete limit, synchronous delete is rejected with a conflict/user-safe error and a
hint to narrow the operation or use a future async job path.

## Tree limits

A full `tree` command is not part of the first MCP tool set. If introduced later, it must include:

```text
tree_default_max_depth = 2
tree_max_depth = 5
tree_default_max_nodes = 200
tree_max_nodes = 1000
```

Tree responses must include `truncated` and a next-action hint when limits are reached.

## API pagination limits

```text
workspaces_default_limit = 50
workspaces_max_limit = 100
access_default_limit = 100
access_max_limit = 100
agents_default_limit = 100
agents_max_limit = 100
```

All list endpoints must clamp or reject client limits above the max. Cursors are opaque.

## Soft-delete retention

Soft-deleted nodes remain in canonical tables until a retention job purges them. Search and listing
queries always filter `nodes.deleted_at IS NULL`.

Future config:

```text
soft_delete_retention_days = 30 or 90
```
