# Search

Command API와 MCP의 path-first `find`·`grep`은 `notegate-search`가, 일반 file-tree `tree`는 `FilesService`가 소유한다.

`tree`는 요청 내 활성 DFS frame의 자식 page와 parent path를 재사용한다. Page 크기는 남은 응답 개수와 `search_children_page_max` 중 작은 값이다. Frame을 벗어나면 버리고 요청 간에는 공유하지 않는다. Cursor는 마지막으로 소비한 자식 위치를 유지한다. 외부 결과는 반환 전 primary에서 Node/ancestor 접근과 삭제 여부를 재검증한다. 순회 중 변경은 best-effort이며 요청 전체 snapshot은 보장하지 않는다.

## Execution boundary

```text
MCP/CLI -> SearchClient -> signed private HTTP -> SearchRuntime
  -> admission -> SearchService -> PostgresSearchStore -> PostgreSQL primary/read pool
```

Runtime은 admission, service, 복호화 body cache와 telemetry를 소유한다.

| 실행 방식 | Listener와 호출 |
|---|---|
| Local Search를 포함한 `all` / `api` | Public와 Search를 다른 socket으로 실행 |
| `search` | Private Search listener만 실행; 같은 image로 별도 pod 구성 가능 |
| Remote Search | API의 `search_service_url`로 내부 service 호출 |

Private route는 Search listener에만 등록한다. 배포 설정과 topology는 [개발 가이드](../development.md#로컬-개발)를 따른다.

`notegate-search`의 store 경계는 `PostgresSearchStore`를 통해 `FilesRepo`에 storage 연산을 위임한다.

| 읽기 | DB 경계 |
|---|---|
| 권한, scope, candidate·경로·본문 해시, 현재 접근·삭제 여부, 자식 존재 여부, 잠금 | Primary; cache hit도 반환 전 접근 재검증 |
| Body, content stats | `read_database_url` 설정 시 별도 read pool, 없으면 primary |

별도 read pool은 local Search listener 소유 process만 생성한다. API `/ready`는 자신이 소유한 dependency만, Search `/ready`는 자신의 DB/schema를 검사한다. 연결 실패는 retryable `search_unavailable`이며 운영 경보는 오류율과 Search readiness를 함께 본다.

Private HMAC 계약:

- Request: timestamp + method + path + 정확한 body bytes, clock skew 최대 60초.
- Response: request timestamp + status + path + body.
- Key: LOOKUP root에서 session과 다른 purpose label로 파생.
- 서명은 service authentication/integrity를 보장한다. Transport confidentiality와 `/metrics` 접근 제한은 TLS 또는 cluster network policy가 담당한다.
- `x-request-id`는 내부 요청/응답에도 유지한다. 관측성 필드는 command 입력이나 권한 계약에 넣지 않는다.

Deadline은 ingress에서 한 번 설정한 30초를 monotonic clock으로 계산한다. API는 응답 여유 1초를 뺀 `timeout_ms`를 private envelope로 전달하고 Search는 최대 29초만 실행한다.

| 조건 | 처리 |
|---|---|
| API 잔여 시간 ≤1초 / Search `timeout_ms=0` | 검색 시작 안 함 |
| 실행 중 timeout | 작업 취소, 서명된 `504 deadline_exceeded` |
| Ingress deadline context 누락 | 새 시간을 만들지 않고 `search_unavailable` |

초과는 `notegate_search_deadline_exceeded_total{operation,phase}`와 `internal_search.deadline_exceeded` warning으로 기록한다. Label은 `operation=find|grep`, `phase=before_execution|during_execution`이다.

### Runtime contract

`/internal/v1`은 같은 NoteGate release의 process role 사이 계약이다.

| 입력/응답 | 호환 규칙 |
|---|---|
| Public Command API/MCP input | Unknown field 거부 |
| HMAC 인증 private find/grep command·Search response | Unknown field 무시 |
| 필수 필드 누락 / type 오류 | 거부 |
| 기존 필드·enum | 이름, type, 의미 유지; 호환되지 않는 변경은 새 version path |

선택 필드는 **Search 배포 → readiness 확인 → API 배포** 순서로 활성화한다. 날짜 없는 이전 API 요청도 새 Search가 처리한다.

API client는 서명된 status/body가 모순되거나 성공 status가 `200 OK`가 아니면 `search_unavailable`로 처리한다.

| Private error kind | HTTP status | Public error code |
|---|---:|---|
| `invalid_input` | `400` | `invalid_input` |
| `forbidden` | `403` | `forbidden` |
| `not_found` | `404` | `not_found` |
| `conflict` | `409` | `conflict` |
| `write_locked` + `scope` | `423` | `node_write_locked` / `subtree_write_locked` |
| `search_busy` | `429` | `search_busy` |
| `usage_recalculation_in_progress` | `503` | `usage_recalculation_in_progress` |
| `deadline_exceeded` | `504` | `deadline_exceeded` |
| `internal_error` | `500` | `internal_error` |

MCP dependency/maintenance 임시 실패는 `-32001`, process capacity 거부는 `-32002`다. 분기는 숫자뿐 아니라 `data.code`로 판단한다.

## Authorization

Read permission이 필요하다. User는 소유 Space, Agent는 active connection의 `read` 또는 `write` Space만 검색한다. 권한이 없으면 존재 여부를 숨긴다. Scope는 folder subtree이며 생략하면 Space root `/`다.

## Result shape

| Command | 결과 |
|---|---|
| `find` | `McpNodeSummary[] + Page` |
| `grep` | `McpGrepSummary[] + Page` |

[Schema](schemas.md)가 정본이다. 본문·metadata는 응답에 넣지 않고 `read op=stat` / `read op=read`로 조회한다.

## Common traversal

Scope 아래 deterministic DFS pre-order, sibling 순서는 `sort_order, name, id`다.

`external_access_enabled=false`인 Node와 하위는 제외한다. Folder를 ON으로 바꾸면 자체 설정이 ON인 하위의 접근이 복구되며 자식 설정은 변경하지 않는다. OFF folder를 직접 scope로 지정하면 `not_found`다. 반환 전 접근 검증에서 제외된 후보도 cursor는 소비한 위치 이후로 진행한다.

## Pagination and scan budget

Result limit은 반환 item 수, scan budget은 검사할 candidate 양이다. Result limit·scan budget·subtree 끝 중 먼저 도달한 곳에서 멈춘다. Budget 안에서 match가 없어도 후보가 남으면 다음 응답이 유효하다.

```json
{"items":[],"page":{"limit":20,"returned":0,"has_more":true,"next_cursor":"..."}}
```

## Date filters

`created_from`, `created_to`, `updated_from`, `updated_to`는 선택적인 timezone 포함 RFC 3339 timestamp다. Command boundary에서 UTC로 정규화하며 internal Search도 typed range를 검증한다.

- 범위는 `from <= node timestamp < to`; 생략한 경계는 무제한, 지정 조건은 AND.
- 같은 날짜 종류의 `from >= to`는 invalid input.
- `updated_at`는 본문·이름·이동·설정을 포함한 현재 Node 변경 시각이며 수정 이력 검색이 아니다.
- Recursive subtree 순회 후, candidate `LIMIT` 전에 적용한다. 날짜가 맞지 않는 folder도 하위 탐색을 계속한다.
- Grep cache·본문 읽기·복호화는 날짜를 통과한 후보에만 수행한다. 접근 정책, DFS 순서, scan budget은 유지한다.
- 날짜는 cursor fingerprint에 포함한다. 전부 생략하면 기존 fingerprint를 유지한다. 페이지 사이 날짜 변경은 best-effort다.

날짜 인덱스는 추가하지 않는다. Recursive CTE 결과 필터에 인덱스만 추가해도 subtree 순회는 줄지 않으며 기존 child/recent 인덱스를 유지한다. CI 합성 workload는 조건 없음/생성일/수정일/둘 다의 후보 수·후보 본문 byte 합계·candidate query 시간을 비교한다. 운영 latency나 실제 본문 읽기량 측정은 아니다. 배포 순서는 [Runtime contract](#runtime-contract)를 따른다.

## Two-stage search pipeline

| 단계 | 책임 |
|---|---|
| DB candidate scan | Live subtree, DFS `sort_path`, 날짜 필터, cursor 이후 bulk 조회 |
| App matcher | Rust regex dialect, 이름/본문·path match, result limit·scan budget |

DB는 traversal/bulk read, application은 match semantics를 소유한다. Rust regex로 backtracking 위험을 피한다. Raw recursive CTE 반환 순서에 의존하지 않고 명시적 `ORDER BY sort_path` 또는 동등한 정렬을 사용한다.

## Cursor state

Opaque string의 논리 상태:

```ts
type SearchCursor = {
  version: number
  command: "find" | "grep"
  fingerprint: string
  scope_node_id: string
  after_sort_path?: string
}
```

`after_sort_path`는 마지막 match가 아닌 마지막 소비 후보다. 다음 page는 그 이후부터 검사한다. Fingerprint는 Space, scope, `q`, match mode, kind, include/exclude, 날짜, case policy와 traversal order를 묶는다. 조건이 다르면 invalid cursor다. `sort_path`는 응답 schema나 DB 저장 model이 아닌 내부 pagination key이며 순회 중 변경은 best-effort다.

## Candidate scan algorithm

```text
read permission -> live folder scope resolve -> cursor 검증
-> 날짜 조건을 통과한 DFS 후보 bulk 조회 -> command matcher
-> 반환 전 접근 재검증 -> result/page
```

Limit/budget에서 멈추면 소비 위치로 `next_cursor`를 만들고, subtree 끝이면 `has_more=false`다.

### `find` candidate scan

Node summary만 검사하며 content/metadata를 읽지 않는다. Root와 외부 접근 불가 후보는 결과에서 제외하고 kind와 derived-path include/exclude를 통과한 이름을 검사한다.

| Match mode | Node name 검사 |
|---|---|
| `contains` | Substring |
| `regex` | Rust regex |
| `glob` | Glob |

대소문자를 구분하지 않는다. Glob/regex는 명시적으로 선택하므로 `*.md`는 glob mode에서만 pattern이다. `q`는 이름, include/exclude glob list는 derived path에만 적용한다.

### `grep` candidate scan

외부 접근 가능한 `kind=text`, `storage_format=plain` 본문을 검사한다. 서버 at-rest 암호화는 복호화하지만 client-side encrypted Text, File, Node metadata는 검사하지 않는다. 결과는 Text 후보와 선택적인 matching line number이며 context/snippet은 없다. 본문은 `read op=read`로 조회한다.

| 옵션 | 의미 |
|---|---|
| `literal` / `regex` | 대소문자를 구분하지 않는 substring / Rust regex |
| Line `none` / `first` / `all` | 번호 생략 / 첫 matching line / 모든 matching line |
| Include/exclude | 각각 glob 최대 32개, pattern당 최대 256자 |

Line은 1-based logical line이며 regex도 line별 평가한다. Cross-line match는 지원하지 않는다. Text 하나는 `text_max_bytes` 이하의 atomic scan unit이다. 다음 Text의 `byte_len`이 잔여 budget을 넘으면 검사 전에 멈추고 해당 Text부터 재개하는 cursor를 반환한다. Text 내부 line offset cursor는 없다.

## Decrypted body cache

Process-local cache key는 `(space_id, node_id, content_sha256)`다.

| 경로 | 처리 |
|---|---|
| Candidate metadata | 매번 현재 SHA, `byte_len`, `line_count`, `at_rest_encryption` 조회 |
| Cache hit | PostgreSQL body query와 복호화 생략; primary 접근 검증 유지 |
| Cache miss | 8 MiB request budget 내 miss를 한 번에 bulk 조회; live/plain/접근/SHA/byte_len 재검증·복호화 후 `Arc<str>` 저장 |
| 겹치는 concurrent miss | Key별 load flight 공유; 대기 후 cache 재확인, 남은 miss만 조회. 겹치지 않는 key 집합은 독립 실행 |

본문 변경 transaction이 SHA를 갱신하므로 다음 검색은 새 key를 사용한다. 이전 entry는 아래 정책으로 제거하며 단일 요청 중 변경은 best-effort다.

| 정책 | 기본값 |
|---|---|
| Capacity | Process당 plaintext byte weight 128 MiB; `0`은 비활성화 |
| Eviction/admission | TinyLFU |
| TTL / TTI | 삽입 후 30분 / 마지막 hit 후 5분 |

본문만 저장한다. Candidate, folder page, 결과와 replica 간 공유/coherence는 없다.

## Worst-case scan and memory model

전체 scope를 끝까지 탐색할 수 있지만 여러 page로 나눠 읽는다. Space quota는 [전역 상한](performance-limits.md#tree-and-content-limits)을 따른다. Root의 최대 논리 범위는 system_max 25,000 nodes·live Text 1 GiB, tier0 2,000 nodes·128 MiB다.

| 요청별 budget | 상한 |
|---|---:|
| DB candidate inspect / Node scan | 1,000 summaries |
| Grep scan / Text read total | 8 MiB |
| Response result limit | 100 summaries |
| Include / exclude glob | 각각 32 × 256자 |
| Response body target | 256 KiB |
| Body cache | 기본 process당 plaintext weight 128 MiB |

큰 scope는 빈 결과 page를 포함해 cursor로 계속 탐색하며 마지막에 `has_more=false`가 된다. Response byte 값은 target이며 cache 용량은 요청별이 아니라 process 전체에 적용된다.
