# Files performance limits

Files data can grow without bound, so every list/search/read/subtree operation must have explicit
limits. These values are initial product defaults; they may later become config.

## Path and name limits

```text
max_name_len = 255 bytes
max_path_len = 2048 bytes
max_depth = 64 segments
```

## Listing limits

```text
children_default_limit = 100
children_max_limit = 500
```

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

`cat/open` reads can be range-limited by line and byte count.

```text
cat_default_max_lines = 200
cat_max_lines = 1000
cat_default_max_bytes = 65536      # 64 KiB
cat_max_bytes = 262144             # 256 KiB
```

If a document is truncated, response includes:

```text
truncated = true
next_start_line
```

## Write limits

```text
document_max_bytes = 2097152       # 2 MiB
```

Oversized documents are rejected. If larger documents become a product requirement, introduce chunked
storage as a separate design rather than silently raising this limit.

## Subtree mutation limits

Moving or deleting a folder touches every live descendant.

```text
subtree_move_max_nodes = 1000
subtree_delete_max_nodes = 1000
```

If the subtree exceeds the limit, synchronous operation is rejected with a conflict/user-safe error and
a hint to narrow the operation or use a future async job path.

## Tree limits

A full `tree` command is not part of the first MCP tool set. If introduced later, it must include:

```text
tree_default_max_depth = 2
tree_max_depth = 5
tree_default_max_nodes = 200
tree_max_nodes = 1000
```

Tree responses must include `truncated` and a next-action hint when limits are reached.

## Soft-delete retention

Soft-deleted nodes remain in canonical tables until a retention job purges them. Search and listing
queries always filter `nodes.deleted_at IS NULL`.

Future config:

```text
soft_delete_retention_days = 30 or 90
```
