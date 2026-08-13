# Search

Search는 MCP와 Public V2 REST가 공유하는 path-first command다. Browser V1 REST는 search endpoint를 제공하지 않는다.

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

본문 또는 metadata는 search 응답에 싣지 않는다.

- MCP `find` result는 `schemas.md`의 `McpNodeSummary[] + Page`, `grep` result는 `McpGrepSummary[] + Page`다. 자세한 내용은 `read op=stat`, `read op=read`로 조회한다.
- Public V2 REST는 `{ items: SearchHitOut[], page: Page }`를 반환한다. `grep` hit에는 요청한 line mode에 따른 `match_lines`가 추가된다. Node 상세는 `GET /api/v2/spaces/{space_id}/nodes/{node_id}`, Text 본문은 `GET /api/v2/spaces/{space_id}/text/{node_id}`로 조회한다.

## Common traversal

Search는 scope folder 아래를 deterministic DFS pre-order로 순회한다.

```text
sibling order = sort_order, name, id
```

`nodes.search_enabled=false`인 node는 `find` 결과에서 제외한다. Text node는 `grep`에서도 제외한다. Folder의 값은 자식에게 상속되지 않으며 traversal 자체를 막지 않는다. 따라서 제외된 folder 아래의 검색 허용 node는 계속 검색할 수 있다.

순회 cursor는 마지막 match가 아니라 마지막으로 소비한 candidate 위치를 가리킨다. Cursor는 opaque이며 다음 조건에 묶인다.

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

## Two-stage search pipeline

Search는 두 단계로 동작한다.

```text
1. DB candidate scan
   - scope folder의 live subtree를 DFS pre-order로 후보화한다.
   - sibling order는 sort_order, name, id다.
   - DB는 내부 정렬 키(sort_path)를 만들어 순서를 안정화한다.
   - cursor는 마지막으로 소비한 candidate의 sort_path를 기억한다.

2. App matcher
   - DB가 반환한 candidate를 application에서 match한다.
   - regex는 application Rust regex dialect로 평가한다.
   - result limit과 scan budget에 도달하면 멈춘다.
```

DB는 traversal과 후보 bulk read를 담당하고, application은 match semantics를 담당한다. 이 구조는 DB round-trip을 줄이면서 regex backtracking 위험을 피하기 위한 결정이다.

## Cursor state

Cursor는 구현 세부 정보를 감싼 opaque string이다. 논리 상태는 다음 정보를 포함한다.

```ts
type SearchCursor = {
  version: number
  command: "find" | "grep"
  fingerprint: string
  scope_node_id: string
  after_sort_path?: string
}
```

`after_sort_path`는 마지막 match가 아니라 마지막으로 소비한 candidate의 내부 DFS 정렬 위치다. 다음 page는 같은 조건에서 `after_sort_path` 이후 candidate부터 이어서 검사한다.

`fingerprint`는 `space`, scope folder, `q`, match mode, kind filter, include/exclude, case policy, traversal order를 묶은 값이다. 다른 조건에 cursor를 재사용하면 invalid cursor다.

`sort_path`는 응답 schema나 DB 저장 model이 아니다. Search pagination을 위한 내부 정렬 키다. Tree가 pagination 중 변경되면 결과 일관성은 best-effort다.

## Candidate scan algorithm

```text
1. caller의 read permission을 확인한다.
2. scope path를 live folder node로 resolve한다.
3. cursor가 있으면 cursor fingerprint와 scope를 검증한다.
4. DB가 scope subtree candidate를 DFS pre-order로 bulk 조회한다.
5. cursor가 있으면 after_sort_path 이후 candidate만 조회한다.
6. application matcher가 candidate를 검사한다.
7. command별 matcher가 match하면 result에 추가한다.
8. result limit 또는 scan budget에 도달하면 마지막으로 소비한 candidate 위치로 next_cursor를 만든다.
9. scope subtree 끝이면 has_more=false로 끝낸다.
```

DB candidate scan은 raw recursive CTE 반환 순서에 의존하지 않는다. 반드시 명시적인 `sort_path` 또는 동등한 정렬 키를 만들고 `ORDER BY sort_path`로 DFS pre-order를 보장한다.

### `find` candidate scan

`find`는 node summary만 검사한다. Content와 metadata를 읽지 않는다.

```text
for each node candidate in DFS order:
  if node is root:
    skip result
  if search_enabled is false:
    skip result
  if kind filter mismatches:
    continue
  if include/exclude path filter mismatches:
    continue
  if name matches q with match mode:
    emit matched node summary
```

Match mode:

```text
contains = node name substring match
regex    = node name regex match
glob     = node name glob match
```

Match는 대소문자를 구분하지 않는다.

Glob과 regex는 명시적으로 선택한다. 예를 들어 `*.md`는 glob mode에서만 glob pattern이다.

`include`/`exclude` path filter는 glob pattern list다. `q`는 node name에만 적용하고, path filter는 derived path에만 적용한다.

### `grep` candidate scan

`grep`은 query를 포함하는 plain Text node 후보를 찾는다. 기본 응답은 파일 후보 목록이고, 요청 옵션에 따라 matching line number만 추가할 수 있다. Context line과 snippet은 반환하지 않는다.

대상:

```text
nodes.kind = 'text'
nodes.search_enabled = true
text_objects.storage_format = 'plain'
복호화된 plain content
```

- File은 grep 대상이 아니다.
- Client-side encrypted Text는 grep 대상이 아니다.
- 서버 관리 방식으로 at-rest 암호화된 plain Text는 복호화 후 grep한다.
- `grep`은 `nodes.metadata`를 검색하지 않는다.
- Match된 Text의 실제 내용은 MCP에서 `read op=read`, Public V2 REST에서 `GET /api/v2/spaces/{space_id}/text/{node_id}`로 조회한다.

Match mode:

```text
literal = content substring match
regex   = content regex match
```

Match는 대소문자를 구분하지 않는다.

Line mode:

```text
none  = line 정보를 반환하지 않는다
first = 첫 matching line number만 반환한다
all   = 모든 matching line number를 반환한다
```

Line number는 Text 안의 1-based logical line number다. Line matching은 line 단위로 수행한다. Regex도 각 line에 대해 평가하며, cross-line match는 지원하지 않는다.

`include`/`exclude` path filter는 glob pattern list다. 각 list는 최대 32개 pattern을 담을 수 있고, pattern 하나는 최대 256자다.

Text 하나는 atomic scan unit이다. Text 하나의 `byte_len`은 `text_max_bytes`를 넘지 않는다.

```text
for each plain text candidate in DFS order:
  if include/exclude path filter mismatches:
    continue
  if text.byte_len would exceed remaining_grep_scan_budget:
    stop before matching content
    return cursor that resumes before this text candidate
  remaining_grep_scan_budget -= text.byte_len
  match_lines = lines whose content matches q with match mode
  if match_lines is not empty:
    emit matched Text node summary
    if line mode is none:
      omit match_lines
    if line mode is first:
      include first matching line number
    if line mode is all:
      include all matching line numbers
```

Text 내부 line offset cursor는 사용하지 않는다.

## Decrypted body cache

`grep`은 후보 조회에서 얻은 `(space_id, node_id, content_sha256)`를 key로 복호화된 plain body를 process-local memory에 캐시한다.

```text
candidate metadata query:
  항상 실행
  current content_sha256, byte_len, line_count, at_rest_encryption 조회

body cache hit:
  PostgreSQL body query와 서버 복호화 생략

body cache miss:
  8 MiB request budget 안의 miss만 한 번의 bulk query로 조회
  live/plain/search_enabled/SHA/byte_len 재검증 후 복호화
  Arc<str>로 cache 저장
```

본문 변경 transaction이 `content_sha256`을 갱신하면 다음 candidate scan은 새 key를 사용하므로 이전 body를 재사용하지 않는다. 이전 key의 entry는 별도 전체 무효화 없이 capacity, TTL, TTI 정책으로 제거한다. Candidate scan 직후 write가 경합한 현재 요청은 scan 시점의 이전 body를 사용할 수 있으며, 다음 요청부터 새 SHA를 관측한다. Search pagination과 동일하게 단일 요청을 가로지르는 tree/content 변경 일관성은 best-effort다.

기본 정책:

```text
capacity = plaintext byte weight 128 MiB per process
eviction/admission = TinyLFU
TTL = insertion 후 30 minutes
TTI = 마지막 hit 후 5 minutes
capacity 0 = disabled
```

Cache에는 복호화된 본문만 저장하며 DB candidate, folder page, search result는 저장하지 않는다. 여러 replica 사이에 cache coherence나 공유 cache를 두지 않는다.

동시에 들어온 요청의 miss key가 겹치면 process 안에서 key별 load flight를 공유한다. 대기한 요청은 cache를 다시 확인하고, 여전히 없는 miss만 bulk query로 읽는다. 서로 겹치지 않는 key 집합은 독립적으로 진행한다.

## Worst-case scan and memory model

현재 `system_max` hard limit에서 scope가 root이고 모든 live node가 scope 안에 있으면 최악의 논리 scan 범위는 다음과 같다. `tier0`는 이보다 낮은 quota를 적용한다.

```text
node scan upper bound       = 25000 nodes per system_max space
                           = 2000 nodes per tier0 space
plain text scan upper bound = 1 GiB live Text content per system_max space
                           = 128 MiB live Text content per tier0 space
```

최악의 경우 search는 위 범위를 끝까지 탐색해야 한다. 하지만 한 요청에서 전체를 메모리에 올리지 않는다.

한 요청의 메모리 사용은 다음 budget으로 제한한다.

```text
DB candidate inspect    <= 1000 node summaries
node scan budget        <= 1000 node summaries
grep scan budget        <= 8 MiB content bytes
grep text read total    <= 8 MiB content bytes
response result limit   <= 100 node summaries
include glob patterns   <= 32 patterns × 256 chars
exclude glob patterns   <= 32 patterns × 256 chars
response body target    <= 256 KiB
decrypted body cache    <= 128 MiB plaintext weight per process by default
```

따라서 큰 scope 검색은 여러 page로 나뉜다.

```text
요청 1: scope 일부 scan -> result 일부 또는 0개 -> next_cursor
요청 2: cursor 이후 scan -> result 일부 또는 0개 -> next_cursor
...
마지막: scope 끝 -> has_more=false
```

이 모델은 전체 탐색 가능성을 인정하되 요청 단위 memory와 response size를 bounded하게 유지한다.
