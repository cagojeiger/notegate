# Reconciliation

NoteGate의 주기적인 전역 수렴 작업은 `notegate-reconciliation` runtime을 사용한다. Runtime은 실행 제어만 담당하고 실제 업무는 API adapter가 DB 또는 service 계층에 위임한다.

## 계약

- `Reconciler::KIND`와 구현 타입은 compile time에 결합된다.
- 각 kind는 고정 주기와 실행 timeout을 가진다. 한 번의 제한된 실행으로 backlog를 비우지 못한 handler는 성공 결과와 함께 짧은 후속 실행 간격을 요청할 수 있다. Runtime은 요청값을 고정 주기 이하로 제한하므로 후속 실행이 원래 schedule을 늦추지 않는다.
- 모든 `all` 또는 `reconciler` process가 같은 kind를 등록한다.
- PostgreSQL session advisory lock으로 같은 database에서 동일 kind가 동시에 하나만 실행된다.
- Session advisory lock은 직접 PostgreSQL 연결 또는 PgBouncer session pooling에서만 사용할 수 있다. Transaction pooling은 lock 획득과 해제가 서로 다른 server session에서 실행될 수 있으므로 지원하지 않는다.
- Advisory lock은 handler용 공유 pool과 분리된 session을 사용하므로 동시에 실행되는 kind 수만큼 추가 database 연결을 사용할 수 있다.
- 후속 실행 요청도 lock을 먼저 해제한 뒤 다시 선점한다. 다른 process의 동일 kind 실행을 막은 채 대기하지 않는다.
- 실패, timeout 또는 panic은 다음 고정 주기의 실행을 막지 않으며 짧은 후속 실행을 자동 요청하지 않는다.
- 구현은 현재 원본을 다시 읽고 같은 작업을 반복해도 같은 상태로 수렴해야 한다.
- Runtime은 exactly-once 실행과 업무 transaction을 보장하지 않는다.

```text
ReconciliationRegistry
  ├─ system.purge
  ├─ background_jobs.lease_recovery
  ├─ background_jobs.history_retention
  ├─ object_storage.cleanup
  └─ link_graph.change_collector
         │
         ▼
scheduled lane ── advisory lock ── application adapter ── DB/service operation
       ▲                                                │
       └──────── optional bounded continuation ─────────┘
```

## 코드 경계

```text
backend/crates/reconciliation/
  lib.rs       typed contract와 schedule
  registry.rs  등록, kind 검증, lock namespace
  runtime.rs   주기, 전역 선점, timeout, shutdown, metric

backend/crates/api/src/reconciliations/
  mod.rs               application 조립
  purge.rs             hard purge adapter
  background_jobs.rs   lease recovery와 history retention adapter
  object_storage.rs    S3-compatible object cleanup adapter
  link_graph.rs        변경 수집과 link projection dispatch adapter
```

Object storage cleanup은 전역 singleton reconciliation으로 실행하지만, provider 호출 전의 행 단위 claim과 `retry_after` lease를 유지한다. 이 안전장치는 롤링 배포 중 worker 간 경합, provider 호출 후 DB 갱신 전 종료, 재시도 backoff를 처리한다. 한 번에 100개를 모두 처리하면 lock을 해제하고 1초 후 다시 선점해 남은 backlog를 이어서 처리한다.

행 단위 claim으로 replica가 작업을 나눠 처리하는 queue consumer는 이 runtime 대상이 아니다. Process별 상태를 관리하는 metrics upkeep과 metadata write-behind도 전역 singleton으로 만들지 않는다.

## System purge의 트랜잭션 경계

`system.purge`는 하나의 kind, 일정, advisory lock을 유지하며 다음 세 묶음을 순차 실행한다. 별도 worker나 병렬 reconciliation으로 분리하지 않는다.

```text
system.purge (1시간 주기, 전체 실행 timeout 1시간)
  ├─ resources  → 객체 삭제 요청 기록 + 리소스 DB 삭제 + orphan projection 정리 → COMMIT
  ├─ identities → 계정 익명화 + 만료된 API key/browser session 정리 → COMMIT
  └─ history    → terminal object 원장 + 보존 기간 만료 event/invocation 정리 → COMMIT
```

각 묶음은 자체 트랜잭션을 사용한다. 특히 객체 삭제 요청 기록과 리소스 DB 삭제는 함께 커밋하거나 함께 롤백한다. 일반적인 DB 오류가 발생하면 해당 묶음만 롤백하고 뒤의 묶음도 실행한다. 앞서 커밋한 묶음은 유지하며, 모든 묶음을 시도한 뒤 하나라도 실패했으면 첫 오류를 반환해 runtime이 실행 실패로 기록한다. 다음 고정 주기에 원본 상태를 다시 읽어 재시도한다.

트랜잭션 분리는 실행 시간이나 프로세스 장애의 격리를 뜻하지 않는다. 앞의 묶음이 느리면 뒤의 묶음도 기다린다. 전체 timeout, 종료 또는 panic으로 실행이 중단되면 뒤의 묶음은 실행되지 않을 수 있고, 이미 커밋한 결과는 유지된다. 커밋 응답 중 연결이 끊기면 결과가 불확실할 수 있으므로 재실행은 현재 DB 상태를 기준으로 수렴한다.

기존 보존 기간과 묶음별 SQL의 batch 제한은 유지한다. 한 번의 실행에서 남은 backlog는 다음 고정 주기에 처리하며, `system.purge`는 짧은 후속 실행을 요청하지 않는다. 화면에서 제외하는 논리 삭제와 S3 객체를 실제 삭제하는 `object_storage.cleanup`의 실행 방식도 바뀌지 않는다.

각 묶음의 커밋 후 `purge.group_completed`에 `group`과 처리 건수를 기록하고, 오류는 `purge.group_failed`에 해당 `group`과 함께 기록한다. 모든 묶음이 성공한 경우에만 기존 `purge.completed`가 기록된다. 부분 성공을 전체 성공으로 보고하지 않는다.

## 관측

Reconciliation runtime은 kind별 활성 상태, 실행 결과, 실행 시간과 최근 완료·성공 시각을 노출한다. Metric 이름, label domain과 fleet 집계 방법은 [Observability의 Reconciliation 메트릭](observability.md#reconciliation-메트릭)을 따른다.

`lock_held`는 다른 replica가 동일 kind를 실행 중인 정상적인 조정 결과다. 같은 kind의 `active` 합계가 `1`을 초과하면 단일 실행 불변식 위반이다. `ContinueAfter`를 반환한 bounded pass도 성공한 실행이며, 완전 수렴 여부는 업무별 backlog 또는 freshness metric으로 판단한다.
