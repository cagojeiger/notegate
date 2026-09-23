#!/usr/bin/env bash
# Run the complete Rust suite, then remove only this run's leftover DB schemas.
set -euo pipefail

for name in NOTEGATE_TEST_DATABASE_URL NOTEGATE_TEST_S3_ENDPOINT; do
  value="${!name:-}"
  if [[ -z "${value//[[:space:]]/}" ]]; then
    echo "${name} is required; use make test-fast for checks without DB/S3 integration." >&2
    exit 1
  fi
done
command -v psql >/dev/null || { echo "PostgreSQL psql is required for integration test cleanup." >&2; exit 1; }
command -v cargo >/dev/null
PGCONNECT_TIMEOUT=5 psql --dbname="$NOTEGATE_TEST_DATABASE_URL" -X --set=ON_ERROR_STOP=1 --command='SELECT 1' >/dev/null

# 14-byte prefix + 16 hex digits + underscore + 32 UUID digits = 63 bytes.
NOTEGATE_TEST_RUN_ID="$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
export NOTEGATE_TEST_RUN_ID
schema_prefix="notegate_test_${NOTEGATE_TEST_RUN_ID}_"

cleanup() {
  local result_code=$?
  trap - EXIT
  if ! PGCONNECT_TIMEOUT=5 PGOPTIONS='-c lock_timeout=5000 -c statement_timeout=30000 -c client_min_messages=warning' \
    psql --dbname="$NOTEGATE_TEST_DATABASE_URL" -X --set=ON_ERROR_STOP=1 --set="schema_prefix=${schema_prefix}" <<'SQL'
SELECT format('DROP SCHEMA %I CASCADE', nspname)
FROM pg_namespace
WHERE nspname ~ ('^' || :'schema_prefix' || '[0-9a-f]{32}$')
ORDER BY nspname
\gexec
SQL
  then
    echo "Failed to clean up schemas for test run ${NOTEGATE_TEST_RUN_ID}." >&2
    if [ "$result_code" -eq 0 ]; then result_code=1; fi
  fi
  exit "$result_code"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

echo "Running Rust integration tests with PostgreSQL/S3; run ID ${NOTEGATE_TEST_RUN_ID}."
cargo test --locked --workspace "$@"
