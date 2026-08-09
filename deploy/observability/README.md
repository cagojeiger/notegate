# NoteGate observability

This directory is the source of truth for NoteGate's Prometheus scrape
configuration and provisioned Grafana dashboards.

## Layout

```text
deploy/observability/
├── prometheus/
│   └── prometheus.yml
└── grafana/
    ├── dashboards/
    │   ├── notegate-service-overview.json
    │   ├── notegate-search-detail.json
    │   └── notegate-internals-detail.json
    └── provisioning/
        ├── dashboards/notegate.yml
        └── datasources/prometheus.yml
```

The dashboard JSON files are portable Grafana assets. The provisioning files
are local-Docker configuration:

- `notegate.yml` loads the JSON files into the local **NoteGate** folder.
- `prometheus.yml` registers the local Prometheus datasource with UID
  `prometheus`.
- `docker-compose.yml` mounts both provisioning files and the dashboard
  directory into Grafana.

## Dashboard roles

| Dashboard | Purpose |
| --- | --- |
| Service Overview | Service health, HTTP RED, runtime USE, cache capacity, and process fleet health |
| Search Detail | `find`/`grep` RED, pipeline-stage cost, scanned workload, and grep-body cache efficiency |
| Internals Detail | MCP tool operations, database-pool acquisition, and server-managed text decryption |

All dashboards assume:

- a Prometheus datasource with UID `prometheus`;
- a scrape target with `job="notegate"`;
- bounded operational labels only—never queries, paths, account/Space/node
  identifiers, filenames, or content.

## Run locally

```sh
cp .env.example .env
make up
make curl-metrics
```

Local endpoints:

- NoteGate: `http://localhost:9191`
- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3000`
- Grafana login: `admin` / `notegate-local` by default

The application default for `NOTEGATE_METRICS_ENABLED` is `false`. Docker
Compose enables it for every web process through `COMPOSE_NOTEGATE_METRICS_ENABLED`,
which defaults to `true`. HTTP and background-job metrics share the same `/metrics`
endpoint.

## Kubernetes delivery

Kubernetes deployment assets live in the separate
`project-jelly/multi-cluster-gitops` repository. Keep the JSON files here as
the source assets and package one dashboard per labeled ConfigMap under the
Grafana hub cluster:

```text
platform/kube-prometheus-stack/manifests/oci-prod-chuncheon/
├── dashboard-notegate-service-overview.yaml
├── dashboard-notegate-search-detail.yaml
└── dashboard-notegate-internals-detail.yaml
```

Each ConfigMap must use the sidecar label:

```yaml
metadata:
  namespace: monitoring
  labels:
    grafana_dashboard: "1"
```

The NoteGate workload runs in Osaka, so metrics collection belongs with that
deployment:

```text
apps/notegate/helm/values-oci-prod-osaka.yaml
apps/notegate/manifests/oci-prod-osaka/servicemonitor.yaml
```

The runtime values must set `NOTEGATE_METRICS_ENABLED=true`. The
`ServiceMonitor` must scrape the `http` service port at `/metrics`, carry the
`release: kube-prometheus-stack` label, and relabel the Prometheus `job` label
to `notegate` because every dashboard query uses that value.

The JSON is compatible with the GitOps Grafana 13.1.x stack and its
`prometheus` datasource UID. Two local-only details need adaptation when
packaging it:

- replace or remove the `http://localhost:9090` Prometheus UI link;
- the local **NoteGate** folder comes from file provisioning and is not
  embedded in the JSON, so configure a Kubernetes-side folder explicitly if
  folder parity is required.

If NoteGate later runs in more than one cluster, add a `cluster` variable and
matcher before treating the aggregated Thanos datasource as a multi-cluster
dashboard.

## Validate changes

Validate JSON and Prometheus syntax:

```sh
jq empty deploy/observability/grafana/dashboards/*.json

docker run --rm --entrypoint=promtool \
  -v "$PWD/deploy/observability/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro" \
  prom/prometheus:v3.13.1 \
  check config /etc/prometheus/prometheus.yml
```

Then run the local stack, generate representative HTTP, MCP, `find`, and
`grep` traffic, and verify that all three dashboards render real values at
desktop and narrow-desktop widths. A zero value is valid for an event that did
not occur; `No data` means the datasource, scrape labels, query, or time range
still needs investigation.
