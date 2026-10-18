# Observability

Metrics are process-local, enabled only by `NOTEGATE_METRICS_ENABLED=true` (default: disabled). When disabled, `/metrics` is not registered and the HTTP middleware skips recording. Every series has `process_mode=all|api|search|worker|reconciler`.

| Process | Scrape endpoint |
|---|---|
| Local Search를 포함한 `all` / `api` | Public listener; shared Search/cache recorder, no duplicate private `/metrics` |
| Standalone `search` | Private Search listener |
| Other active roles | Application control-plane listener |

`/metrics` uses the control-plane timeout, outside data-plane body/rate limits. It is unauthenticated and must be exposed only to a trusted monitoring network.

## HTTP RED metrics

| Metric | Labels |
|---|---|
| `notegate_http_requests_total` | `method, route, status_class` |
| `notegate_http_request_duration_seconds` | `method, route, status_class` |
| `notegate_http_requests_in_flight` | `method, route` |

- `route` is the Axum route template, for example
  `/api/v1/spaces/{space_id}/nodes/{node_id}`.
- Requests without a matched template use `route="unmatched"`.
- `method` is one of `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`,
  `CONNECT`, `TRACE`, or `OTHER`.
- `status_class` is one of `1xx`, `2xx`, `3xx`, `4xx`, `5xx`, or `other`.
- Duration is recorded in seconds using fixed buckets from 5 ms through 30 s.
- `/metrics`, `/health`, `/ready`, and frontend static/SPA fallback requests are
  excluded. REST, MCP, authentication, metadata, and unmatched API requests are
  included.
- Histogram upkeep runs every 5 seconds and follows the server's graceful-shutdown
  lifecycle, so a monitoring outage does not leave samples accumulating until the
  next scrape.

## Command invocation metrics

MCP tool dispatch, Command API, and public API v2 requests share one bounded command-invocation
metric family. Shared completion handling records execution, awaited history persistence and completion; adapters retain protocol-specific validation, result classification and redaction. The Command API surface is labelled `cli` because it is the
machine JSON surface used by `notegate-cli`.

| Metric | Labels |
|---|---|
| `notegate_command_invocations_total` | `surface, tool, outcome` |
| `notegate_command_invocation_duration_seconds` | `surface, tool, outcome` |
| `notegate_command_history_records_total` | `surface, tool, outcome` |
| `notegate_command_history_duration_seconds` | `surface, tool, outcome` |
| `notegate_command_completion_duration_seconds` | `surface, tool, outcome` |
| `notegate_command_invocations_in_flight` | `surface, tool` |

- `surface` is `mcp`, `cli`, or `api` (public v2).
- `tool` is one of `me`, `read`, `search`, `write`, `manage`,
  `file_download`, `file_upload`, `run_read_sequence`,
  `run_write_sequence`, or `unknown`; public v2 uses the fixed label `http`.
- `outcome` is `success` or `error`. History metrics describe persistence success;
  invocation and completion metrics describe the command result. A history failure
  does not change the command result.
- MCP records only calls that reach tool dispatch. Authentication failures before
  dispatch remain visible through HTTP RED metrics.
- Invocation duration and persisted history `duration_ms` measure command execution,
  excluding history capture. Completion duration includes awaited history capture,
  ending immediately before the adapter returns. It excludes upstream authentication,
  network delivery; it is not end-to-end client latency. MCP/CLI also exclude response
  serialization. Public v2 measures through handler response construction, before
  HTTP summary persistence; it does not wait for response-body delivery.
- History duration covers post-execution snapshot preparation, redaction, encryption, connection
  acquisition and insertion. MCP calls without a caller do not attempt history capture
  and emit no history sample.
- In-flight includes history capture. Cancellation releases the gauge but emits no
  completion sample; an already finished command execution remains counted.
- Invocation/completion durations use HTTP RED buckets (5 ms–30 s); history uses
  search-duration buckets (1 ms–30 s). Metrics do not change the awaited, best-effort
  history policy or add background tasks.
- Compare history and completion averages using their `_sum / _count` rates with
  the same `surface, tool` aggregation. Compute percentiles from aggregated buckets;
  subtracting independently calculated p95 values does not measure history overhead.
  CI's ignored `measure_command_completion` tests report isolated adapter/history
  timings in a debug build; they are diagnostic, not a production latency budget.

### CI HTTP concurrency smoke

Browser E2E의 [api-load-smoke.mjs](../../deploy/ci/api-load-smoke.mjs)는 격리된 app/PostgreSQL에서 동시 읽기·쓰기와 동일 hash 경쟁을 검증한다. 문서 최종 상태, 하나의 winning write, 완료 요청별 invocation, committed write만의 Changes 연결을 확인한다. 요청 오류나 내용·이력 불일치는 CI를 실패시킨다. Workload와 latency budget은 script가 소유한다.

Job summary와 `api-load-diagnostics` artifact에서 latency, paced throughput, 실제 in-flight, handler/history/completion 및 pool 관측을 확인한다. SQL·credential·본문은 기록하지 않는다. HTTP 시간과 handler 시간의 차이는 인증·기록·전송·client scheduling을 함께 포함하며 history 비용만을 뜻하지 않는다. Pool 관측에도 background query가 포함되고 짧은 DB wait는 sampling에서 누락될 수 있다.

동일 runner의 debug app/DB와 하나의 account를 사용하는 closed-loop 부하다. 최대 RPS, 독립 사용자 수용량 또는 운영 성능의 증거로 사용하지 않는다.

### Authenticated browser baseline

`Browser Performance`는 격리된 PostgreSQL/MinIO와 release API가 제공하는 production frontend를 사용한다. Seed한 browser session으로 OIDC 로그인은 우회하며 API 응답을 mock하지 않는다. 여정·fixture·반복 횟수는 [workbench.spec.ts](../../frontend/web/performance/workbench.spec.ts)가 소유한다.

Job summary와 `browser-performance` artifact는 ready time, API 요청, 전송량과 long task를 기록한다. Timing에는 Playwright와 animation-frame 대기가 포함되므로 INP나 정확한 paint 시각이 아니다. Route는 query/ID/header/body를 제외해 기록하고 credential을 담는 raw trace는 수집하지 않는다. 예상하지 않은 HTTP/browser 오류와 잘못 저장된 내용은 실패시킨다.

이 baseline의 시간은 진단값이다. 적은 반복으로 p95/p99나 운영 수용량을 추정하지 않으며, OAuth 전체 로그인·운영 네트워크·다중 사용자 경합·background 포화는 별도 검증이 필요하다. 기능 E2E, HTTP concurrency smoke와 Lighthouse는 독립 검사다.

## Resource utilization metrics

| Metric | Labels |
|---|---|
| `notegate_db_pool_connections` | `pool (primary, read), state (in_use, idle)` |
| `notegate_db_pool_max_connections` | `pool (primary, read)` |
| `notegate_search_body_cache_size_bytes` | — |
| `notegate_search_body_cache_capacity_bytes` | — |
| `notegate_search_body_cache_entries` | — |

DB pool gauges are read when `/metrics` is scraped. `read` is emitted only when the process owns a
separate read pool; an aliased read handle is reported once as `primary`. Search cache gauges are initialized by the
owning SearchRuntime, refreshed after search execution, and refreshed again on its `/metrics`
scrape. DB pool utilization can be derived from `in_use / max_connections`.
Cache utilization can be derived from `size_bytes / capacity_bytes`. Cache size and
entry count are process-local Moka estimates and may briefly lag concurrent cache
maintenance. A disabled cache reports zero capacity, size, and entries.

SQLx does not expose the pool's waiter count through the current shared-pool API.
NoteGate therefore does not publish a synthetic saturation value. Instead,
`notegate_db_pool_acquire_duration_seconds` measures observed connection acquisition
waits and `notegate_db_pool_acquire_timeouts_total` counts acquisition timeouts.

## Background job metrics

| Metric | Labels |
|---|---|
| `notegate_background_jobs` | `kind, state` |
| `notegate_background_job_oldest_ready_age_seconds` | `kind` |
| `notegate_background_jobs_in_flight` | `kind` |
| `notegate_background_job_attempts_total` | `kind, outcome` |
| `notegate_background_job_transitions_total` | `kind, transition` |
| `notegate_background_job_state_transition_errors_total` | `kind, operation` |
| `notegate_background_job_queue_errors_total` | `operation` |
| `notegate_background_job_duration_seconds` | `kind` |

- `kind`는 API background runtime에 등록된 bounded job kind다. Lease 복구 중 발견한 미등록 kind는 `unregistered`로 합친다.
- `state`는 `ready`, `delayed`, `running`, `lease_expired`, `dead` 중 하나다. 90일간 보관되는 `succeeded` 이력은 scrape 비용이 누적되지 않도록 gauge에서 제외한다.
- `outcome`은 `succeeded`, `retrying`, `deferred`, `dead`, `claim_lost` 중 하나다.
- `transition`은 현재 `lease_retry`, `lease_dead` 중 하나다.
- `operation`은 `succeed`, `fail`, `defer` 중 하나다.
- Queue error의 `operation`은 `listen`, `wake_query`, `claim`, `heartbeat` 중 하나다.
- Queue gauge와 oldest-ready age는 15초마다 PostgreSQL 운영 원장을 읽어 갱신한다. 조회에 실패하면 오류를 기록하고 마지막 정상 값을 유지한다. API replica마다 같은 전역 값이 노출되므로 fleet 조회에는 `max` 집계를 사용하며 `sum`으로 합산하지 않는다.
- In-flight, attempt, transition, state-transition error, queue error, duration metric은 해당 API process에서 발생한 값이다.
- Duration은 handler 실행과 최종 queue state 저장을 포함한 attempt wall time이다. Fleet percentile은 histogram bucket을 `kind`별로 합산한 뒤 계산한다.
- Queue의 실행 및 retention 계약은 `background-jobs.md`를 따른다.

## Metadata write-behind metrics

| Metric | Labels |
|---|---|
| `notegate_metadata_write_flushes_total` | `outcome` |
| `notegate_metadata_write_flush_duration_seconds` | `outcome` |
| `notegate_metadata_write_items_total` | `kind, disposition` |

- `outcome` is `success`, `error`, or `timeout`.
- `kind` is `api_key`, `browser_session`, or `media_type`.
- `disposition` is `flushed`, `dropped`, or `stranded`. `stranded` means the
  graceful-shutdown retry budget ended with values still in process memory.
- IDs, media types, and error text are not metric labels.

## Cardinality and data policy

Metric labels use only bounded domains declared for each family in this document. Allowed values come from fixed enums such as process mode, method, status class, outcome, state, operation and stage, or from bounded code-registered catalogs such as route templates, tool names, job kinds and reconciler kinds. New metrics define their label domains here before implementation.

Labels must not derive from user, request, or content data. They must not contain IDs, raw paths/query strings, search inputs/cursors, filenames, content/payloads, or error/exception text. Request and trace IDs are not labels. Logs/traces follow the [security data policy](security.md#기본-원칙).

## Reconciliation 메트릭

| Metric | Labels |
|---|---|
| `notegate_reconciliation_active` | `kind` |
| `notegate_reconciliation_runs_total` | `kind, outcome` |
| `notegate_reconciliation_duration_seconds` | `kind, outcome` |
| `notegate_reconciliation_last_completed_timestamp_seconds` | `kind` |
| `notegate_reconciliation_last_success_timestamp_seconds` | `kind` |

- `kind`는 `system.purge`, `object_storage.cleanup`,
  `background_jobs.lease_recovery`, `background_jobs.history_retention` 중 하나다.
- `outcome`은 `succeeded`, `failed`, `timed_out`, `panicked`, `cancelled`,
  `lock_held`, `lock_error` 중 하나다.
- 실행 시간은 advisory lock을 획득한 결과에만 기록한다.
- `active`는 process-local gauge이며 등록 시 kind별 `0` series를 만든다. Fleet 상태는 `sum by (kind)`으로 집계하며 advisory lock이 정상일 때 값은 `0` 또는 `1`이다.
- `runs_total`은 process-local counter다. Fleet 실행 수는 `sum by (kind)`으로 replica별 `increase`를 합산한다. `max`는 실행 수를 누락하므로 사용하지 않는다.
- `duration_seconds`는 process-local histogram이다. Fleet percentile은 먼저 `sum by (kind, le)`로 replica별 bucket 증가량을 합산한 뒤 `histogram_quantile`을 적용한다. Replica별 percentile의 평균이나 최댓값을 사용하지 않는다.
- `last_completed_timestamp_seconds`는 lock을 획득한 실행이 성공, 실패, timeout, panic 또는 취소로 끝날 때 갱신한다. Fleet의 최신 완료 시각은 `max by (kind)`으로 집계한다.
- `last_success_timestamp_seconds`는 성공한 실행에만 갱신하며 `max by (kind)`으로 집계한다.
- `lock_held`는 다른 replica가 같은 kind를 실행 중임을 뜻하므로 다중 replica 환경에서 정상적으로 발생할 수 있다. 동일 kind의 `active` 합계가 `1`을 초과하면 단일 실행 불변식 위반이다.
- `ContinueAfter`를 반환한 bounded pass도 `succeeded`다. 마지막 완전 수렴 여부는 업무별 backlog 또는 freshness metric으로 판단한다.
- 완료 시각 gauge는 process-local이다. Pod 교체를 포함하는 fleet 조회는 reconciliation interval보다 긴 범위의 `max_over_time`을 사용한다.

## Search metrics

Disabled metrics do not start search timers or update counters.

| Metric | Labels |
|---|---|
| `notegate_search_operations_total` | `operation, mode, outcome` |
| `notegate_search_operation_duration_seconds` | `operation, mode, outcome` |
| `notegate_search_stage_duration_seconds` | `operation, stage` |
| `notegate_search_match_reduce_duration_seconds` | `operation, mode, line_mode` |
| `notegate_search_candidates_total` | `operation` |
| `notegate_search_results_total` | `operation` |
| `notegate_search_scanned_bytes_total` | `operation` |
| `notegate_search_body_load_bytes_total` | `operation` |
| `notegate_search_cache_lookups_total` | `result` |

- `operation` is `find` or `grep`.
- `mode` is `contains`, `glob`, `literal`, or `regex`; only modes valid for the
  selected operation are emitted.
- `line_mode` is `none`, `first`, or `all` for `grep`, and `not_applicable`
  for `find`.
- `outcome` is `success`, `invalid`, `not_found`, `forbidden`, `conflict`, or
  `internal`.
- `stage` is `authorize`, `prepare`, `resolve_scope`, `candidate_query`,
  `cache_lookup`, `body_load`, `match_reduce`, or `hydrate`. Cache and body-load
  stages are grep-only.
- `result` is `hit`, `miss`, or `coalesced`. A coalesced lookup initially missed
  and then observed a body loaded by another request after acquiring the
  single-flight lock.
- Candidate counters measure rows returned by the candidate query. Result
  counters measure returned nodes. Scanned bytes measure plaintext bytes passed
  to grep matching. Body-load bytes measure plaintext bytes returned by the
  database body-load/decryption boundary.
- Match/reduce duration is recorded once per request from the same elapsed time
  as the `match_reduce` stage, so it does not add a second timer. It includes
  path filtering and result reduction around matcher execution.
- Operation, stage, and match/reduce durations use fixed histogram buckets. No
  search query, path, cursor, identifier, filename, content, or error text is
  recorded.

### Text revision cleanup

Reconciliation kind `text_revisions.retention` uses the standard active/result/duration/last-success metrics. `text_revisions.cleaned` logs each pass's deleted row count; a successful pass with follow-up work is not proof that the backlog is empty. Per-Space retained encrypted-body bytes are available in `text_revision_usage`. See [Text revisions](text-revisions.md).
