# Reconciliation

NoteGate의 주기적인 전역 수렴 작업은 `notegate-reconciliation` runtime을 사용한다. Runtime은 실행 제어만 담당하고 실제 업무는 API adapter가 DB 또는 service 계층에 위임한다.

## 계약

- `Reconciler::KIND`와 구현 타입은 compile time에 결합된다.
- 각 kind는 고정 주기와 실행 timeout을 가진다. 한 번의 제한된 실행으로 backlog를 비우지 못한 handler는 성공 결과와 함께 짧은 후속 실행 간격을 요청할 수 있다.
- 모든 `all` 또는 `worker` process가 같은 kind를 등록한다.
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
  └─ object_storage.cleanup
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
```

Object storage cleanup은 전역 singleton reconciliation으로 실행하지만, provider 호출 전의 행 단위 claim과 `retry_after` lease를 유지한다. 이 안전장치는 롤링 배포 중 구버전 worker와의 경합, provider 호출 후 DB 갱신 전 종료, 재시도 backoff를 처리한다.

행 단위 claim으로 replica가 작업을 나눠 처리하는 queue consumer는 이 runtime 대상이 아니다. Process별 상태를 관리하는 metrics upkeep과 metadata write-behind도 전역 singleton으로 만들지 않는다.

## 관측

```text
notegate_reconciliation_active{kind}
notegate_reconciliation_runs_total{kind,outcome}
notegate_reconciliation_duration_seconds{kind,outcome}
notegate_reconciliation_last_success_timestamp_seconds{kind}
```

Metric label에는 node ID, Space ID, payload 또는 오류 본문을 넣지 않는다. 구조화 로그는 진단에 필요한 제한된 오류 정보를 기록할 수 있지만 문서 본문이나 job payload는 기록하지 않는다.
