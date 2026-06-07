# Files search design

notegate의 현재 검색은 Postgres 테이블만 사용한다. 별도 검색 테이블이나 외부 검색
엔진은 현재 스펙에 포함하지 않는다.

```text
find = nodes metadata/path search
grep = documents.content_md body search
```

## Source of truth

```text
nodes.name
nodes.path_cache
nodes.kind
documents.content_md
```

## Find

`find`는 node metadata를 검색한다.

검색 대상:

```text
nodes.name
nodes.path_cache
nodes.kind
```

현재 query shape:

```sql
SELECT n.*
FROM nodes n
WHERE n.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND n.path_cache ILIKE $query
  AND ($kind IS NULL OR n.kind = $kind)
  AND (
      $scope_path IS NULL
      OR n.path_cache = $scope_path
      OR n.path_cache LIKE $scope_prefix
  )
ORDER BY n.path_cache
LIMIT $limit;
```

현재 migration에 있는 보조 index:

```sql
CREATE INDEX nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_path_trgm_idx
    ON nodes USING gin (path_cache gin_trgm_ops)
    WHERE deleted_at IS NULL;
```

## Grep

`grep`은 `documents.content_md`에서 후보 문서를 찾고, application code가 원문을
line-split해서 line number와 context를 만든다. 저장 시 line별 row를 만들지 않는다.

현재 query shape:

```sql
SELECT n.id, n.path_cache, d.content_md
FROM documents d
JOIN nodes n
  ON n.id = d.node_id
 AND n.workspace_id = d.workspace_id
WHERE d.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND d.content_md ILIKE $query
  AND (
      $scope_path IS NULL
      OR n.path_cache = $scope_path
      OR n.path_cache LIKE $scope_prefix
  )
ORDER BY d.updated_at DESC
LIMIT $limit;
```

현재 migration에 있는 보조 index:

```sql
CREATE INDEX documents_content_trgm_idx
    ON documents USING gin (content_md gin_trgm_ops);
```

## Pagination

현재 구현은 `limit`만 강제한다. search cursor는 아직 구현하지 않는다.

- `find_default_limit = 50`, `find_max_limit = 100`
- `grep_default_limit = 20`, `grep_max_limit = 100`

cursor가 요청되면 현재 REST API는 unsupported로 거부한다.
