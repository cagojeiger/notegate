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
NoteGate therefore does not publish a synthetic saturation value. Acquire wait
duration can be added later if connection acquisition is centralized behind an
instrumented boundary.

## Cardinality and data policy

Metric labels must be bounded and must not contain:

- request paths or query strings;
- search queries, include/exclude patterns, or cursors;
- account, user, agent, Space, node, upload, or request identifiers;
- filenames, content, error messages, or exception text.

New metrics must define their label domains in this document before implementation.
Unbounded diagnostic values belong in structured logs or traces, not Prometheus labels.

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
