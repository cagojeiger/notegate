# Search

Search는 MCP/CLI용 path-first command다. REST resource API는 search endpoint를 제공하지 않는다.

검색은 항상 folder scope의 subtree를 대상으로 한다. Scope를 생략하면 Space root `/`를 scope로 사용한다.

## Authorization

```text
user caller:
  space.owner_user_id = caller_user_id

agent caller:
  active connection exists
  permission read 또는 write
```

Search는 read permission으로 실행한다. 권한이 없으면 존재 여부를 숨긴다.

## Result shape

Search result는 `schemas.md`의 `McpNodeSummary[] + Page`다. 본문 또는 metadata는 search 응답에 싣지 않는다. 자세한 내용은 `files_stat`, `files_read`로 조회한다.

## Common traversal

Search는 scope folder 아래를 deterministic DFS pre-order로 순회한다.

```text
sibling order = sort_order, name, id
```

순회 cursor는 마지막 match가 아니라 마지막으로 검사한 위치를 가리킨다. Cursor는 opaque이며 다음 조건에 묶인다.

```text
space
scope folder
command kind
query
filter/match option
traversal order
```

다른 조건에 cursor를 재사용하면 invalid cursor다.

## Pagination and scan budget

Search는 반환 result 수와 scan budget을 분리한다.

```text
result limit = 응답으로 반환할 최대 item 수
scan budget  = 한 요청에서 검사할 최대 candidate 양
```

한 요청은 다음 중 하나에 도달하면 멈춘다.

```text
result limit 도달
scan budget 도달
scope subtree 끝
```

Scan budget에 먼저 도달하면 result가 없어도 `has_more=true`와 `next_cursor`를 반환할 수 있다.

```json
{"items":[],"page":{"limit":20,"returned":0,"has_more":true,"next_cursor":"..."}}
```

이 응답은 이번 요청의 budget 안에서 match가 없었지만 아직 탐색할 candidate가 남았다는 의미다.

## Scanner algorithm

### Cursor state

Cursor는 구현 세부 정보를 감싼 opaque string이다. 논리 상태는 다음 정보를 포함한다.

```ts
type SearchCursor = {
  version: number
  command: "find" | "grep"
  fingerprint: string
  stack: DfsFrame[]
}

type DfsFrame = {
  folder_node_id: string
  after?: ChildrenCursor
}

type ChildrenCursor = {
  sort_order: number
  name: string
  id: string
}
```

`fingerprint`는 `space`, scope folder, `q`, match mode, kind filter, include/exclude, case policy, traversal order를 묶은 값이다.

### Common DFS scanner

```text
1. caller의 read permission을 확인한다.
2. scope path를 live folder node로 resolve한다.
3. cursor가 있으면 cursor state와 query fingerprint를 검증한다.
4. cursor가 없으면 DFS stack을 scope folder로 초기화한다.
5. stack top folder의 children을 (sort_order, name, id) 순서로 page 조회한다.
6. child를 하나씩 검사한다.
7. command별 matcher가 match하면 McpNodeSummary result에 추가한다.
8. child가 folder이면 DFS stack에 추가하고 그 folder로 먼저 내려간다.
9. result limit 또는 scan budget에 도달하면 현재 traversal state로 next_cursor를 만든다.
10. stack이 비면 has_more=false로 끝낸다.
```

Folder의 children page size는 `search_children_page_max`를 넘지 않는다.

### `find` scanner

`find`는 node summary만 검사한다. Content와 metadata를 읽지 않는다.

```text
for each child in DFS order:
  if child is root:
    skip result
  if kind filter mismatches:
    continue
  if name matches q with match mode:
    emit McpNodeSummary
```

Match mode:

```text
contains = node name substring match
regex    = node name regex match
glob     = node name glob match
```

Match는 대소문자를 구분하지 않는다.

Glob과 regex는 명시적으로 선택한다. 예를 들어 `*.md`는 glob mode에서만 glob pattern이다.

### `grep` scanner

`grep`은 query를 포함하는 plain Text node 후보를 찾는다. Line number, context line, snippet은 반환하지 않는다.

대상:

```text
nodes.kind = 'text'
text_objects.storage_format = 'plain'
text_objects.content_text
```

- File은 grep 대상이 아니다.
- Encrypted Text는 grep 대상이 아니다.
- `grep`은 `nodes.metadata`를 검색하지 않는다.
- Match된 Text의 실제 내용은 `files_read`로 조회한다.

Match mode:

```text
literal = content substring match
regex   = content regex match
```

Match는 대소문자를 구분하지 않는다.

`include`/`exclude` path filter는 glob pattern list다. 각 list는 최대 32개 pattern을 담을 수 있고, pattern 하나는 최대 256자다.

Text 하나는 atomic scan unit이다. Text 하나의 `byte_len`은 `text_max_bytes`를 넘지 않는다.

```text
for each text child in DFS order:
  if include/exclude path filter mismatches:
    continue
  read plain text object
  if text.byte_len would exceed remaining_grep_scan_budget:
    stop before matching content
    return cursor pointing to this text as next candidate
  remaining_grep_scan_budget -= text.byte_len
  if content matches q with match mode:
    emit McpNodeSummary
```

Text 내부 line offset cursor는 사용하지 않는다.

## Worst-case scan and memory model

현재 hard limit에서 scope가 root이고 모든 live node가 scope 안에 있으면 최악의 논리 scan 범위는 다음과 같다.

```text
node scan upper bound       = 10000 nodes
plain text scan upper bound = 256 MiB per space
```

최악의 경우 search는 위 범위를 끝까지 탐색해야 한다. 하지만 한 요청에서 전체를 메모리에 올리지 않는다.

한 요청의 메모리 사용은 다음 budget으로 제한한다.

```text
children page           <= 200 node summaries
node scan budget        <= 1000 node summaries
grep scan budget        <= 8 MiB content bytes
response result limit   <= 100 node summaries
text read batch         <= grep_scan_budget_bytes / text_max_bytes
                         현재 hard limit 기준 최대 8 text objects
include glob patterns   <= 32 patterns × 256 chars
exclude glob patterns   <= 32 patterns × 256 chars
response body target    <= 256 KiB
```

따라서 큰 scope 검색은 여러 page로 나뉜다.

```text
요청 1: scope 일부 scan -> result 일부 또는 0개 -> next_cursor
요청 2: cursor 이후 scan -> result 일부 또는 0개 -> next_cursor
...
마지막: scope 끝 -> has_more=false
```

이 모델은 전체 탐색 가능성을 인정하되 요청 단위 memory와 response size를 bounded하게 유지한다.
