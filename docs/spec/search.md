# Files search design

notegate has two search modes:

```text
find = metadata/path search over nodes
grep = Markdown body line search over document_lines
```

## Source of truth vs index

Canonical data:

```text
nodes.path_cache, nodes.name, nodes.kind
documents.content_md
```

Derived search data:

```text
document_lines
document_index_status
```

Derived index tables must be rebuildable from canonical data.

## Find

`find` searches node metadata.

Fields:

```text
name
path_cache
kind
updated_at
```

Suggested indexes:

```sql
CREATE INDEX nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_path_trgm_idx
    ON nodes USING gin (path_cache gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_find_cursor_idx
    ON nodes(workspace_id, path_cache, id)
    WHERE deleted_at IS NULL;
```

Find queries must include `workspace_id` and `deleted_at IS NULL`. Scope path filtering uses:

```sql
n.path_cache = $scope_path OR n.path_cache LIKE $scope_prefix
```

## Grep

`grep` searches `document_lines.line_text` and joins `nodes` for current path and deletion state.

Conceptual query:

```sql
SELECT n.id, n.path_cache, l.line_no, l.line_text
FROM document_lines l
JOIN nodes n
  ON n.id = l.node_id
 AND n.workspace_id = l.workspace_id
WHERE l.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND l.line_text ILIKE $query
  AND (n.path_cache = $scope OR n.path_cache LIKE $scope_prefix)
ORDER BY n.path_cache, l.line_no
LIMIT $limit;
```

Context lines are fetched by `(workspace_id, node_id, line_no)` around each hit.

## Index freshness

Initial implementation may update `document_lines` synchronously inside document save. The schema still
tracks `document_index_status` so the system can later move to async indexing.

Possible statuses:

```text
ready   index matches current content hash
stale   document changed but index is not updated yet
failed  indexing failed; error contains safe diagnostic
```

## Pagination

`find` supports keyset pagination by `(path_cache, id)`.

`grep` supports keyset pagination by `(path_cache, node_id, line_no)`. Cursor is opaque to clients.

## External search engines

Postgres remains the source of truth. If future scale requires typo tolerance, advanced ranking,
semantic search, high QPS, or independent search scaling, `document_lines` can be treated as the
local implementation of a broader search-index interface and replaced/augmented by an external engine.
