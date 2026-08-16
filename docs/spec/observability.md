# Observability

NoteGate exposes process-local Prometheus metrics on the application listener when
`NOTEGATE_METRICS_ENABLED=true`. Metrics are disabled by default. When disabled,
`/metrics` is not registered and the HTTP middleware skips metric recording.

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

## Resource utilization metrics

```text
notegate_db_pool_connections
  labels: state (in_use, idle)

notegate_db_pool_max_connections

notegate_search_body_cache_size_bytes
notegate_search_body_cache_capacity_bytes
notegate_search_body_cache_entries
```

These gauges are read only when `/metrics` is scraped; application requests do not
update them. DB pool utilization can be derived from `in_use / max_connections`.
Cache utilization can be derived from `size_bytes / capacity_bytes`. Cache size and
entry count are process-local Moka estimates and may briefly lag concurrent cache
maintenance. A disabled cache reports zero capacity, size, and entries.

SQLx does not expose the pool's waiter count through the current shared-pool API.
NoteGate therefore does not publish a synthetic saturation value. Instead,
`notegate_db_pool_acquire_duration_seconds` measures observed connection acquisition
waits and `notegate_db_pool_acquire_timeouts_total` counts acquisition timeouts.

## Background job metrics

API process 안의 background runtime은 기존 application listener의 `/metrics`에 metric을 함께 제공한다. `NOTEGATE_METRICS_ENABLED=true`일 때만 기록과 노출을 활성화한다.

```text
notegate_background_jobs
  labels: kind, state

notegate_background_job_oldest_ready_age_seconds

notegate_background_jobs_in_flight
  labels: kind

notegate_background_job_attempts_total
  labels: kind, outcome

notegate_background_job_transitions_total
  labels: transition

notegate_background_job_duration_seconds
  labels: kind
```

- `kind`는 API background runtime에 등록된 bounded job kind다.
- `state`는 `ready`, `delayed`, `running`, `lease_expired`, `dead` 중 하나다. 90일간 보관되는 `succeeded` 이력은 scrape 비용이 누적되지 않도록 gauge에서 제외한다.
- `outcome`은 `succeeded`, `retrying`, `dead`, `claim_lost` 중 하나다.
- `transition`은 현재 `lease_retry`, `lease_dead` 중 하나다.
- Queue gauge와 oldest-ready age는 15초마다 PostgreSQL 운영 원장을 읽어 갱신한다. 조회에 실패하면 오류를 기록하고 마지막 정상 값을 유지한다. API replica마다 같은 전역 값이 노출되므로 fleet 조회에는 `max` 집계를 사용하며 `sum`으로 합산하지 않는다.
- In-flight, attempt, transition, duration metric은 해당 API process에서 발생한 값이다.
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

Metric labels must be bounded and must not contain:

- request paths or query strings;
- search queries, include/exclude patterns, or cursors;
- account, user, agent, Space, node, upload, or request identifiers;
- background job ID, claim token, payload, or worker ID;
- filenames, content, error messages, or exception text.

New metrics must define their label domains in this document before implementation.
Unbounded diagnostic values belong in structured logs or traces, not Prometheus labels.

## Reconciliation 메트릭

```text
notegate_reconciliation_active
  labels: kind

notegate_reconciliation_runs_total
  labels: kind, outcome

notegate_reconciliation_duration_seconds
  labels: kind, outcome

notegate_reconciliation_last_success_timestamp_seconds
  labels: kind
```

- `kind`는 `system.purge`, `object_storage.cleanup`,
  `background_jobs.lease_recovery`, `background_jobs.history_retention` 중 하나다.
- `outcome`은 `succeeded`, `failed`, `timed_out`, `panicked`, `cancelled`,
  `lock_held`, `lock_error` 중 하나다.
- 실행 시간은 advisory lock을 획득한 결과에만 기록한다.
- ID, payload, 파일 이름, 문서 본문, 오류 문구는 metric label로 사용하지 않는다.
- `active`는 process-local gauge다. Fleet 상태는 `sum by (kind)`으로 집계하며 advisory lock이 정상일 때 값은 `0` 또는 `1`이다.
- `runs_total`은 `sum by (kind, outcome)`으로 집계한다.
- `duration_seconds` histogram은 `kind`와 `outcome` 기준으로 bucket, count, sum을 합산한다.
- `last_success_timestamp_seconds`는 `max by (kind)`으로 집계한다.

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
