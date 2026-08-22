#!/bin/sh
set -eu

PROMETHEUS_URL="${PROMETHEUS_URL:-http://prometheus:9090}"
GRAFANA_URL="${GRAFANA_URL:-http://grafana:3000}"
GRAFANA_ADMIN_USER="${GRAFANA_ADMIN_USER:-admin}"
GRAFANA_ADMIN_PASSWORD="${GRAFANA_ADMIN_PASSWORD:-notegate-local}"

has_prometheus_value_one() {
  grep -Eq '"value":\[[^]]*,"1"\]'
}

wait_url() {
  url="$1"
  attempt=0
  until curl -fsS "$url" >/dev/null; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 120 ]; then
      echo "timed out waiting for $url" >&2
      exit 1
    fi
    sleep 0.5
  done
}

require_contains() {
  value="$1"
  expected="$2"
  label="$3"
  if ! printf '%s' "$value" | grep -Fq "$expected"; then
    echo "$label did not contain $expected" >&2
    exit 1
  fi
}

assert_role() {
  role="$1"
  base_url="$2"
  wait_url "$base_url/health"
  wait_url "$base_url/ready"
  metrics="$(curl -fsS "$base_url/metrics")"
  require_contains "$metrics" "process_mode=\"$role\"" "$role metrics"
}

prometheus_query() {
  query="$1"
  curl -fsSG "$PROMETHEUS_URL/api/v1/query" --data-urlencode "query=$query"
}

wait_prometheus_role() {
  role="$1"
  query="(count(up{job=\"notegate\",scrape_role=\"$role\"}) == 1) and (sum(up{job=\"notegate\",scrape_role=\"$role\"}) == 1)"
  attempt=0
  while :; do
    response="$(prometheus_query "$query")"
    if printf '%s' "$response" | has_prometheus_value_one; then
      return 0
    fi
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 120 ]; then
      echo "Prometheus did not report exactly one healthy $role target" >&2
      echo "$response" >&2
      exit 1
    fi
    sleep 0.5
  done
}

wait_prometheus_role_absent_or_down() {
  role="$1"
  query="sum(up{job=\"notegate\",scrape_role=\"$role\"}) > 0"
  attempt=0
  while :; do
    response="$(prometheus_query "$query")"
    if printf '%s' "$response" | grep -Fq '"result":[]'; then
      return 0
    fi
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 120 ]; then
      echo "Prometheus still reports a healthy $role target" >&2
      echo "$response" >&2
      exit 1
    fi
    sleep 0.5
  done
}

assert_api_unreachable() {
  attempt=0
  while curl --connect-timeout 1 --max-time 2 -fsS http://api:9191/ready >/dev/null 2>&1; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 20 ]; then
      echo "API is still reachable during the isolation scenario" >&2
      exit 1
    fi
    sleep 0.5
  done
}

assert_grafana_provisioning() {
  wait_url "$GRAFANA_URL/api/health"
  health="$(curl -fsS "$GRAFANA_URL/api/health")"
  if ! printf '%s' "$health" | grep -Eq '"database"[[:space:]]*:[[:space:]]*"ok"'; then
    echo "Grafana database health is not ok" >&2
    exit 1
  fi

  datasource="$(curl -fsS -u "$GRAFANA_ADMIN_USER:$GRAFANA_ADMIN_PASSWORD" \
    "$GRAFANA_URL/api/datasources/uid/prometheus")"
  require_contains "$datasource" '"uid":"prometheus"' "Grafana datasource"
  require_contains "$datasource" '"url":"http://prometheus:9090"' "Grafana datasource"

  dashboards="$(curl -fsS -u "$GRAFANA_ADMIN_USER:$GRAFANA_ADMIN_PASSWORD" \
    "$GRAFANA_URL/api/search?type=dash-db")"
  for title in \
    "NoteGate Service Overview" \
    "NoteGate Search Detail" \
    "NoteGate Internals Detail"; do
    require_contains "$dashboards" "\"title\":\"$title\"" "Grafana dashboards"
  done
}

run_full_smoke() {
  wait_url "$PROMETHEUS_URL/-/ready"
  for role_url in \
    "api=http://api:9191" \
    "search=http://search:9192" \
    "worker=http://worker:9191" \
    "reconciler=http://reconciler:9191"; do
    role="${role_url%%=*}"
    base_url="${role_url#*=}"
    assert_role "$role" "$base_url"
    wait_prometheus_role "$role"
  done
  assert_grafana_provisioning
  echo "split topology health, metrics, and Grafana provisioning passed"
}

run_isolation_smoke() {
  assert_api_unreachable
  wait_url "$PROMETHEUS_URL/-/ready"
  wait_prometheus_role_absent_or_down api
  for role_url in \
    "search=http://search:9192" \
    "worker=http://worker:9191" \
    "reconciler=http://reconciler:9191"; do
    role="${role_url%%=*}"
    base_url="${role_url#*=}"
    assert_role "$role" "$base_url"
    wait_prometheus_role "$role"
  done
  assert_grafana_provisioning
  echo "split topology remains healthy while the API is stopped"
}

case "${1:-full}" in
  full)
    run_full_smoke
    ;;
  isolation)
    run_isolation_smoke
    ;;
  *)
    echo "unknown split smoke scenario: $1" >&2
    exit 2
    ;;
esac
