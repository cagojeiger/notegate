# Observability

NoteGate exposes process-local Prometheus metrics on each active process control plane when
`NOTEGATE_METRICS_ENABLED=true`. Public/worker processes use their application listener;
`search` mode uses the private search listener. Metrics are disabled by default. When disabled,
`/metrics` is not registered and the HTTP middleware skips metric recording.
Every exported series carries the bounded global label `process_mode`, whose value is the configured
process mode: `all`, `api`, `search`, `worker`, or `reconciler`.

Combined `all`/local `api` mode intentionally exposes one process-local scrape endpoint on the
public listener. Search operation and cache metrics are recorded in the shared process recorder and
appear there; the private listener does not register a duplicate `/metrics`. A standalone `search`
process exposes its scrape endpoint on the private listener instead.

`/metrics` is a control-plane route. It has the control-plane timeout and is excluded
from the data-plane body limit and rate limit. It is not an authenticated user API;
deployments must expose it only to a trusted monitoring network.

## HTTP RED metrics

```text
notegate_http_requests_total
  labels: method, route, status_class

notegate_http_request_duration_seconds
  labels: method, route, status_class

notegate_http_requests_in_flight
  labels: method, route
```

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

MCP tool dispatch and Command API requests share one bounded command-invocation
metric family. The Command API surface is labelled `cli` because it is the
machine JSON surface used by `notegate-cli`.

```text
notegate_command_invocations_total
  labels: surface, tool, outcome

notegate_command_invocation_duration_seconds
  labels: surface, tool, outcome

notegate_command_invocations_in_flight
  labels: surface, tool
```

- `surface` is `mcp` or `cli`.
- `tool` is one of `me`, `read`, `search`, `write`, `manage`,
  `file_download`, `file_upload`, `run_read_sequence`,
  `run_write_sequence`, or `unknown`.
- `outcome` is `success` or `error`.
- MCP records only calls that reach tool dispatch. Authentication failures before
  dispatch remain visible through HTTP RED metrics.
- Duration is recorded in seconds using the HTTP RED bucket layout from 5 ms
  through 30 s.

## Resource utilization metrics

```text
notegate_db_pool_connections
  labels: pool (primary, read), state (in_use, idle)

notegate_db_pool_max_connections
  labels: pool (primary, read)

notegate_search_body_cache_size_bytes
notegate_search_body_cache_capacity_bytes
notegate_search_body_cache_entries
```

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

Background runtime은 active process listener의 `/metrics`에 metric을 함께 제공한다. `NOTEGATE_METRICS_ENABLED=true`일 때만 기록과 노출을 활성화한다.

```text
notegate_background_jobs
  labels: kind, state

notegate_background_job_oldest_ready_age_seconds
  labels: kind

notegate_background_jobs_in_flight
  labels: kind

notegate_background_job_attempts_total
  labels: kind, outcome

notegate_background_job_transitions_total
  labels: kind, transition

notegate_background_job_state_transition_errors_total
  labels: kind, operation

notegate_background_job_queue_errors_total
  labels: operation

notegate_background_job_duration_seconds
  labels: kind
```

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

```text
notegate_metadata_write_flushes_total
  labels: outcome

notegate_metadata_write_flush_duration_seconds
  labels: outcome

notegate_metadata_write_items_total
  labels: kind, disposition
```

- `outcome` is `success`, `error`, or `timeout`.
- `kind` is `api_key`, `browser_session`, or `media_type`.
- `disposition` is `flushed`, `dropped`, or `stranded`. `stranded` means the
  graceful-shutdown retry budget ended with values still in process memory.
- IDs, media types, and error text are not metric labels.

## Cardinality and data policy

Metric labels use only bounded domains declared for each family in this document. Allowed values come from fixed enums such as process mode, method, status class, outcome, state, operation and stage, or from bounded code-registered catalogs such as route templates, tool names, job kinds and reconciler kinds. New metrics define their label domains here before implementation.

Labels do not derive from user, request or content data. IDs, raw paths and query strings, search input and cursors, filenames, content and payloads, and error or exception text belong in structured logs or traces. Request and trace IDs are not metric labels.

## Reconciliation 메트릭

```text
notegate_reconciliation_active
  labels: kind

notegate_reconciliation_runs_total
  labels: kind, outcome

notegate_reconciliation_duration_seconds
  labels: kind, outcome

notegate_reconciliation_last_completed_timestamp_seconds
  labels: kind

notegate_reconciliation_last_success_timestamp_seconds
  labels: kind
```

- `kind`는 `system.purge`, `object_storage.cleanup`,
  `background_jobs.lease_recovery`, `background_jobs.history_retention` 중 하나다.
- `outcome`은 `succeeded`, `failed`, `timed_out`, `panicked`, `cancelled`,
  `lock_held`, `lock_error` 중 하나다.
- 실행 시간은 advisory lock을 획득한 결과에만 기록한다.
- ID, payload, 파일 이름, 문서 본문, 오류 문구는 metric label로 사용하지 않는다.
- `active`는 process-local gauge이며 등록 시 kind별 `0` series를 만든다. Fleet 상태는 `sum by (kind)`으로 집계하며 advisory lock이 정상일 때 값은 `0` 또는 `1`이다.
- `runs_total`은 process-local counter다. Fleet 실행 수는 `sum by (kind)`으로 replica별 `increase`를 합산한다. `max`는 실행 수를 누락하므로 사용하지 않는다.
- `duration_seconds`는 process-local histogram이다. Fleet percentile은 먼저 `sum by (kind, le)`로 replica별 bucket 증가량을 합산한 뒤 `histogram_quantile`을 적용한다. Replica별 percentile의 평균이나 최댓값을 사용하지 않는다.
- `last_completed_timestamp_seconds`는 lock을 획득한 실행이 성공, 실패, timeout, panic 또는 취소로 끝날 때 갱신한다. Fleet의 최신 완료 시각은 `max by (kind)`으로 집계한다.
- `last_success_timestamp_seconds`는 성공한 실행에만 갱신하며 `max by (kind)`으로 집계한다.
- `lock_held`는 다른 replica가 같은 kind를 실행 중임을 뜻하므로 다중 replica 환경에서 정상적으로 발생할 수 있다. 동일 kind의 `active` 합계가 `1`을 초과하면 단일 실행 불변식 위반이다.
- `ContinueAfter`를 반환한 bounded pass도 `succeeded`다. 마지막 완전 수렴 여부는 업무별 backlog 또는 freshness metric으로 판단한다.
- 완료 시각 gauge는 process-local이다. Pod 교체를 포함하는 fleet 조회는 reconciliation interval보다 긴 범위의 `max_over_time`을 사용한다.

## Search metrics

Search metrics are recorded only when `NOTEGATE_METRICS_ENABLED=true`. Disabled
metrics do not start search timers or update counters.

```text
notegate_search_operations_total
  labels: operation, mode, outcome

notegate_search_operation_duration_seconds
  labels: operation, mode, outcome

notegate_search_stage_duration_seconds
  labels: operation, stage

notegate_search_match_reduce_duration_seconds
  labels: operation, mode, line_mode

notegate_search_candidates_total
  labels: operation

notegate_search_results_total
  labels: operation

notegate_search_scanned_bytes_total
  labels: operation

notegate_search_body_load_bytes_total
  labels: operation

notegate_search_cache_lookups_total
  labels: result
```

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
