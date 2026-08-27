# Search

Search는 Command API와 MCP의 path-first command다.

`notegate-search` crate는 `find`와 `grep`을 소유하고, `FilesService`는 일반 file-tree 조회인 `tree`를 소유한다.

## Execution boundary

`find`와 `grep`은 private HTTP client를 통해 `SearchRuntime`으로 전달된다. Runtime은
`SearchService`, `SearchAdmission`, 복호화 body cache와 search telemetry를 소유한다.

```text
MCP/CLI search
  -> SearchClient
  -> signed private HTTP
  -> SearchRuntime
       -> admission
       -> SearchService
       -> PostgresSearchStore
       -> PostgreSQL primary/read pool
```

기본 `all`과 local `api` mode는 public listener와 search listener를 다른 socket으로 띄운다.
`search` mode는 private search listener만 띄우므로 같은 image를 별도 search pod로 배포할 수 있다.
API pod에 `search_service_url`을 지정하면 해당 내부 service를 호출한다. Private search route는 search
listener에만 등록한다.

Search storage access는 `notegate-search` 내부 `store` 경계가 소유한다. `PostgresSearchStore`는
`FilesRepo`에 권한, scope, candidate, body와 result hydration 연산을 위임한다. 권한 판정은 primary
pool을 사용한다. `read_database_url`이 설정되면 scope, candidate, body와 result hydration은 별도
read pool을 사용하고, 설정되지 않으면 primary pool을 공유한다. 따라서 권한 철회는 primary 기준으로
판정하고 검색 결과는 read replica의 지연을 반영할 수 있다. 별도 read pool은 local search listener를
소유한 process에서만 생성한다.

API `/ready`는 API가 소유한 dependency만 검사한다. Search pod는 자신의 `/ready`로 DB/schema 준비
상태를 알린다. API가 Search에 연결하지 못하면 retryable `search_unavailable`을 반환한다. 운영에서는
이 오류율과 Search pod readiness를 함께 경보한다.

Private request signature는 timestamp, HTTP method, path와 정확한 body bytes를 묶는다. 허용 clock
skew는 60초다. Response도 request timestamp, status, path와 body를 묶어 서명한다. 양쪽 key는
LOOKUP root에서 session key와 다른 purpose label로 파생한다. 이 서명은 service authentication과
integrity 경계이고, pod 간 transport confidentiality와 `/metrics` 접근 제한은 TLS 또는 cluster
network policy가 담당한다.

API는 `x-request-id`를 내부 Search 요청에 전달하고 Search 응답도 같은 값을 유지한다. 이 request
context carrier는 향후 W3C `traceparent`/`tracestate` 전파를 추가하는 경계이며, 검색 명령 본문이나
권한 계약에는 관측성 필드를 넣지 않는다.

Data-plane ingress는 외부 요청의 30초 deadline을 한 번 정하고, API는 서명된 private request
envelope에 응답 여유 1초를 제외한 남은 실행 시간 `timeout_ms`를 전달한다. API와 Search는 각자
monotonic clock으로 남은 시간을 측정하므로 split pod의 wall-clock 차이에 의존하지 않는다. API에서
1초 이하만 남았거나 Search가 `timeout_ms=0`을 받으면 검색을 시작하지 않는다. 실행 중 timeout을
넘겨도 작업을 취소하고 서명된 `504 deadline_exceeded`를 반환한다. 외부 caller가 지나치게 큰 값을
보내더라도 Search 실행은 29초로 제한된다. ingress deadline extension이 누락된 내부 호출은 새로운
시간을 만들지 않고 `search_unavailable`로 fail-closed 처리한다.

Search deadline은 `notegate_search_deadline_exceeded_total{operation,phase}` counter와
`internal_search.deadline_exceeded` warning log로 기록한다. Label은 `operation=find|grep`,
`phase=before_execution|during_execution`으로 제한한다.

### Runtime contract

Private Search HTTP는 독립 제품 API가 아니라 같은 NoteGate release의 process role 사이 계약이다.
`/internal/v1`이 현재 process 간 계약이다.

- Command API와 MCP의 public command input은 unknown field를 거부한다.
- HMAC 인증된 private find/grep command와 Search response는 unknown field를 무시한다.
- 필수 필드 누락과 필드 type 오류는 거부한다.
- `/internal/v1` 안에서 필드의 이름, type, 의미와 enum 의미는 안정적으로 유지한다.
- 호환되지 않는 변경은 새 version path로 정의한다.

선택 필드는 expand-activate 순서로 배포한다.

1. 새 필드를 수용하는 Search role을 배포한다.
2. Search readiness를 확인한다.
3. 새 필드를 전송하는 API role을 배포한다.

서명된 private response의 HTTP status와 error kind는 다음 계약을 따른다. API client는 status와 body가
모순되거나 성공 응답의 status가 `200 OK`가 아니면 `search_unavailable`로 처리한다.

| Private error kind | HTTP status | Public error code |
| --- | ---: | --- |
| `invalid_input` | `400` | `invalid_input` |
| `forbidden` | `403` | `forbidden` |
| `not_found` | `404` | `not_found` |
| `conflict` | `409` | `conflict` |
| `write_locked` + `scope` | `423` | `node_write_locked` / `subtree_write_locked` |
| `search_busy` | `429` | `search_busy` |
| `usage_recalculation_in_progress` | `503` | `usage_recalculation_in_progress` |
| `deadline_exceeded` | `504` | `deadline_exceeded` |
| `internal_error` | `500` | `internal_error` |

MCP의 dependency/maintenance 임시 실패는 공통 JSON-RPC server code `-32001`, process capacity 거부는
`-32002`를 사용한다. 구체적인 분기는 숫자만이 아니라 `data.code`로 수행한다.

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
- Match된 Text의 실제 내용은 MCP `read op=read`로 조회한다.

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

본문 변경 transaction은 `content_sha256`을 갱신하고 다음 candidate scan은 새 cache key를 사용한다.
이전 key의 entry는 capacity, TTL과 TTI 정책으로 제거한다. 단일 요청 중 발생한 tree/content 변경은
best-effort로 관측한다.

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
