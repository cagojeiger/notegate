"""Summarize paired CI measurements; noisy timing is informational, not a gate."""
import json
import statistics
import sys
from pathlib import Path

root = Path(sys.argv[1])
measurements = {}
for label in ("baseline", "candidate"):
    cases = {}
    for trial in (1, 2, 3):
        seen = set()
        for line in (root / f"{label}-{trial}.log").read_text().splitlines():
            prefix = "NOTE_TEXT_WRITE_BENCH="
            if prefix not in line:
                continue
            row = json.loads(line.split(prefix, 1)[1])
            key = (row["bytes"], row["encrypted"], row["shape"])
            if key in seen or row["samples"] != 20 * row["workers"]:
                raise SystemExit("Unexpected benchmark samples")
            seen.add(key)
            cases.setdefault(key, []).append(row)
        if len(seen) != 24:
            raise SystemExit("Expected all 24 cases in each trial")
    if any(len(rows) != 3 for rows in cases.values()):
        raise SystemExit("Cases differ between trials")
    measurements[label] = cases
if measurements["baseline"].keys() != measurements["candidate"].keys():
    raise SystemExit("Baseline/candidate cases differ")
print("# Text write performance\n")
print(f"Baseline: `{(root / 'baseline.sha').read_text().strip()}`  ")
print(f"Candidate: `{(root / 'candidate.sha').read_text().strip()}`\n")
print("Same Linux CI runner and PostgreSQL 17; release profile; four DB connections; "
      "20 changed saves per writer after two warmups and an untimed CHECKPOINT per case; "
      "three paired trials in alternating order. Durability remains enabled. "
      "Service-call latency includes DB/pool/row-lock waiting and crypto; it excludes HTTP/auth middleware. "
      "Payloads are synthetic repeated characters. Values below are medians of trial p95/throughput, "
      "not fleet percentiles or production capacity. Timing has no pass/fail threshold.\n")
print("| KiB | Encryption | Shape | Base p95 ms | New p95 ms | p95 change | Base writes/s | New writes/s |")
print("|---:|---|---|---:|---:|---:|---:|---:|")
for key, before in measurements["baseline"].items():
    after = measurements["candidate"][key]
    bp = statistics.median(row["p95_ms"] for row in before)
    ap = statistics.median(row["p95_ms"] for row in after)
    bt = statistics.median(row["writes_per_second"] for row in before)
    at = statistics.median(row["writes_per_second"] for row in after)
    print(f"| {key[0] // 1024} | {'server' if key[1] else 'plain'} | {key[2]} | "
          f"{bp:.2f} | {ap:.2f} | {(ap / bp - 1) * 100:+.1f}% | {bt:.1f} | {at:.1f} |")
