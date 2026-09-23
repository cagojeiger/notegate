#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
runner="${script_dir}/test-rust.sh"
tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT
mkdir "$tmp_root/bin"
export TEST_RUNNER_LOG="$tmp_root/calls"

cat > "$tmp_root/bin/psql" <<'MOCK'
#!/usr/bin/env bash
set -eu
case "$*" in
  *'SELECT 1'*) echo preflight >> "$TEST_RUNNER_LOG"; exit "${TEST_PREFLIGHT_CODE:-0}" ;;
  *)
    echo cleanup >> "$TEST_RUNNER_LOG"
    cat >/dev/null
    exit "${TEST_CLEANUP_CODE:-0}"
    ;;
esac
MOCK
cat > "$tmp_root/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -eu
[[ "$NOTEGATE_TEST_RUN_ID" =~ ^[0-9a-f]{16}$ ]]
[[ "$*" == 'test --locked --workspace example_filter -- --exact' ]]
echo cargo >> "$TEST_RUNNER_LOG"
exit "${TEST_CARGO_CODE:-0}"
MOCK
chmod +x "$tmp_root/bin/psql" "$tmp_root/bin/cargo"

run_case() {
  local expected="$1"
  shift
  : > "$TEST_RUNNER_LOG"
  local result_code=0
  env PATH="$tmp_root/bin:$PATH" \
    NOTEGATE_TEST_DATABASE_URL=postgres://test/test \
    NOTEGATE_TEST_S3_ENDPOINT=http://storage.test \
    "$@" "$runner" example_filter -- --exact > "$tmp_root/output" 2>&1 || result_code=$?
  if [ "$result_code" -ne "$expected" ]; then
    cat "$tmp_root/output" >&2
    echo "expected exit ${expected}, got ${result_code}" >&2
    exit 1
  fi
}
assert_calls() {
  [ "$(cat "$TEST_RUNNER_LOG")" = "$1" ] || { cat "$TEST_RUNNER_LOG" >&2; exit 1; }
}
run_case 1 NOTEGATE_TEST_DATABASE_URL=
assert_calls ''
run_case 1 NOTEGATE_TEST_S3_ENDPOINT='   '
assert_calls ''
run_case 2 TEST_PREFLIGHT_CODE=2
assert_calls preflight
run_case 0
assert_calls $'preflight\ncargo\ncleanup'
run_case 101 TEST_CARGO_CODE=101
assert_calls $'preflight\ncargo\ncleanup'
run_case 1 TEST_CLEANUP_CODE=3
assert_calls $'preflight\ncargo\ncleanup'
run_case 101 TEST_CARGO_CODE=101 TEST_CLEANUP_CODE=3
assert_calls $'preflight\ncargo\ncleanup'
echo 'Rust test runner contract tests passed.'

if [ "${1:-}" != --postgres ]; then exit 0; fi
: "${NOTEGATE_TEST_DATABASE_URL:?PostgreSQL is required for cleanup integration tests}"
# Keep real psql; replace only cargo to simulate a failed test process.
rm "$tmp_root/bin/psql"
export TEST_RUNNER_SCHEMAS="$tmp_root/schemas"
cat > "$tmp_root/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
prefix="notegate_test_${NOTEGATE_TEST_RUN_ID}_"
owned="${prefix}00000000000000000000000000000001"
# Same prefix but not a TestDb name, and a valid schema owned by another run.
keep="${prefix}keep"
other="notegate_test_$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')_00000000000000000000000000000002"
printf '%s\n' "$owned" "$keep" "$other" > "$TEST_RUNNER_SCHEMAS"
psql --dbname="$NOTEGATE_TEST_DATABASE_URL" -X --set=ON_ERROR_STOP=1 <<SQL
CREATE SCHEMA "$owned";
CREATE TABLE "$owned".test_data (id integer);
CREATE SCHEMA "$keep";
CREATE SCHEMA "$other";
SQL
exit 101
MOCK
# Remove only schemas created by this regression test, including on assertion failure.
cleanup_postgres() {
  if [ -f "$TEST_RUNNER_SCHEMAS" ]; then
    while IFS= read -r schema; do
      psql --dbname="$NOTEGATE_TEST_DATABASE_URL" -X --set=ON_ERROR_STOP=1 --command="DROP SCHEMA IF EXISTS \"${schema}\" CASCADE" >/dev/null
    done < "$TEST_RUNNER_SCHEMAS"
  fi
  rm -rf "$tmp_root"
}
trap cleanup_postgres EXIT
result_code=0
PATH="$tmp_root/bin:$PATH" "$runner" > "$tmp_root/output" 2>&1 || result_code=$?
[ "$result_code" -eq 101 ] || { cat "$tmp_root/output" >&2; exit 1; }
index=0
while IFS= read -r schema; do
  exists="$(psql --dbname="$NOTEGATE_TEST_DATABASE_URL" -X -At --set=ON_ERROR_STOP=1 --set="schema=$schema" <<'SQL'
SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = :'schema');
SQL
)"
  if [ "$index" -eq 0 ]; then [ "$exists" = f ]; else [ "$exists" = t ]; fi
  index=$((index + 1))
done < "$TEST_RUNNER_SCHEMAS"
echo 'Failed-run cleanup removed its schema and preserved unrelated schemas.'
