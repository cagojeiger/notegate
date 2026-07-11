# Usage and quotas

이 문서는 현재 사용량을 계산하고 quota를 적용하는 계약의 정본이다. Tier별 숫자는 `performance-limits.md`, DB 구조는 `db.md`, REST 응답은 `rest/identity.md`와 `rest/spaces.md`를 따른다.

## General model

Quota는 `scope + metric + used + limit`으로 표현한다. REST 응답은 계산 방법과 관계없이 `{used, limit}` 형태를 사용한다.

```text
Scope    Metric                    Usage source       Limit source
User     owned_spaces              live count         tier
User     active_agents             live count         tier
Account  live_api_keys             live count         hard limit
Space    active_connections        live count         tier
Agent    connected_spaces          live count         tier
Space    live_nodes                stored counter     tier + runtime cap
Space    live_content_bytes        stored counter     tier + runtime cap
Folder   live_children             live count         tier + runtime cap
Text     object_bytes/lines        request/object     hard limit
File     object_bytes              request/object     hard limit
```

작고 상한이 낮은 값은 요청 시 정확히 계산한다. Space 전체를 반복해서 스캔해야 하는 node 수와 content bytes만 counter로 저장한다. 일반화는 공통 scope/metric 모델과 API shape에 적용하고, persistence는 typed table을 사용한다.

## Usage semantics

Usage는 역대 누적량이 아니라 현재 live 상태다. 생성은 사용량을 늘리고, soft delete는 사용량을 줄인다.

- Live node 수에는 Space root node를 포함한다.
- Content bytes는 live node에 연결된 `text_objects.byte_len`과 `file_objects.byte_len`의 합이다.
- Node metadata와 event history는 content bytes에 포함하지 않는다.
- Soft-deleted node와 deleted space는 Usage 응답에서 제외한다.
- 사용자 전체 content quota는 없다. Content quota는 Space별로 적용한다.

## Space usage counter

비용이 큰 Space usage와 reconciliation schedule은 `space_usage`에 저장한다.

```text
space_usage
  space_id
  live_node_count
  live_content_bytes
  reconciled_at
  next_reconcile_at
```

Space 생성은 root node와 `space_usage(nodes=1, bytes=0)`를 같은 transaction에서 만든다. 이후 counter도 원본 변경과 같은 transaction에서 갱신한다. 원본 테이블은 reconciliation 기준이고 counter는 일반 쿼터 검사와 Usage 조회에 사용한다. Event log는 Usage 계산에 사용하지 않는다.

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
acquire shared full-recalculation gate
  -> acquire shared Space reconciliation gate
  -> resolve and lock the owner tier quota
  -> lock Space
  -> lock space_usage
  -> validate current counter + delta
  -> reserve the delta in space_usage
  -> mutate source rows
  -> commit
```

한도를 초과한 상태에서도 사용량을 줄이는 save/delete는 허용한다. 증가하는 metric만 effective quota와 비교한다. Counter row 누락, underflow, overflow는 원본 변경을 rollback하는 internal error다. Reconciliation 또는 전체 재계산으로 counter를 복구한 뒤 mutation을 재시도한다.

## Distributed reconciliation

Reconciliation은 전체 Space를 한 번에 처리하지 않는다. DB 전체에서 활성 worker는 하나이며, 한 번에 due Space 하나를 처리한다.

```text
worker tick
  -> select oldest next_reconcile_at <= now()
  -> try exclusive Space reconciliation gate
  -> skip when the gate is busy
  -> lock the Space and space_usage
  -> COUNT/SUM live source rows
  -> replace counters when different
  -> set reconciled_at = now()
  -> schedule the next run around 7 days later with bounded jitter
  -> commit
```

- 순수 `ORDER BY random()`을 사용하지 않는다. Randomness는 `next_reconcile_at`을 시간대에 분산하는 데만 사용한다.
- 새 Space와 backfill row의 첫 실행 시각은 다음 7일에 분산한다.
- Deleted Space는 schedule 대상에서 제외하고 hard purge 시 usage row도 cascade delete한다.
- File-tree mutation은 shared gate, reconciler는 exclusive gate를 사용한다. Shared gate 획득에 실패한 mutation은 DB connection을 점유하며 기다리지 않고 임시 오류를 반환한다.
- 재계산 중 해당 Space의 read는 허용하고 mutation만 일시적으로 거부한다. 다른 Space는 영향받지 않는다.
- 실패하면 counter를 rollback하고 해당 Space를 나중에 재시도한다.
- Space별 재계산 statement timeout은 30초, row lock timeout은 5초다. Timeout은 transaction 전체를 rollback하며 다음 tick에서 재시도한다.
- 운영은 가장 오래 지연된 `next_reconcile_at`을 reconciliation lag로 관찰한다. Lag가 지속적으로 증가할 때만 worker 병렬화를 검토한다.

## Manual reconciliation

사용자 Refresh는 counter를 다시 조회할 뿐 재계산하지 않는다. Owner user는 특정 Space의 reconciliation을 요청할 수 있다.

```http
POST /api/v1/spaces/{space_id}/usage/reconcile
```

요청은 중복 요청과 최근 reconciliation 완료 후 1시간 cooldown을 검사한 뒤 `next_reconcile_at=now()`로 앞당기고 `202 Accepted`를 반환한다. HTTP 요청 안에서 COUNT/SUM을 실행하지 않는다. Agent는 요청할 수 없다.

## Full recalculation

전체 재계산은 초기 backfill 또는 심각한 장애 복구를 위한 관리자 작업이다. 정기 schedule이나 사용자 요청으로 실행하지 않는다. 다음 명령은 migration을 확인한 뒤 전체 재계산을 실행한다.

```sh
cargo run -p notegate-api --example recalculate_usage
```

명령은 worker lock 또는 진행 중인 mutation 때문에 gate를 얻지 못하면 1초 간격으로 최대 30번 재시도한다. Gate를 얻으면 read는 허용하고 file-tree mutation은 retry 가능한 임시 오류로 거부한다. Counter 교체, `reconciled_at` 갱신, 다음 실행 시각 분산은 하나의 DB transaction으로 commit하거나 모두 rollback한다. 전체 재계산 statement timeout은 5분, row lock timeout은 5초다.

`space_usage`를 처음 배포할 때 이전 버전 API는 counter와 maintenance gate를 갱신하지 못한다. 따라서 이전 버전 writer를 모두 중지하고, 새 코드로 migration과 전체 재계산을 완료한 다음 새 API를 시작한다. 이전 버전과 authoritative counter 버전을 동시에 writer로 운영하지 않는다.

## Maintenance error

재계산 때문에 차단된 REST mutation은 `503 Service Unavailable`, `Retry-After`, `kind=usage_recalculation_in_progress`를 반환한다. MCP mutation은 JSON-RPC server error에 같은 `data.kind`, `retryable=true`, `retry_after_seconds`를 반환한다. Client는 인증 상태와 편집 중인 draft를 유지하고 mutation을 자동 재실행하지 않는다.
