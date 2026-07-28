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

## Cardinality and data policy

Metric labels must be bounded and must not contain:

- request paths or query strings;
- search queries, include/exclude patterns, or cursors;
- account, user, agent, Space, node, upload, or request identifiers;
- filenames, content, error messages, or exception text.

New metrics must define their label domains in this document before implementation.
Unbounded diagnostic values belong in structured logs or traces, not Prometheus labels.

## Planned search metrics

Search metrics are intentionally separate from the HTTP foundation. A later change may
add operation and stage-level measurements for `find` and `grep`, using bounded labels
such as `operation`, `stage`, `outcome`, and `cache_result`. It must not expose query or
node identity and must reuse the existing search pipeline boundaries.
