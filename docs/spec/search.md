# Files search design

notegate의 현재 검색은 Postgres 테이블만 사용한다. 별도 검색 테이블이나 외부 검색
엔진은 현재 스펙에 포함하지 않는다.

```text
find = nodes name/kind search + derived path display
grep = documents.content_md body search + derived path display
```

## Source of truth

```text
nodes.parent_id
nodes.name
nodes.kind
documents.content_md
```

Full path string은 canonical column으로 저장하지 않는다. 응답 path와 scope path는
`parent_id + name` tree에서 derive한다.

## Scope path

`find`와 `grep`이 `scope_path`를 받으면 먼저 selected workspace 안에서 path를 resolve해
scope node를 찾는다. Folder scope는 그 folder subtree 안으로 검색을 제한한다. Document
scope는 그 document 하나만 검색한다. Resolve되지 않는 scope path는 not found로 거부한다.

처리 흐름:

```text
1. workspace selector를 accessible workspace로 resolve한다.
2. scope_path를 root부터 segment별 `(workspace_id, parent_id, name)` lookup으로 resolve한다.
3. scope node kind를 확인한다.
4. folder scope면 subtree node id set을 recursive CTE로 계산한다.
5. document scope면 scope node id 하나만 검색 대상으로 둔다.
6. find/grep query는 이 scope id set 안에서만 실행한다.
7. 결과 path는 parent chain에서 derive한다.
```

Subtree id set은 application이 depth별 병렬 query를 직접 날려서 만들지 않는다. 기본 구현은
Postgres recursive CTE가 depth 0부터 child를 확장해가며 계산한다. 개념적으로는 다음과 같다.

```text
depth 0: scope node
depth 1: scope node의 children(folder scope인 경우)
depth 2: depth 1 children의 children
...
depth 5: 최대 depth 도달
```

예시 CTE:

```sql
WITH RECURSIVE subtree AS (
  SELECT id
  FROM nodes
  WHERE workspace_id = $workspace_id
    AND id = $scope_node_id
    AND deleted_at IS NULL

  UNION ALL

  SELECT n.id
  FROM nodes n
  JOIN subtree s ON n.parent_id = s.id
  WHERE n.workspace_id = $workspace_id
    AND n.deleted_at IS NULL
)
SELECT id
FROM subtree;
```

현재 한계값 때문에 subtree 계산은 bounded operation이다.

```text
max_path_depth = 5
folder_max_children = 200
workspace_max_nodes = 10000
workspace_max_documents = 5000
workspace_max_document_bytes = 268435456
```

`folder_max_children`만 보면 fanout이 커질 수 있지만, workspace 전체 live node 수가
`10000`으로 제한되므로 scoped search의 최악 subtree 크기도 workspace limit을 넘지 않는다.
`grep`의 후보 문서 수와 본문 총량도 `workspace_max_documents`와
`workspace_max_document_bytes`로 bounded된다.

## Find

`find`는 node metadata를 검색한다.

검색 대상:

```text
nodes.name
nodes.kind
```

`find.q`는 node name에만 매칭한다. Path는 검색 매칭 대상이 아니라 scope 제한과 결과 display에
사용한다. 특정 경로 아래에서 찾고 싶으면 `scope_path`를 사용한다. Query는 single-line,
non-empty, 최대 256 characters다.
Workspace root node는 `find` 결과에서 제외한다.

현재 query shape는 name/kind 후보를 먼저 찾고, 필요한 경우 scope subtree로 제한한다.
path는 결과 조립 단계에서 derive한다.

```sql
SELECT n.*
FROM nodes n
WHERE n.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND n.name ILIKE $query
  AND ($kind IS NULL OR n.kind = $kind)
ORDER BY n.name, n.id
LIMIT $limit;
```

`scope_path`가 있으면 scope node를 resolve한 뒤 recursive CTE나 application traversal로
scope subtree node id set을 만든다. 이 작업은 workspace live node limit과 depth limit으로
상한이 있다.

현재 보조 index:

```sql
CREATE INDEX nodes_name_trgm_idx
    ON nodes USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;

CREATE INDEX nodes_kind_idx
    ON nodes(workspace_id, kind)
    WHERE deleted_at IS NULL;
```

Path substring search는 현재 `find` 계약에 포함하지 않는다. 강한 path substring search가
필요해지면 full-path string을 canonical column으로 되살리기보다 별도 search index/materialized
view를 도입한다.

## Grep

`grep`은 `documents.content_md`에서 후보 문서를 찾고, application code가 원문을
line-split해서 line number와 context를 만든다. 저장 시 line별 row를 만들지 않는다.

Query는 single-line, non-empty, 최대 256 characters다.

현재 query shape:

```sql
SELECT n.id, d.content_md
FROM documents d
JOIN nodes n
  ON n.id = d.node_id
 AND n.workspace_id = d.workspace_id
WHERE d.workspace_id = $workspace_id
  AND n.deleted_at IS NULL
  AND d.content_md ILIKE $query
ORDER BY d.updated_at DESC
LIMIT $limit;
```

`scope_path`가 있으면 `find`와 같은 방식으로 scope subtree node id set을 만든 뒤
후보 문서를 제한한다. 결과 path는 parent chain에서 derive한다.

현재 보조 index:

```sql
CREATE INDEX documents_content_trgm_idx
    ON documents USING gin (content_md gin_trgm_ops);

CREATE INDEX documents_workspace_updated_idx
    ON documents(workspace_id, updated_at DESC, node_id);
```

## Pagination

Search endpoints use opaque cursors. Cursor format is server-owned and clients must not parse it.
Cursors are signed; malformed or tampered cursors are rejected.

- `find_default_limit = 50`, `find_max_limit = 100`
- `grep_default_limit = 20`, `grep_max_limit = 100`

`find` cursor is based on the stable ordering used for node metadata results, currently `(name ASC, id ASC)`.
`grep` cursor is based on the stable ordering used for content candidates, currently `(updated_at DESC, node_id ASC)`
plus an intra-document `match_offset` when needed. Invalid or tampered cursors return `400 invalid cursor`;
stale cursors are not a stable client contract.
