#!/usr/bin/env bash
# Build once per revision, then alternate three paired runs on one CI runner/DB.
set -euo pipefail
[[ "${CI:-}" == "true" ]] || { echo 'Run performance comparison in CI only.' >&2; exit 1; }
: "${NOTEGATE_TEST_DATABASE_URL:?PostgreSQL is required}"
baseline_dir="${1:?Baseline checkout is required}"
candidate_dir="$PWD"
results_dir="$candidate_dir/text-write-results"
mkdir -p "$results_dir"
export CARGO_TARGET_DIR="$candidate_dir/target"
cp backend/crates/service/tests/text_write_performance.rs "$baseline_dir/backend/crates/service/tests/text_write_performance.rs"
for label in baseline candidate; do
  checkout_dir="$candidate_dir"
  if [[ "$label" == baseline ]]; then checkout_dir="$baseline_dir"; fi
  git -C "$checkout_dir" rev-parse HEAD > "$results_dir/$label.sha"
  (
    cd "$checkout_dir"
    cargo test --locked --release -p notegate-service --test text_write_performance --no-run --message-format=json
  ) > "$results_dir/$label-build.jsonl"
  python3 - "$results_dir" "$label" <<'PY'
import json, pathlib, shutil, sys
out, label = pathlib.Path(sys.argv[1]), sys.argv[2]
paths = [item['executable'] for line in (out / f'{label}-build.jsonl').read_text().splitlines()
         if (item := json.loads(line)).get('reason') == 'compiler-artifact'
         and item['target']['name'] == 'text_write_performance' and item.get('executable')]
if len(paths) != 1:
    raise SystemExit('Expected one benchmark executable')
shutil.copy2(paths[0], out / f'{label}-bench')
PY
done
for round in 1 2 3; do
  order='baseline candidate'
  if [[ "$round" == 2 ]]; then order='candidate baseline'; fi
  for label in $order; do
    "$results_dir/$label-bench" --ignored --exact compare_text_writes --nocapture --test-threads=1 \
      | tee "$results_dir/$label-$round.log"
  done
done
python3 deploy/ci/summarize-text-write.py "$results_dir" > "$results_dir/summary.md"
cat "$results_dir/summary.md" >> "${GITHUB_STEP_SUMMARY:?GitHub summary is required}"
rm "$results_dir/baseline-bench" "$results_dir/candidate-bench"
