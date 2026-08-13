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
Space    live_text_bytes           stored counter     tier + runtime cap
Space    live_file_bytes           stored counter     tier + runtime cap
Folder   live_children             live count         tier + runtime cap
Text     object_bytes/lines        request/object     hard limit
File     object_bytes              request/object     hard limit
```

작고 상한이 낮은 값은 요청 시 정확히 계산한다. Space 전체를 반복해서 스캔해야 하는 node 수, Text bytes, File bytes만 counter로 저장한다. 일반화는 공통 scope/metric 모델과 API shape에 적용하고, persistence는 typed table을 사용한다.

`GET /api/v1/me/usage`는 Storage 화면에 필요한 소유 Space별 `items`, `text_bytes`, `file_bytes`를 반환한다. `items`는 내부 Space root를 제외한 값이다. User, Account, Agent, connection 범위의 quota는 해당 리소스 API에서 검사하며 Usage 응답에 합치지 않는다.

## Usage semantics

Usage는 역대 누적량이 아니라 현재 live 상태다. 생성은 사용량을 늘리고, soft delete는 사용량을 줄인다.

- Live node 수에는 Space root node를 포함한다.
- Storage 화면은 사용자가 볼 수 있는 Folder, Text, File 수를 `Items`로 표시하며 Space root node는 제외한다.
- Text bytes는 live Text node에 연결된 `text_objects.byte_len`의 합이다.
- File bytes는 live File node에 연결된 `file_objects.byte_len`의 합이다.
- Node metadata와 event history는 Text/File bytes에 포함하지 않는다.
- Soft-deleted node와 deleted space는 Usage 응답에서 제외한다.
- 사용자 전체 content quota는 없다. Text/File quota는 Space별로 독립 적용한다.

## Space usage counter

비용이 큰 Space usage는 `space_usage`에 저장한다. Reconciliation 요청과 실행 이력은 범용 background job queue에 둔다.

```text
space_usage
  space_id
  live_node_count
  live_text_bytes
  live_file_bytes
  reconciled_at

background_jobs
  job_kind = 'space_usage_reconcile'
  payload = {"space_id": ...}
```

Space 생성은 root node와 `space_usage(nodes=1, text_bytes=0, file_bytes=0)`를 같은 transaction에서 만든다. 이후 counter도 원본 변경과 같은 transaction에서 갱신한다. 원본 테이블은 reconciliation 기준이고 counter는 일반 쿼터 검사와 Usage 조회에 사용한다. Event log는 Usage 계산에 사용하지 않는다.

API startup은 migration 이후 usage 테이블과 Space 생성 trigger를 검증한다. Live Space에 counter row가 누락되어 있으면 자동 복구하지 않고 startup을 실패시킨다. 스키마 누락은 readiness도 실패한다. Operator는 전체 재계산 명령으로 복구한 뒤 API를 다시 시작한다.

```text
Operation               nodes          text bytes       file bytes
Space 생성              +1              0                0
Folder 생성             +1              0                0
Text 생성               +1             +new              0
File 생성               +1              0               +new
Text 내용 변경           0              +(new - old)      0
같은 Space 안 이동       0               0                0
Metadata 변경            0               0                0
Text-only subtree 복사  +count          +text sum         0
Subtree soft delete     -count          -text sum        -file sum
Soft-deleted row purge   0               0                0
No-op 변경               0               0                0
```

원본 변경, counter 증감, file change event 기록은 모두 성공하거나 모두 rollback되어야 한다.

File node 또는 File을 포함한 subtree 복사는 지원하지 않는다.

## Quota enforcement

File-tree mutation은 Space를 잠근 transaction 안에서 변경 후 예상 counter를 계산한다. 예상 값이 effective tier quota를 넘으면 원본과 counter를 변경하지 않고 `409 conflict`로 거부한다.

```text
acquire shared Space reconciliation gate
  -> resolve and lock the owner tier quota
  -> lock Space
  -> lock space_usage
  -> validate current counters + deltas
  -> reserve the delta in space_usage
  -> mutate source rows
  -> commit
```

한도를 초과한 상태에서도 사용량을 줄이는 save/delete는 허용한다. 증가하는 metric만 해당 Text 또는 File effective quota와 비교한다. Counter row 누락, underflow, overflow는 원본 변경을 rollback하는 internal error다. 해당 Space의 reconciliation으로 counter를 복구한 뒤 mutation을 재시도한다.

## Reconciliation worker

정기 자동 재계산은 하지 않는다. API background runtime은 수동 요청으로 등록된 `space_usage_reconcile` job만 처리한다. 여러 API replica가 서로 다른 job을 병렬 처리할 수 있지만, Space별 reconciliation gate가 같은 Space의 재계산과 mutation을 직렬화한다.

```text
worker claim
  -> select ready job with FOR UPDATE SKIP LOCKED
  -> try exclusive Space reconciliation gate
  -> retry after 5 seconds when the gate is busy
  -> lock the Space and space_usage
  -> COUNT/SUM live Text/File source rows
  -> upsert counters (a missing counter row is recreated)
  -> set reconciled_at = now()
  -> commit
  -> mark queue attempt succeeded
```

- Queue는 중복 job을 허용하지만 정확한 재계산은 멱등이다. 수동 요청 경로는 동일 Space의 활성 job을 검사해 사용자 중복 요청을 차단한다.
- Deleted Space의 job은 성공으로 종료한다.
- File-tree mutation은 shared gate, reconciler는 exclusive gate를 사용한다. Shared gate 획득에 실패한 mutation은 DB connection을 점유하며 기다리지 않고 임시 오류를 반환한다.
- 재계산 중 해당 Space의 read는 허용하고 mutation만 일시적으로 거부한다. 다른 Space는 영향받지 않는다.
- Space gate가 busy이거나 실행이 실패하면 queue attempt를 닫고 재시도한다. 최대 attempt를 소진하면 `dead`가 된다.
- 성공과 실패 attempt는 `background_job_attempts`에 기록하고 terminal job과 함께 90일 동안 보관한다.
- Space별 재계산 statement timeout은 30초, row lock timeout은 5초다.
- 프로세스 종료 시 실행 중 handler를 취소하고 해당 attempt를 즉시 재시도 가능 상태로 돌린다. 비정상 종료로 상태 전이를 못 하면 lease recovery가 이어서 처리한다.

Queue 공통 계약은 `background-jobs.md`를 따른다.

## Manual reconciliation

사용자 Refresh는 counter를 다시 조회할 뿐 재계산하지 않는다. Owner user는 특정 Space의 reconciliation을 요청할 수 있다.

```http
POST /api/v1/spaces/{space_id}/usage/reconcile
```

요청은 중복 job과 최근 reconciliation 완료 후 1시간 cooldown을 검사한 뒤 job을 생성하고 `202 Accepted`를 반환한다. 중복 job은 `409 usage_reconciliation_pending`, cooldown은 `409 usage_reconciliation_cooldown`으로 구분한다. HTTP 요청 안에서 COUNT/SUM을 실행하지 않는다. Agent는 요청할 수 없다.

`GET /api/v1/me/usage`의 Space별 `reconciliation_pending`은 활성 job 존재 여부를 나타낸다. Client는 POST 이후 Usage를 다시 조회해 `reconciliation_pending=false`를 확인한다. 마지막 성공 시각은 내부 cooldown과 운영 진단에 사용하며 Usage 응답에는 노출하지 않는다.

## Full recalculation

전체 재계산은 운영자가 명시적으로 수행하는 maintenance/recovery 작업이다. Startup과 사용자 요청에서는 자동 실행하지 않는다.

```sh
notegate-api --recalculate-usage
```

저장소에서 실행할 때는 `cargo run -p notegate-api -- --recalculate-usage`를 사용한다.

명령은 현재 live Space를 ID 순서로 조회하고 같은 정확한 재계산 함수를 동기적으로 실행한다. Space 하나를 재계산하는 동안 그 Space의 mutation만 잠시 거부되고, 나머지 Space와 read는 영향받지 않는다. 다른 worker가 해당 Space gate를 쥐고 있거나 한 Space라도 실패하면 명령은 오류로 종료한다. 누락된 counter row는 재계산이 다시 생성한다. 이 operator 경로는 background job을 만들거나 queue가 비기를 기다리지 않는다.

## Maintenance error

재계산 때문에 차단된 REST mutation은 `503 Service Unavailable`, `Retry-After`, `kind=usage_recalculation_in_progress`를 반환한다. MCP mutation은 JSON-RPC server error에 같은 `data.kind`, `retryable=true`, `retry_after_seconds`를 반환한다. Client는 인증 상태와 편집 중인 draft를 유지하고 mutation을 자동 재실행하지 않는다.
