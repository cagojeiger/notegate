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
Space    stored_text_bytes           stored counter     tier + runtime cap
Space    stored_file_bytes           stored counter     tier + runtime cap
Folder   live_children             live count         tier + runtime cap
Text     object_bytes/lines        request/object     hard limit
File     object_bytes              request/object     hard limit
```

작고 상한이 낮은 값은 요청 시 정확히 계산한다. Space 전체를 반복해서 스캔해야 하는 node 수, Text bytes, File bytes만 counter로 저장한다. 일반화는 공통 scope/metric 모델과 API shape에 적용하고, persistence는 typed table을 사용한다.

`GET /api/v1/me/usage`는 Storage 화면에 필요한 소유 Space별 `items`, `text_bytes`, `file_bytes`를 반환한다. `items`는 내부 Space root를 제외한 값이다. User, Account, Agent, connection 범위의 quota는 해당 리소스 API에서 검사하며 Usage 응답에 합치지 않는다.

## Usage semantics

Items는 live 상태, Text/File quota는 아직 보관 중인 bytes를 기준으로 한다.

- Live node 수에는 Space root를 포함하지만 화면의 `items.used`에서는 제외한다.
- Text bytes는 휴지통을 포함한 `text_objects.byte_len` 합이다. 실제 DB row 삭제가 commit될 때 반환한다.
- File bytes는 object 원장의 `attached` + `delete_pending` 선언 크기 합이다. S3 DeleteObject 성공 후 완료 transaction이 commit될 때 반환한다.
- Soft delete와 영구 삭제 요청만으로 bytes를 반환하지 않는다. 복원은 이미 계산된 bytes를 다시 더하지 않는다.
- 미완료 upload는 별도 예약량으로 업로드 시작 시 합산하며 attach 시 저장 용량으로 전환한다. 실패한 upload 예약은 expiry cleanup 완료까지 유지한다.
- 문서 과거 본문은 별도 `text_revision_usage` 예산을 사용하고 revision DELETE와 함께 반환한다. Node metadata, event history, DB/S3 내부 overhead는 Text/File quota에 포함하지 않는다.
- 삭제된 Space도 보관 bytes를 조회할 수 있다. DB에서 Space가 제거된 뒤에는 이름 대신 `Deleted space`와 ID를 반환한다. 소유자만 볼 수 있다.
- 사용자 전체 content quota는 없다. Text/File quota는 Space별로 독립 적용한다.

S3 acknowledgement는 NoteGate의 quota 반환 경계다. 저장소 내부 GC나 디스크 공간 반환을 확인하지 않는다. Versioning bucket의 DeleteObject는 delete marker만 만들 수 있으므로 provider의 과거 object version 정리는 별도 운영 책임이다. [S3 DeleteObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObject.html)

## Space usage counter

활성 항목은 `space_usage`, 보관 용량은 `space_storage_usage`에 저장한다. Reconciliation 요청과 실행 이력은 범용 background job queue에 둔다.

```text
space_usage
  space_id
  live_node_count
  live_text_bytes
  live_file_bytes
  reconciled_at

space_storage_usage
  space_id (no FK)
  owner_user_id (no FK)
  text_bytes
  file_bytes

background_jobs
  job_kind = 'space_usage_reconcile'
  payload = {"space_id": ...}
```

Space 생성은 root node와 두 counter row를 같은 transaction에서 만든다. `space_storage_usage`는 Space 삭제 뒤에도 유지하며 비어 있고 nonterminal object 참조가 없을 때 bounded cleanup으로 제거한다. 이후 counter도 원본 변경과 같은 transaction에서 갱신한다. 원본 테이블은 reconciliation 기준이고 counter는 일반 쿼터 검사와 Usage 조회에 사용한다. Event log는 Usage 계산에 사용하지 않는다.

API startup은 migration 이후 usage 테이블과 Space 생성 trigger를 검증한다. Live Space에 counter row가 누락되어 있으면 자동 복구하지 않고 startup을 실패시킨다. 스키마 누락은 readiness도 실패한다. Operator는 전체 재계산 명령으로 복구한 뒤 API를 다시 시작한다.

```text
Operation               nodes          text bytes       file bytes
Space 생성              +1              0                0
Folder 생성             +1              0                0
Text 생성               +1             +new              0
File 생성               +1              0               +new
Text 내용 변경           0              +(new - old)      0
같은 Space 안 이동       0               0                0
Text-only subtree 복사  +count          +text sum         0
Subtree soft delete     -count          -text sum        -file sum
Soft-deleted row purge   0               0                0
No-op 변경               0               0                0
```

원본 변경, counter 증감, file change event 기록은 모두 성공하거나 모두 rollback되어야 한다.

복사 대상과 형식은 [`files-commands.md`](./files-commands.md#copy-semantics)의 공통 command 계약을 따른다.

저장 용량은 source-table trigger가 같은 transaction에서 갱신한다. Text는 INSERT/byte_len UPDATE/DELETE, File은 `attached` 또는 `delete_pending`에 진입하거나 나올 때의 delta를 반영한다. 위 표는 기존 live counter 기준이며 soft delete는 저장 용량을 변경하지 않는다. S3 완료는 object 상태 전환, 저장 용량 반환, `object.delete` Audit receipt를 함께 commit한다. 실패/timeout은 용량을 유지하며 반복 완료는 no-op이다.

Migration은 기존 휴지통과 pending object를 포함해 backfill한다. 이미 Space FK가 끊긴 과거 object는 owner/Space를 추측하지 않아 이 집계에 포함할 수 없다. 구버전 프로세스도 trigger로 counter를 유지하지만, 새 quota 정책은 모든 writer가 업그레이드된 뒤 적용된다.

## Quota enforcement

File-tree mutation은 Space를 잠근 transaction 안에서 변경 후 예상 counter를 계산한다. 예상 값이 effective tier quota를 넘으면 원본과 counter를 변경하지 않고 `409 conflict`로 거부한다.

```text
acquire shared Space reconciliation gate
  -> resolve and lock the owner tier quota
  -> lock Space
  -> lock space_usage and space_storage_usage
  -> validate live nodes and stored byte deltas
  -> update live counters
  -> mutate source rows (triggers update stored counters)
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
  -> lock Space, live counters and stored counters
  -> COUNT/SUM live nodes/content and all retained Text/File source rows
  -> upsert counters (a missing counter row is recreated)
  -> set reconciled_at = now()
  -> commit
  -> mark queue attempt succeeded
```

- Queue는 중복 job을 허용하지만 정확한 재계산은 멱등이다. 수동 요청 경로는 동일 Space의 활성 job을 검사해 사용자 중복 요청을 차단한다.
- Deleted Space는 수동 reconciliation을 제공하지 않으며 기존 job은 성공으로 종료한다. 저장 용량 trigger는 삭제 완료까지 계속 동작한다.
- 저장 counter row lock은 S3 완료와 재계산을 직렬화하여 동시 완료의 delta가 유실되지 않게 한다.
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
POST /api/v1/spaces/{space_id}/actions/reconcile-usage
```

요청은 중복 job과 최근 reconciliation 완료 후 1시간 cooldown을 검사한 뒤 job을 생성하고 `202 Accepted`와 공통 `AsyncCommandAck`를 반환한다. 중복 job은 새 job을 만들지 않고 `202`와 `result=already_pending`을 반환한다. Cooldown은 `409 usage_reconciliation_cooldown`으로 구분한다. HTTP 요청 안에서 COUNT/SUM을 실행하지 않고 command API는 내부 job ID를 노출하지 않는다. Agent는 요청할 수 없다.

`GET /api/v1/me/usage`의 Space별 `reconciliation.status`는 활성 작업 여부를 나타낸다. `reconciliation.availability`는 실행 가능 여부와 cooldown 종료 시각을 제공한다. Client는 이 서버 상태와 현재 POST mutation 상태를 함께 사용해 버튼을 비활성화하고, POST 이후 같은 Usage cache를 갱신한다.

## Full recalculation

전체 재계산은 운영자가 명시적으로 수행하는 maintenance/recovery 작업이다. Startup과 사용자 요청에서는 자동 실행하지 않는다.

```sh
notegate-api --recalculate-usage
```

저장소에서 실행할 때는 `cargo run -p notegate-api -- --recalculate-usage`를 사용한다.

명령은 현재 live Space를 ID 순서로 조회하고 같은 정확한 재계산 함수를 동기적으로 실행한다. Space 하나를 재계산하는 동안 그 Space의 mutation만 잠시 거부되고, 나머지 Space와 read는 영향받지 않는다. 다른 worker가 해당 Space gate를 쥐고 있거나 한 Space라도 실패하면 명령은 오류로 종료한다. 누락된 counter row는 재계산이 다시 생성한다. 이 operator 경로는 background job을 만들거나 queue가 비기를 기다리지 않는다.

## Maintenance error

재계산 때문에 차단된 REST mutation은 `503 Service Unavailable`, `Retry-After`, `kind=usage_recalculation_in_progress`를 반환한다. MCP mutation은 JSON-RPC server error에 같은 `data.kind`, `retryable=true`, `retry_after_seconds`를 반환한다. Client는 인증 상태와 편집 중인 draft를 유지하고 mutation을 자동 재실행하지 않는다.
