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
  ├─ text_revisions.retention
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
system.purge (1분 주기, 전체 실행 timeout 2분)
  ├─ resources
  │    ├─ Space별: 버전/참조 정리 + 객체 삭제 요청 + leaf node 삭제 → COMMIT
  │    └─ 이미 제거된 owner의 orphan projection 정리 → 별도 COMMIT
  ├─ identities → 계정 익명화 + 만료된 API key/browser session 정리 → COMMIT
  └─ history    → terminal object 원장 + 보존 기간 만료 event/invocation 정리 → COMMIT
```

리소스 정리는 마지막 시도 시각이 오래된 Space부터 최대 10개를 순회하며, Space마다 짧은 트랜잭션을 사용한다. `purge_last_attempt_at`은 선택 시 별도로 커밋하는 순회 metadata다. 잠긴 Space나 실패한 Space도 다음 순회에서 다른 Space보다 우선하지 않으며, 완료 증거로 사용하지 않는다. 변경 경로와 같은 Space gate를 사용하고 잠긴 Space는 건너뛴다.

삭제 대상은 물리적인 자식이 없는 leaf부터 선택한다. 만료/영구 삭제된 조상의 subtree에는 독립적으로 삭제했던 자식도 포함된다. 본문 버전, 양방향 링크 참조, upload의 부모 참조를 batch로 먼저 정리하며, 남은 참조가 있으면 node 삭제를 미룬다. Space는 모든 non-root node와 객체 원장의 Space 참조, Agent connection을 정리한 뒤 빈 root와 함께 제거한다. 큰 subtree나 문서 버전을 한꺼번에 cascade하지 않는다.

객체 삭제 요청과 해당 semantic row 제거는 같은 Space batch에서 함께 커밋하거나 롤백한다. 실패한 batch는 재시도하며, 이미 커밋한 다른 Space의 진척은 유지한다. 일반적인 오류 이후에도 뒤의 Space와 identities/history 묶음을 시도하고, 하나라도 실패하면 첫 오류를 반환한다. 정상 실행에서 resource backlog가 남으면 runtime lock을 해제하고 1초 후 이어서 실행한다. 실패는 다음 1분 주기에 재시도한다. 보존 기간은 변경하지 않는다.

리소스 순회는 30초 이후 새 Space batch를 시작하지 않으며, 한 batch의 timeout은 15초다. 개별 SQL statement는 10초, lock 대기는 2초로 제한한다. 이후 orphan projection과 backlog 조회를 수행한다. 전체 실행 timeout, 종료, panic으로 중단되면 뒤의 묶음은 실행되지 않을 수 있지만 이전 커밋은 유지된다. 커밋 응답이 유실되어도 다음 실행은 남은 DB 상태를 읽어 수렴한다.

처리 상한은 `performance-limits.md`를 따른다. 상한은 row 변경량을 제한하며 조회 비용, WAL, vacuum이나 실제 디스크 공간 반환 시간을 보장하지 않는다. S3 물리 삭제는 기존 `object_storage.cleanup`이 별도로 재시도한다.

커밋한 Space batch는 `purge.space_completed`, 오류는 `purge.space_failed`로 기록한다. resource 묶음 성공 시 `purge.group_completed`에 처리량, 경과 시간, 남은 삭제 후보 수와 가장 오래된 eligible 시각을 기록한다. 후보 수는 due Space/node의 수이며 subtree 전체 row 수나 S3 삭제 완료 수가 아니다. 다른 묶음의 오류는 `purge.group_failed`로 기록하고, 모든 묶음이 성공해야 `purge.completed`를 기록한다.

## 관측

Reconciliation runtime은 kind별 활성 상태, 실행 결과, 실행 시간과 최근 완료·성공 시각을 노출한다. Metric 이름, label domain과 fleet 집계 방법은 [Observability의 Reconciliation 메트릭](observability.md#reconciliation-메트릭)을 따른다.

`lock_held`는 다른 replica가 동일 kind를 실행 중인 정상적인 조정 결과다. 같은 kind의 `active` 합계가 `1`을 초과하면 단일 실행 불변식 위반이다. `ContinueAfter`를 반환한 bounded pass도 성공한 실행이며, 완전 수렴 여부는 업무별 backlog 또는 freshness metric으로 판단한다.

## Text revision retention

`text_revisions.retention`은 기존 runtime에서 10분마다 실행하며, 한 Space의 만료된 본문 버전을 최대 100개씩 정리한다. 삭제가 있었다면 lock을 해제한 뒤 1초 후 후속 실행한다. 보존 경계·별도 용량·실패 의미는 [Text revisions](text-revisions.md)를 따른다. 별도 worker를 추가하지 않는다.
