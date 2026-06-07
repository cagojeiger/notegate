# Files search design

notegate has two search modes:

```text
find = metadata/path search over nodes
grep = Markdown body search over documents.content_md, then line-split in application code
```

## Source of truth vs index

Canonical data:

```text
nodes.path_cache, nodes.name, nodes.kind
documents.content_md
```

현재 단계에는 derived search table을 두지 않는다. `documents.content_md`가 grep의 원본이자
현재 검색 대상이다.

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

현재 grep은 `documents.content_md`에서 후보 문서를 찾고, application code가 원문을
line-split하여 line number와 context를 만든다. 이 방식은 저장 시 line index를 삭제/삽입하지
않으므로 빈번한 autosave에서 write amplification을 피한다.

Conceptual query:

```sql
SELECT n.id, n.path_cache, d.content_md
FROM documents d
JOIN nodes n
  ON n.id = d.node_id
 AND n.workspace_id = d.workspace_id
WHERE d.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND d.content_md ILIKE $query
  AND (n.path_cache = $scope OR n.path_cache LIKE $scope_prefix)
ORDER BY d.updated_at DESC
LIMIT $limit;
```

나중에 grep QPS나 문서 크기가 커지면 `document_lines`와 async indexer를 별도 설계로
추가한다. 그때도 line index는 source of truth가 아니라 `documents.content_md`에서 재생성 가능한
derived data여야 한다.

## Index freshness

현재 단계에는 별도 index freshness 상태가 없다. 저장 성공은 원본 `documents.content_md`와
metadata(`content_sha256`, `byte_len`, `line_count`)가 갱신되었음을 의미한다.

## Pagination

`find` supports keyset pagination by `(path_cache, id)`.

현재 grep cursor는 구현하지 않는다. line-level index를 도입할 때 `(path_cache, node_id, line_no)`
keyset pagination을 함께 설계한다. Cursor is opaque to clients.

## External search engines

Postgres remains the source of truth. If future scale requires typo tolerance, advanced ranking,
semantic search, high QPS, or independent search scaling, introduce a separate search-index interface
that can be backed by `document_lines` or an external engine.
