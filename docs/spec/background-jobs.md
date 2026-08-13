# 백그라운드 작업

NoteGate의 지연 가능한 내부 작업은 PostgreSQL 기반 범용 queue를 사용한다. Queue는 작업 전달과 실행 상태만 책임지고, payload 의미와 멱등성은 각 application handler가 책임진다.

## 보장 범위

- 전달 보장은 `at-least-once`다. 성공 응답을 기록하기 전에 worker가 종료되면 같은 작업이 다시 실행될 수 있다.
- 작업 등록과 원본 변경은 같은 PostgreSQL transaction에 넣을 수 있다. 원본만 저장되거나 작업만 등록되는 상태를 허용하지 않는다.
- 여러 worker는 `FOR UPDATE SKIP LOCKED`로 서로 다른 작업을 선점한다.
- 실행 중인 작업에는 lease와 claim token이 있다. Lease가 만료되면 다른 worker가 재선점할 수 있고, 이전 claim token으로는 완료 또는 실패 상태를 기록할 수 없다.
- 코드에서 등록하는 작업은 `JobSpec`이 kind와 payload type을 결합한다. 생산자는 `NewJob<J>`, 소비자는 `JobHandler<J>`를 사용하므로 서로 다른 payload를 연결하면 compile되지 않는다.
- Queue는 동일 payload의 중복 등록을 제거하지 않는다. Handler는 같은 작업이 반복 실행되어도 최종 상태가 같아야 한다. 도메인은 필요할 때 의미가 같은 fresh queued 요청만 병합할 수 있다.
- `NOTIFY`는 worker를 빠르게 깨우는 신호일 뿐 작업 원장이 아니다. LISTEN 연결에 실패하거나 알림을 놓쳐도 consumer와 safety poll은 DB의 준비된 작업을 계속 확인한다.
- PostgreSQL `NOTIFY`는 모든 listener에 전달되는 broadcast다. 수신 직후 각 worker는 이미 버퍼에 쌓인 알림을 함께 소진하고 최대 50ms의 짧은 wake spread를 적용한다. 실제 중복 선점은 `FOR UPDATE SKIP LOCKED`가 막는다.

Queue가 보장하지 않는 것:

- exactly-once 실행
- DB에 직접 저장된 JSON payload와 현재 payload type의 호환성
- 도메인 결과의 stale 여부
- 서로 다른 작업 사이의 실행 순서
- 사용자별 또는 Space별 동시성 정책

## 코드 경계

Queue 상태 머신과 NoteGate 업무 처리는 crate와 application 조립 경계로 나눈다.

```text
backend/crates/jobs/                 package: notegate-jobs
  model.rs                           job, claim, failure model
  handler.rs                         typed handler contract와 runtime registry
  queue.rs                           enqueue, claim, heartbeat, state transition
  worker.rs                          concurrency, lease, retry, timeout runtime
  reconciler.rs                      lease recovery와 terminal history 정리

backend/crates/api/src/background_jobs/
  mod.rs                             API process의 runtime과 handler 등록
  handlers.rs                        Usage 업무 adapter

backend/crates/db/                   schema, repository, transaction
backend/crates/service/              authorization과 업무 흐름
```

의존 방향은 다음과 같다.

```text
API 또는 domain transaction
  └─ notegate-jobs::JobQueue로 작업 등록

notegate-api background runtime
  ├─ notegate-jobs::Worker로 작업 실행
  ├─ notegate-jobs::QueueReconciler로 queue 복구와 정리
  └─ handler를 통해 db/service 업무 호출
```

- `notegate-jobs`는 application 업무 개념과 payload schema를 알지 않는다.
- API의 `background_jobs` 모듈은 handler를 조립하는 실행 경계이며 업무 규칙을 다시 구현하지 않는다.
- Registry는 저장된 JSON을 `JobSpec::Payload`로 한 번 변환한다. 변환할 수 없는 payload는 handler를 호출하지 않고 permanent failure로 종료한다.
- Handler는 typed payload를 받아 완료, 지연 또는 분류된 실패를 queue runtime에 반환한다.
- 멱등성, stale 판정, 업무 결과 transaction은 handler가 호출하는 db/service 계층이 소유한다.
- Queue schema와 migration은 database schema 소유권에 따라 `notegate-db`가 관리한다.
- API process는 HTTP server, queue consumer, queue reconciler를 함께 실행하고 하나의 `PgPool`을 공유한다.

## 상태 머신

```text
enqueue
  │
  ▼
queued ── claim ──▶ running ── success ──▶ succeeded
  ▲                    │
  │                    ├─ expected contention / defer
  │                    │        ├─ attempts remaining ──▶ queued
  │                    │        └─ attempts exhausted ──▶ dead
  │                    │
  │                    ├─ retryable error / timeout / panic / shutdown
  │                    │        └─ attempts remaining ──▶ queued
  │                    │
  │                    └─ permanent error / attempts exhausted ──▶ dead
  │
  └──── expired lease with attempts remaining
```

`queued`, `running`, `succeeded`, `dead`만 영속 상태다. `ready`, `delayed`, `lease_expired`는 시간 조건을 반영한 관측용 상태다.

## 저장 구조

```text
background_jobs
  job_id
  job_kind
  payload
  history_visibility / history_owner_account_id
  context_kind / context_id / context_label
  status
  available_at
  attempt_count / failure_count / max_attempts
  claim_token / claimed_by / lease_until
  last_error_code / last_error_message
  created_at / updated_at / completed_at

background_job_attempts
  job_id / attempt_number
  claim_token / worker_id
  started_at / finished_at
  outcome
  error_code / error_message
```

작업은 성공하거나 재시도 한도를 소진해도 즉시 삭제하지 않는다. `succeeded`와 `dead` 작업 및 연결된 attempt는 90일 동안 보관한 뒤 짧은 transaction batch로 삭제한다. 한 maintenance pass는 최대 5초 동안 batch를 반복하고 backlog가 남으면 약 1초 뒤 이어서 처리한다.

## 실행 규칙

- Worker는 자신에게 등록된 `job_kind`만 선점한다. 처리할 수 없는 kind 때문에 polling loop가 계속 깨어나면 안 된다.
- 기본 동시 실행 수는 process당 4이고 최대 64다.
- `NOTEGATE_BACKGROUND_JOBS__CONCURRENCY`로 process별 동시 실행 수를 설정한다.
- 공유 database pool은 concurrency보다 최소 2개 커야 한다. LISTEN 연결 하나와 HTTP, heartbeat, reconciliation, metric 조회가 공유할 최소 여유를 남긴다. 운영값은 HTTP 부하까지 포함해 이 최솟값보다 크게 잡는다.
- 기본 lease는 2분이며 worker는 lease의 3분의 1 간격으로 heartbeat한다.
- Handler timeout은 kind별로 정한다.
- 자동 재시도는 5초에서 시작해 최대 15분까지 증가하는 exponential backoff와 ±10% jitter를 사용한다.
- Handler가 명시한 `retry_after`는 더 일찍 실행되지 않도록 +0~20% jitter를 적용한다.
- 한 작업은 기본 8회, 최대 100회의 실행 attempt를 허용한다. 성공하지 못한 claim은 defer, retryable failure, timeout, panic, 취소, lease 만료 여부와 관계없이 같은 실행 한도를 소비한다. 마지막 attempt에서 완료하지 못한 작업과 permanent failure는 `dead`가 되며 자동으로 다시 활성화되지 않는다.
- `failure_count`는 오류로 끝난 실행을 관측하기 위한 값이다. 실행 상한은 `attempt_count`와 `max_attempts`가 결정하므로 정상적인 자원 경합을 나타내는 defer도 무한히 반복되지 않는다.
- Queue의 `dead`는 실행 이력이며 도메인 상태와 동일하지 않다. 도메인은 필요하면 별도의 상태를 관리한다.
- Worker는 다음 delayed 작업 시각 또는 safety poll 중 이른 시점에 깨어난다. Safety poll 기본값은 10분이고 매 주기에 ±10% jitter를 적용한다.
- Worker에 빈 실행 슬롯이 남은 비포화 claim 주기는 최소 25ms 뒤에 다시 확인해, ready row가 일시적으로 잠겼을 때 발생할 수 있는 zero-delay polling loop를 막는다.
- LISTEN 재연결은 10초 기준 ±20% jitter를 적용한다.
- Panic, timeout, graceful shutdown 중 취소는 retryable failure로 기록한다.
- Worker가 비정상 종료되어 attempt를 닫지 못하면 lease recovery가 `lease_expired`로 마감하고 재시도하거나 `dead`로 전환한다.
- Lease recovery와 retention 정리는 consumer loop와 독립적인 `QueueReconciler`가 수행한다.
- 모든 API replica가 reconciler를 시작하지만 각 실행은 PostgreSQL advisory transaction lock을 먼저 얻는다. 같은 database에서는 한 시점에 하나의 recovery 또는 retention pass만 실행된다.
- Lease recovery는 60초 기준 ±10% jitter로 실행하고 시작 시점을 최대 5초 분산한다. Retention 정리는 1시간 기준 ±10% jitter로 실행하고 최초 실행도 한 주기 안에서 분산한다.
- Consumer 또는 reconciler가 shutdown 신호 없이 종료되면 API process도 오류로 종료한다. HTTP만 정상인 채 queue 처리가 멈춘 상태를 허용하지 않는다.

현재 등록된 kind:

```text
space_usage_reconcile  # Space 사용량 전체 재계산
```

## 도메인 책임

Usage handler는 현재 원본을 다시 집계해 `space_usage`를 덮어쓴다. 같은 작업이 반복되어도 결과가 같다. Space별 reconciliation gate를 사용하므로 같은 Space의 mutation과 정확한 재계산이 겹치지 않는다.

## 실행과 스케일 아웃

각 `notegate-api` process가 HTTP server와 queue runtime을 함께 실행한다. 원본 변경과 enqueue는 같은 PostgreSQL database transaction에 기록되고, 모든 replica의 consumer가 같은 원장에서 작업을 분산 선점한다.

```text
API transaction ── insert background_jobs ── COMMIT ── broadcast NOTIFY
                                                         │
                                                   0~50ms spread
                                                         │
                                                         ▼
API replicas ── claim batch ── bounded handlers ── state transition
     │
     └─ QueueReconciler ── advisory lock ── lease recovery / retention
```

KEDA PostgreSQL scaler는 read-only 계정으로 다음 단일 값을 조회할 수 있다.

```sql
SELECT background_job_backlog(NULL);
```

특정 kind만 확장 기준으로 삼으려면 인자에 kind를 전달한다. 값은 지금 실행 가능한 `queued` 작업과 `running` 작업 수다. 아직 `available_at`에 도달하지 않은 재시도와 `dead` 작업은 제외한다.

## 관측

Background job metric은 API listener의 `/metrics`에 함께 노출된다. `NOTEGATE_METRICS_ENABLED=true`일 때만 route와 주기적 queue snapshot 갱신이 활성화된다.

```text
notegate_background_jobs{kind,state}
notegate_background_job_oldest_ready_age_seconds
notegate_background_jobs_in_flight{kind}
notegate_background_job_attempts_total{kind,outcome}
notegate_background_job_transitions_total{transition}
notegate_background_job_duration_seconds{kind}
```

Metric label에는 job ID, Space ID, node ID, payload, error message를 넣지 않는다.

Queue 상태와 oldest-ready age는 PostgreSQL의 운영 대상 작업을 읽은 전역 값이다. 90일간 보관되는 `succeeded` 이력은 snapshot에서 세지 않는다. API replica마다 같은 전역 값이 노출되므로 Prometheus에서 replica를 합산하지 않고 `max`로 집계한다. In-flight, attempt, duration, transition metric은 process-local 값이다.

History에 보여줄 job은 enqueue envelope에 `history_visibility=visible`, `history_owner_account_id`, 선택적 `context_kind/context_id/context_label`을 기록한다. 이 공통 metadata가 있는 job은 종류와 관계없이 해당 account의 History에 나타난다. 기본값은 `hidden`이며 History에 표시할 필요가 없는 운영·유지보수 job은 제외된다. `context_*`는 Space에 한정되지 않는 표시 문맥이고, Worker의 claim·retry·lease 처리에는 이 metadata를 사용하지 않는다.

History는 enqueue 시 저장된 owner/context snapshot만 사용한다. 현재 Space 상태에서 소유자를 역추정하지 않으며 owner snapshot이 없는 행은 hidden으로 유지한다. Terminal 행은 90일 동안 보관한다. `space_usage_reconcile_jobs` insert는 trigger가 `background_jobs`로 복제하지만 일반적인 job 등록은 공통 enqueue API를 사용한다.

## 검증 경계

- `notegate-jobs` unit test는 handler 등록, 잘못 저장된 payload, configuration, retry delay, panic, timeout, shutdown 결과를 검증한다.
- PostgreSQL integration test는 transaction enqueue, 동시 claim, attempt 기록, heartbeat, 다중 reconciler lease recovery, fencing, terminal 전이와 retention을 검증한다.
- Usage test는 반복 실행해도 같은 정확한 counter가 생성되는 업무 멱등성을 검증한다.
- Browser E2E는 queue consumer가 포함된 API process와 dashboard를 실행해 사용자 흐름을 검증한다.
- Queue test는 handler 업무 정확성을 대신하지 않고, handler test도 queue의 전달 보장을 다시 구현하지 않는다.
