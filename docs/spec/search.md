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
{"items":[],"has_more":true,"next_cursor":"..."}
```

이 응답은 이번 요청의 budget 안에서 match가 없었지만 아직 탐색할 candidate가 남았다는 의미다.

## Result shape

Search result는 기본적으로 node candidate 목록이다. 본문 또는 metadata는 search 응답에 싣지 않는다.

```text
node_id
path
name
kind
byte_len
updated_at
```

자세한 내용은 `files_stat`, `files_read`로 조회한다.

## `find`

`find`는 node name 검색이다.

대상:

```text
nodes.kind IN ('folder','text','file')
nodes.name
node kind filter
folder scope
```

Root node `/`는 결과에서 제외한다. `find`는 content나 `nodes.metadata`를 검색하지 않는다.

Match mode:

```text
contains = node name substring match
regex    = node name regex match
glob     = node name glob match
```

Glob과 regex는 명시적으로 선택한다. 예를 들어 `*.md`는 glob mode에서만 glob pattern이다.

## `grep`

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

`include`/`exclude` path filter는 glob pattern을 사용할 수 있다.

## Worst-case scan and memory model

현재 hard limit에서 scope가 root이고 모든 live node가 scope 안에 있으면 최악의 논리 scan 범위는 다음과 같다.

```text
node scan upper bound      = 10000 nodes
plain text scan upper bound = 256 MiB per space
```

최악의 경우 search는 위 범위를 끝까지 탐색해야 한다. 하지만 한 요청에서 전체를 메모리에 올리지 않는다.

한 요청의 메모리 사용은 다음 budget으로 제한한다.

```text
children page           <= 200 node summaries
grep scan budget        <= 8 MiB content bytes
response result limit   <= 100 node summaries
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
