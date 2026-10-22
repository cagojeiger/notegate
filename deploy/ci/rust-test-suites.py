#!/usr/bin/env python3
"""Partition Rust tests by service requirements without changing Cargo defaults."""

import argparse
from collections import Counter
from fnmatch import fnmatchcase
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
SELECTORS = ("package", "target_kind", "target", "test")
GROUPS = ("self-contained", "external")
TARGETS = ["--lib", "--bins", "--tests"]
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def test_id(test):
    return tuple(test[field] for field in SELECTORS)


def classify(tests, rules):
    names = {}
    for test in tests:
        rule = next((rule for rule in rules if all(
            fnmatchcase(test[field], rule[field]) for field in SELECTORS
        )), None)
        if rule is None or rule["group"] not in GROUPS:
            raise ValueError(f"Unclassified test: {test_id(test)}")
        test["group"] = rule["group"]
        previous = names.setdefault(test["test"], test["group"])
        if previous != test["group"]:
            raise ValueError(f"Exact filter crosses resource groups: {test['test']}")
    ids = [test_id(test) for test in tests]
    if len(ids) != len(set(ids)):
        raise ValueError("Duplicate test identity in compiled inventory")


def logged(command, path, env=None):
    """Keep Cargo's runtime environment and retain complete diagnostic output."""
    started = time.monotonic()
    lines = []
    with path.open("w") as log:
        with subprocess.Popen(command, cwd=ROOT, env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as child:
            for line in child.stdout:
                line = ANSI.sub("", line)
                log.write(line)
                lines.append(line.rstrip("\n"))
                if not line.startswith("{"):
                    print(line, end="", flush=True)
                else:
                    try:
                        message = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if message.get("reason") == "compiler-message":
                        print(message["message"].get("rendered", ""), end="", flush=True)
            code = child.wait()
    return code, time.monotonic() - started, lines


def listed(executable, ignored=False):
    command = [executable, "--list", "--format", "terse"]
    if ignored:
        command.append("--ignored")
    output = subprocess.check_output(command, cwd=ROOT, text=True)
    return {line.removesuffix(": test") for line in output.splitlines()
            if line.endswith(": test")}


def prepare(output, summary):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT, text=True))
    packages = {package["id"]: package["name"] for package in metadata["packages"]}
    code, seconds, lines = logged(
        ["cargo", "test", "--locked", "--workspace", "--no-run", "--message-format=json"],
        output / "compile.log")
    summary["compile_seconds"] = seconds
    if code:
        raise RuntimeError(f"Test compilation failed with exit {code}")
    artifacts = {}
    for line in lines:
        if not line.startswith("{"):
            continue
        artifact = json.loads(line)
        if (artifact.get("reason") != "compiler-artifact"
                or not artifact["profile"]["test"] or not artifact["executable"]):
            continue
        kind, = artifact["target"]["kind"]
        if kind not in ("lib", "bin", "test"):
            raise ValueError(f"Unsupported test harness kind: {kind}")
        artifacts[str(Path(artifact["executable"]).resolve())] = {
            "package": packages[artifact["package_id"]],
            "target_kind": kind, "target": artifact["target"]["name"],
        }
    tests = []
    for executable, target in artifacts.items():
        ignored = listed(executable, ignored=True)
        tests.extend(dict(target, test=name, ignored=name in ignored)
                     for name in sorted(listed(executable)))
    if not tests:
        raise ValueError("Compiled inventory contains no tests")
    manifest = json.loads((ROOT / "deploy/ci/rust-test-resources.json").read_text())
    if manifest["schema_version"] != 1:
        raise ValueError("Unsupported resource manifest version")
    classify(tests, manifest["rules"])
    (output / "inventory.json").write_text(json.dumps(tests, indent=2) + "\n")
    summary["counts"] = {group: dict(Counter(
        "ignored" if test["ignored"] else "active"
        for test in tests if test["group"] == group)) for group in GROUPS}
    return tests, artifacts


def executed(lines, artifacts):
    target = None
    result = {}
    for line in lines:
        running = re.match(r"\s*Running .+ \((.+)\)$", line)
        if running:
            target = artifacts.get(str((ROOT / running[1]).resolve()))
            if target is None:
                raise ValueError(f"Unexpected executable: {running[1]}")
        case = re.match(r"test (\S+)(?: - should panic)? \.\.\. (ok|FAILED|ignored.*)$", line)
        if case:
            if target is None:
                raise ValueError("Test result without a Cargo target")
            identity = test_id(dict(target, test=case[1]))
            if identity in result:
                raise ValueError(f"Test ran twice: {identity}")
            result[identity] = case[2]
    return result


def verify_results(tests, results):
    expected = {test_id(test): test["ignored"] for test in tests}
    if expected.keys() != results.keys():
        missing = sorted(expected.keys() - results.keys())
        extra = sorted(results.keys() - expected.keys())
        raise ValueError(f"Test coverage mismatch: missing={missing}, extra={extra}")
    for identity, ignored in expected.items():
        status = results[identity]
        if (ignored and not status.startswith("ignored")) or (not ignored and status != "ok"):
            raise ValueError(f"Unexpected test result: {identity}: {status}")


def database_timings(path):
    records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    setups = {row["schema"] for row in records if row["phase"] == "setup"}
    cleanups = {row["schema"] for row in records if row["phase"] == "cleanup"}
    result = {"setup_count": len(setups), "cleanup_count": len(cleanups),
              "without_individual_cleanup": len(setups - cleanups),
              "failed_schema_drops": sum(row.get("schema_dropped") is False for row in records)}
    for field in ("setup_total_ms", "migration_ms", "fixture_ms", "fixture_lifetime_ms", "cleanup_ms"):
        values = sorted(row[field] for row in records if field in row)
        if values:
            result[field] = {"sum": sum(values), "median": statistics.median(values),
                             "p95": values[min(len(values) - 1, int(len(values) * .95))]}
    return result


def run_group(group, label, tests, artifacts, output, summary):
    selected = [test for test in tests if group == "baseline" or test["group"] == group]
    if not selected:
        raise ValueError(f"No tests selected for {group}")
    env = dict(os.environ, CARGO_TERM_COLOR="never")
    env.pop("NOTEGATE_TEST_SKIP_INTEGRATION", None)
    if group == "self-contained":
        for name in list(env):
            if name.startswith("NOTEGATE_TEST_S3_") or name in (
                    "NOTEGATE_TEST_DATABASE_URL", "NOTEGATE_TEST_RUN_ID"):
                env.pop(name)
    timing_file = output / f"{label}-db.jsonl"
    timing_file.unlink(missing_ok=True)
    env["NOTEGATE_TEST_TIMING_FILE"] = str(timing_file)
    command = (["cargo", "test", "--locked", "--workspace"] if group == "self-contained"
               else [str(ROOT / "deploy/ci/test-rust.sh")])
    command += TARGETS + ["--", "--format", "pretty", "--color", "never"]
    if group != "baseline":
        command += ["--exact"] + sorted({test["test"] for test in selected})
    code, seconds, lines = logged(command, output / f"{label}.log", env)
    result = {"label": label, "group": group, "wall_seconds": seconds, "exit_code": code,
              "database": database_timings(timing_file)}
    summary["runs"].append(result)
    actual = executed(lines, artifacts)
    result["results"] = [dict(zip(SELECTORS, identity), status=status)
                         for identity, status in sorted(actual.items())]
    if code:
        raise RuntimeError(f"{label} failed with exit {code}")
    verify_results(selected, actual)
    if group != "self-contained" and result["database"]["setup_count"] == 0:
        raise ValueError("External suite produced no DB timing records")
    if any("failed to write test timing" in line for line in lines):
        raise ValueError("DB timing collection failed")
    print(f"{label}: {seconds:.2f}s, exact test coverage verified", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=(*GROUPS, "split", "measure"))
    parser.add_argument("--output", type=Path, default=ROOT / "target/test-suites")
    parser.add_argument("--repetitions", type=int, default=3)
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if args.mode != "self-contained":
        for name in ("NOTEGATE_TEST_DATABASE_URL", "NOTEGATE_TEST_S3_ENDPOINT"):
            if not os.environ.get(name, "").strip():
                parser.error(f"{name} is required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    summary = {"mode": args.mode, "runs": [], "commit": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()}
    try:
        tests, artifacts = prepare(output, summary)
        if args.mode == "measure":
            for index in range(args.repetitions):
                order = ("baseline", *GROUPS) if index % 2 == 0 else (*GROUPS, "baseline")
                for group in order:
                    run_group(group, f"round-{index + 1}-{group}", tests, artifacts, output, summary)
        else:
            for group in GROUPS if args.mode == "split" else (args.mode,):
                run_group(group, group, tests, artifacts, output, summary)
        # Doctests belong to the self-contained phase, once per split execution.
        if args.mode != "external":
            env = dict(os.environ)
            for name in list(env):
                if name.startswith("NOTEGATE_TEST_"):
                    env.pop(name)
            code, seconds, _ = logged(["cargo", "test", "--locked", "--workspace", "--doc"],
                                      output / "doctests.log", env)
            summary["doctest_seconds"] = seconds
            if code:
                raise RuntimeError(f"Doctests failed with exit {code}")
        return 0
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        summary["error"] = str(error)
        print(error, file=sys.stderr)
        return 1
    finally:
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    sys.exit(main())
