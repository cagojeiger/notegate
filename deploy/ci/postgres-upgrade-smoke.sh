#!/usr/bin/env bash
set -euo pipefail
umask 077
trap 'echo "FAIL: PostgreSQL upgrade smoke at line $LINENO" >&2' ERR

: "${GITHUB_RUN_ID:?Run only in GitHub CI}"
: "${COMPOSE_PROJECT_NAME:?}"
archive="${RUNNER_TEMP:?}/postgres17.dump"
legacy="${COMPOSE_PROJECT_NAME}-legacy"
guard="${COMPOSE_PROJECT_NAME}-guard"
volume="${COMPOSE_PROJECT_NAME}-pg17"
compose=(docker compose -f docker-compose.yml)
psql_args=(-U notegate -d notegate -v ON_ERROR_STOP=1 -At)

cleanup() {
  docker stop "$legacy" "$guard" >/dev/null 2>&1 || true
  docker rm "$legacy" "$guard" >/dev/null 2>&1 || true
}
trap cleanup EXIT

wait_for_legacy() {
  for _ in $(seq 1 60); do
    if docker exec "$legacy" pg_isready -h 127.0.0.1 -U notegate >/dev/null 2>&1; then
      return
    fi
    sleep 1
  done
  echo 'PostgreSQL 17 did not become ready' >&2
  return 1
}

probe="SELECT count(*) = 1 AND bool_and(
  id = '11111111-1111-4111-8111-111111111111'::uuid
  AND payload = '{\"name\":\"한글\",\"enabled\":true}'::jsonb
  AND bytes = decode('0001ff', 'hex')
  AND recorded_at = '2030-01-01T00:00:00Z'::timestamptz
) FROM upgrade_probe;"

docker run -d --name "$legacy" \
  -e POSTGRES_USER=notegate -e POSTGRES_PASSWORD=notegate -e POSTGRES_DB=notegate \
  -v "$volume:/var/lib/postgresql/data" postgres:17-alpine >/dev/null
wait_for_legacy
docker exec -i "$legacy" psql "${psql_args[@]}" >/dev/null <<'SQL'
CREATE TABLE upgrade_probe (id uuid PRIMARY KEY, payload jsonb, bytes bytea, recorded_at timestamptz);
INSERT INTO upgrade_probe VALUES (
  '11111111-1111-4111-8111-111111111111', '{"name":"한글","enabled":true}',
  decode('0001ff', 'hex'), '2030-01-01T00:00:00Z'
);
SQL
docker exec "$legacy" pg_dump -U notegate -d notegate --format=custom >"$archive"
docker stop "$legacy" >/dev/null

docker run -d --name "$guard" \
  -e POSTGRES_USER=notegate -e POSTGRES_PASSWORD=notegate -e POSTGRES_DB=notegate \
  -v "$volume:/var/lib/postgresql" postgres:18-alpine >/dev/null
[[ "$(timeout 60 docker wait "$guard")" == 1 ]]
docker logs "$guard" >"${RUNNER_TEMP}/postgres18-legacy.log" 2>&1
grep -q 'there appears to be PostgreSQL data in' "${RUNNER_TEMP}/postgres18-legacy.log"
docker start "$legacy" >/dev/null
wait_for_legacy
[[ "$(docker exec "$legacy" cat /var/lib/postgresql/data/PG_VERSION)" == 17 ]]
[[ "$(docker exec "$legacy" psql "${psql_args[@]}" -c "$probe")" == t ]]
docker stop "$legacy" >/dev/null
echo 'PASS: PostgreSQL 18 refuses the legacy volume; PostgreSQL 17 data remains usable'

"${compose[@]}" up -d --wait --wait-timeout 60 postgres
"${compose[@]}" exec -T postgres pg_restore -U notegate -d notegate \
  --exit-on-error --no-owner <"$archive"
[[ "$("${compose[@]}" exec -T postgres psql "${psql_args[@]}" -c 'SHOW server_version_num')" == 18???? ]]
[[ "$("${compose[@]}" exec -T postgres psql "${psql_args[@]}" -c "$probe")" == t ]]
"${compose[@]}" down
"${compose[@]}" up -d --wait --wait-timeout 60 postgres
[[ "$("${compose[@]}" exec -T postgres psql "${psql_args[@]}" -c "$probe")" == t ]]
echo 'PASS: PostgreSQL 17 logical backup restores into Compose PostgreSQL 18 and survives container replacement'
