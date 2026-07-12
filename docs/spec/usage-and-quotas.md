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

비용이 큰 Space usage는 `space_usage`에 저장한다. Reconciliation 요청과 실행 이력은 별도 테이블에 둔다.

```text
space_usage
  space_id
  live_node_count
  live_content_bytes
  reconciled_at

space_usage_reconcile_jobs
  job_id
  space_id
  requested_at
  run_after
  retry_count

space_usage_reconcile_executions
  execution_id
  job_id
  space_id
  started_at
  finished_at
  outcome
  error_message
  metadata
```

Space 생성은 root node와 `space_usage(nodes=1, bytes=0)`를 같은 transaction에서 만든다. 이후 counter도 원본 변경과 같은 transaction에서 갱신한다. 원본 테이블은 reconciliation 기준이고 counter는 일반 쿼터 검사와 Usage 조회에 사용한다. Event log는 Usage 계산에 사용하지 않는다.

API startup은 migration 이후 usage 테이블과 Space 생성 trigger를 검증한다. Live Space에 counter row가 누락되어 있으면 자동 복구하지 않고 startup을 실패시킨다. 스키마 누락은 readiness도 실패한다. Operator는 전체 재계산 명령으로 복구한 뒤 API를 다시 시작한다.

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
acquire shared Space reconciliation gate
  -> resolve and lock the owner tier quota
  -> lock Space
  -> lock space_usage
  -> validate current counter + delta
  -> reserve the delta in space_usage
  -> mutate source rows
  -> commit
```

한도를 초과한 상태에서도 사용량을 줄이는 save/delete는 허용한다. 증가하는 metric만 effective quota와 비교한다. Counter row 누락, underflow, overflow는 원본 변경을 rollback하는 internal error다. 해당 Space의 reconciliation으로 counter를 복구한 뒤 mutation을 재시도한다.

## Reconciliation worker

정기 자동 재계산은 하지 않는다. Worker는 수동 요청 또는 operator 전체 재계산으로 생성된 job만 처리한다. DB 전체에서 활성 worker는 하나이며, job 하나를 transaction 하나로 실행한다. Tick마다 ready job이 남지 않을 때까지 연속으로 실행한다.

```text
worker tick
  -> select oldest job where run_after <= now()
  -> try exclusive Space reconciliation gate
  -> defer job for 5 minutes when the gate is busy
  -> lock the Space and space_usage
  -> COUNT/SUM live source rows
  -> upsert counters (a missing counter row is recreated)
  -> set reconciled_at = now()
  -> append succeeded execution
  -> delete job
  -> commit
  -> repeat until no ready job remains
```

- Worker는 transaction-scoped advisory lock으로 하나만 활성화한다.
- 같은 Space에는 활성 job을 하나만 허용한다.
- Deleted Space의 job은 취소하고 `cancelled` execution을 기록한다.
- File-tree mutation은 shared gate, reconciler는 exclusive gate를 사용한다. Shared gate 획득에 실패한 mutation은 DB connection을 점유하며 기다리지 않고 임시 오류를 반환한다.
- 재계산 중 해당 Space의 read는 허용하고 mutation만 일시적으로 거부한다. 다른 Space는 영향받지 않는다.
- Space gate가 busy이면 `deferred` execution을 기록하고 `run_after`를 5분 뒤로 옮긴다. `retry_count`는 증가시키지 않는다.
- 실행 실패는 savepoint까지 rollback한 뒤 `failed` execution과 error를 같은 worker transaction에 기록한다. `retry_count`를 증가시키고 5분 뒤 재시도한다.
- 성공, 지연, 실패, 취소 execution은 append-only로 기록한다. Worker lock을 획득한 프로세스가 transaction timeout 안에서 3개월이 지난 행을 정리한다.
- Space별 재계산 statement timeout은 30초, row lock timeout은 5초다.
- 프로세스 종료 시 현재 reconciliation transaction만 commit 또는 rollback까지 완료하고, queue에 남은 job은 시작하지 않는다. 대기 job은 다음 worker가 이어서 처리한다. Worker 종료 후 DB pool을 닫고, 배포 환경의 강제 종료 유예시간은 HTTP와 현재 transaction drain을 포함하도록 90초 이상으로 설정한다.

## Manual reconciliation

사용자 Refresh는 counter를 다시 조회할 뿐 재계산하지 않는다. Owner user는 특정 Space의 reconciliation을 요청할 수 있다.

```http
POST /api/v1/spaces/{space_id}/usage/reconcile
```

요청은 중복 job과 최근 reconciliation 완료 후 1시간 cooldown을 검사한 뒤 job을 생성하고 `202 Accepted`를 반환한다. HTTP 요청 안에서 COUNT/SUM을 실행하지 않는다. Agent는 요청할 수 없다.

`GET /api/v1/me/usage`의 Space별 `reconciliation.pending`은 활성 job 존재 여부를 나타내고, `reconciled_at`은 마지막 성공 시각이다. Client는 POST 이후 Usage를 다시 조회해 `pending=false`와 갱신된 `reconciled_at`을 확인한다.

## Full recalculation

전체 재계산은 초기 backfill 또는 심각한 장애 복구를 위한 maintenance 작업이다. Startup과 사용자 요청에서는 자동 실행하지 않는다. Operator만 다음 명령으로 명시적으로 실행한다.

```sh
notegate-api --recalculate-usage
```

저장소에서 실행할 때는 `cargo run -p notegate-api -- --recalculate-usage`를 사용한다.

명령은 모든 live Space의 reconciliation job을 한 번에 등록한 뒤(`ON CONFLICT`로 기존 job은 즉시 실행 가능하게만 갱신) queue가 빌 때까지 job을 하나씩 실행한다. Space 하나를 재계산하는 동안 그 Space의 mutation만 잠시 거부되고, 나머지 Space와 read는 영향받지 않으므로 서비스 운영 중에도 실행할 수 있다. 다른 worker가 lock을 쥐고 있으면 1초 간격으로 최대 30번 재시도한다. Ready job이 없어도 deferred/failed job이 queue에 남아 있으면 성공으로 처리하지 않는다. Busy로 지연되거나 실패한 Space가 있으면 오류로 종료하고 background worker가 이어서 재시도한다. 누락된 counter row는 job 실행이 다시 생성한다.

`space_usage`를 처음 배포할 때 이전 버전 API는 counter를 갱신하지 못하고 Space reconciliation gate도 지키지 않는다. 따라서 이전 버전 writer에 종료 신호를 보낸 뒤 진행 중인 요청과 worker transaction이 drain되어 모든 프로세스가 종료된 것을 확인한다. 그 다음 새 코드로 migration을 완료하고 새 API를 시작한다. Migration이 counter를 backfill하므로 별도 재계산 없이 시작할 수 있다. 이전 버전과 authoritative counter 버전을 동시에 writer로 운영하지 않는다.

## Maintenance error

재계산 때문에 차단된 REST mutation은 `503 Service Unavailable`, `Retry-After`, `kind=usage_recalculation_in_progress`를 반환한다. MCP mutation은 JSON-RPC server error에 같은 `data.kind`, `retryable=true`, `retry_after_seconds`를 반환한다. Client는 인증 상태와 편집 중인 draft를 유지하고 mutation을 자동 재실행하지 않는다.
