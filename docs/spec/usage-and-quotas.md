# Usage and quotas

이 문서는 현재 사용량을 계산하고 tier quota를 적용하는 계약의 정본이다. Tier별 숫자는 `performance-limits.md`, DB 구조는 `db.md`, REST 응답은 `rest/identity.md`를 따른다.

## Usage semantics

Usage는 역대 누적량이 아니라 현재 live 상태다. 생성은 사용량을 늘리고, soft delete는 사용량을 줄인다.

```text
Account usage
  live owned spaces
  active owned agents
  live user API keys

Space usage
  live nodes
  live Text + File content bytes
  active agent connections
```

- Live node 수에는 Space root node를 포함한다.
- Content bytes는 live node에 연결된 `text_objects.byte_len`과 `file_objects.byte_len`의 합이다.
- Node metadata와 file history는 content bytes에 포함하지 않는다.
- Soft-deleted node와 deleted space는 Usage 응답에서 제외한다.
- 사용자 전체 content quota는 없다. Content quota는 Space별로 적용한다.

## Stored counters

비용이 큰 Space node 수와 content bytes만 `spaces`에 현재 값으로 저장한다.

```text
spaces.live_node_count
spaces.live_content_bytes
```

Space, Agent, API key, connection 수는 각 상한이 작으므로 조회 시 live row를 계산한다.

Usage counter는 원본 변경과 같은 DB transaction에서 갱신한다. 원본 테이블은 전체 재계산의 기준이고, counter는 일반 쿼터 검사와 Usage 조회에 사용한다. Event log는 Usage 계산에 사용하지 않는다.

## Mutation rules

```text
Space 생성              nodes +1       bytes  0
Folder 생성             nodes +1       bytes  0
Text/File 생성          nodes +1       bytes +new
Text 내용 변경          nodes  0       bytes +(new - old)
같은 Space 안 이동      nodes  0       bytes  0
Metadata 변경           nodes  0       bytes  0
Subtree 복사            nodes +count    bytes +sum
Subtree soft delete     nodes -count    bytes -sum
Soft-deleted row purge  nodes  0       bytes  0
No-op 변경              nodes  0       bytes  0
```

원본 변경, counter 증감, file change event 기록은 모두 성공하거나 모두 rollback되어야 한다.

## Quota enforcement

File-tree mutation은 Space를 잠근 transaction 안에서 변경 후 예상 counter를 계산한다. 예상 값이 effective tier quota를 넘으면 원본과 counter를 변경하지 않고 `409 conflict`로 거부한다.

```text
lock Space
  -> resolve effective tier quota
  -> validate current counter + delta
  -> mutate source rows
  -> update counter
  -> commit
```

## Full recalculation

전체 재계산은 초기 backfill과 명시적인 운영 복구에만 사용한다. 정기적인 Usage 갱신 작업이 아니다.

```text
recalculation start
  -> wait for in-flight product mutations
  -> block new product mutations
  -> keep reads available
  -> COUNT/SUM all live source rows
  -> replace counters in one DB transaction
  -> commit and allow mutations
```

- 재계산 중 REST와 MCP read는 계속 허용한다.
- 재계산 중 새 mutation은 대기시키지 않고 retry 가능한 임시 오류로 거부한다.
- Reader는 재계산 commit 전에는 이전 counter를, commit 후에는 새 counter를 본다. 중간 결과는 노출하지 않는다.
- 재계산 실패 시 counter 변경을 모두 rollback하고 mutation 차단을 해제한다.
- 전체 재계산은 HTTP request timeout 안에서 실행하지 않고 운영 작업으로 실행한다.

## Maintenance error

REST mutation은 `503 Service Unavailable`, `Retry-After`, `kind=usage_recalculation_in_progress`를 반환한다. MCP mutation은 JSON-RPC server error에 같은 `data.kind`, `retryable=true`, `retry_after_seconds`를 반환한다. MCP read tool은 정상 동작한다.

Client는 인증 상태와 편집 중인 draft를 유지한다. Mutation을 자동 재실행하지 않고 `Retry-After` 이후 사용자가 다시 시도할 수 있게 한다.
