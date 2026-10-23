#!/usr/bin/env python3
"""Partition Rust tests by service requirements without changing Cargo defaults."""

import argparse
from collections import Counter
from fnmatch import fnmatchcase
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import tarfile
import time

ROOT = Path(__file__).resolve().parents[2]
SELECTORS = ("package", "target_kind", "target", "test")
GROUPS = ("self-contained", "external")
TARGETS = ["--lib", "--bins", "--tests"]
ANSI = re.compile(r"\x1b\[[0-9;]*m")
BUNDLE_MANIFEST = "rust-tests.json"
API_EXECUTABLES = ("target/debug/notegate-api", "target/debug/examples/seed_user")


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


def logged(command, path, env=None, cwd=ROOT):
    """Keep Cargo's runtime environment and retain complete diagnostic output."""
    started = time.monotonic()
    lines = []
    with path.open("w") as log:
        with subprocess.Popen(command, cwd=cwd, env=env, text=True,
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


def listed(executable, ignored=False, cwd=ROOT):
    command = [executable, "--list", "--format", "terse"]
    if ignored:
        command.append("--ignored")
    output = subprocess.check_output(command, cwd=cwd, text=True)
    return {line.removesuffix(": test") for line in output.splitlines()
            if line.endswith(": test")}


def prepare(output, summary):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT, text=True))
    packages = {package["id"]: package for package in metadata["packages"]}
    code, seconds, lines = logged(
        ["cargo", "test", "--locked", "--workspace", "--no-run", "--message-format=json"],
        output / "compile.log")
    summary["compile_seconds"] = seconds
    if code:
        raise RuntimeError(f"Test compilation failed with exit {code}")
    artifacts = {}
    companions = set()
    for line in lines:
        if not line.startswith("{"):
            continue
        artifact = json.loads(line)
        if artifact.get("reason") != "compiler-artifact" or not artifact["executable"]:
            continue
        if not artifact["profile"]["test"]:
            # Cargo integration tests embed absolute CARGO_BIN_EXE_* paths.
            if artifact["target"]["kind"] == ["bin"]:
                companions.add(str(Path(artifact["executable"]).resolve()))
            continue
        kind, = artifact["target"]["kind"]
        if kind not in ("lib", "bin", "test"):
            raise ValueError(f"Unsupported test harness kind: {kind}")
        package = packages[artifact["package_id"]]
        artifacts[str(Path(artifact["executable"]).resolve())] = {
            "package": package["name"],
            "target_kind": kind, "target": artifact["target"]["name"],
            "cwd": str(Path(package["manifest_path"]).parent.relative_to(ROOT)),
            "runtime_env": {"CARGO_MANIFEST_DIR": str(Path(package["manifest_path"]).parent),
                            "CARGO_MANIFEST_PATH": package["manifest_path"],
                            "CARGO_PKG_NAME": package["name"],
                            "CARGO_PKG_VERSION": package["version"]},
        }
    tests = []
    for executable, target in artifacts.items():
        ignored = listed(executable, ignored=True, cwd=ROOT / target["cwd"])
        tests.extend(dict(target, test=name, ignored=name in ignored)
                     for name in sorted(listed(executable, cwd=ROOT / target["cwd"])))
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
    return tests, artifacts, companions


def provenance():
    """A prepared bundle is reusable within one source checkout and CI run."""
    subprocess.check_output(["git", "diff", "--exit-code", "HEAD", "--"], cwd=ROOT)
    return {"commit": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "root": str(ROOT), "system": platform.system(), "machine": platform.machine(),
            "libc": list(platform.libc_ver()),
            "os_release": Path("/etc/os-release").read_text() if Path("/etc/os-release").exists() else "",
            "repository": os.environ.get("GITHUB_REPOSITORY", ""),
            "run_id": os.environ.get("GITHUB_RUN_ID", ""),
            "image_os": os.environ.get("ImageOS", ""),
            "toolchain": (ROOT / "rust-toolchain.toml").read_text()}


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def bundle_path(name):
    path = ROOT / name
    if (Path(name).is_absolute() or ".." in Path(name).parts
            or not Path(name).parts or Path(name).parts[0] != "target"
            or not path.resolve().is_relative_to(ROOT / "target")):
        raise ValueError(f"Invalid prepared executable path: {name}")
    return path


def pack_bundle(path, kind, executables, summary, **metadata):
    started = time.monotonic()
    files = {}
    for name in sorted(executables):
        source = bundle_path(name)
        if not source.is_file() or source.is_symlink() or not os.access(source, os.X_OK):
            raise ValueError(f"Prepared executable is not a regular executable: {name}")
        files[name] = {"sha256": digest(source), "size": source.stat().st_size}
    manifest = dict(schema_version=1, kind=kind, provenance=provenance(), files=files, **metadata)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, "w") as archive:
        data = (json.dumps(manifest, indent=2) + "\n").encode()
        header = tarfile.TarInfo(BUNDLE_MANIFEST)
        header.size = len(data)
        archive.addfile(header, io.BytesIO(data))
        for name in files:
            archive.add(bundle_path(name), arcname=name, recursive=False)
    summary["bundle"] = {"path": str(path), "bytes": path.stat().st_size,
                         "executable_bytes": sum(item["size"] for item in files.values()),
                         "executable_count": len(files),
                         "export_seconds": time.monotonic() - started}


def restore_bundle(path, kind, summary, exact_target=None):
    started = time.monotonic()
    with tarfile.open(path, "r:") as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        if len(names) != len(set(names)) or BUNDLE_MANIFEST not in names:
            raise ValueError("Prepared bundle has duplicate entries or no manifest")
        manifest = json.load(archive.extractfile(BUNDLE_MANIFEST))
        if manifest["schema_version"] != 1 or manifest["kind"] != kind:
            raise ValueError("Unsupported prepared bundle version")
        current = provenance()
        if manifest["provenance"] != current:
            differences = sorted(key for key in current if manifest["provenance"].get(key) != current[key])
            raise ValueError(f"Prepared bundle provenance mismatch: {differences}")
        files = manifest["files"]
        if set(names) != {BUNDLE_MANIFEST, *files}:
            raise ValueError("Prepared bundle executable inventory mismatch")
        if kind == "api" and set(files) != set(API_EXECUTABLES):
            raise ValueError("Prepared API executable inventory mismatch")
        required = set(files)
        if exact_target:
            targets = manifest["artifacts"]
            required = set(files) - set(targets)
            required.update(name for name, target in targets.items()
                            if tuple(target[field] for field in SELECTORS[:-1]) == exact_target)
        for member in members:
            if not member.isfile():
                raise ValueError(f"Prepared bundle entry is not a regular file: {member.name}")
            if member.name == BUNDLE_MANIFEST:
                continue
            destination = bundle_path(member.name)
            expected = files[member.name]
            if member.size != expected["size"] or member.mode & 0o111 == 0:
                raise ValueError(f"Prepared executable metadata mismatch: {member.name}")
            if member.name not in required:
                continue
            if (destination.is_file() and os.access(destination, os.X_OK)
                    and digest(destination) == expected["sha256"]):
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            temporary = destination.with_name(destination.name + ".prepared")
            try:
                checksum = hashlib.sha256()
                with archive.extractfile(member) as source, temporary.open("wb") as target:
                    while block := source.read(1024 * 1024):
                        checksum.update(block)
                        target.write(block)
                if checksum.hexdigest() != expected["sha256"]:
                    raise ValueError(f"Prepared executable digest mismatch: {member.name}")
                temporary.chmod(0o755)
                temporary.replace(destination)
            finally:
                temporary.unlink(missing_ok=True)
    summary["prepared_seconds"] = time.monotonic() - started
    return manifest


def export_prepared(path, tests, artifacts, companions, summary):
    targets = {str(Path(executable).relative_to(ROOT)): target for executable, target in artifacts.items()}
    executables = {str(Path(executable).relative_to(ROOT)) for executable in set(artifacts) | companions}
    pack_bundle(path, "tests", executables, summary, tests=tests, artifacts=targets)


def load_prepared(path, summary, exact_target=None, verify_inventory=True):
    manifest = restore_bundle(path, "tests", summary, exact_target)
    tests = manifest["tests"]
    if not tests or not set(manifest["artifacts"]).issubset(manifest["files"]):
        raise ValueError("Prepared bundle test artifact inventory mismatch")
    rules = json.loads((ROOT / "deploy/ci/rust-test-resources.json").read_text())["rules"]
    classified = [dict(test) for test in tests]
    classify(classified, rules)
    if classified != tests:
        raise ValueError("Prepared resource classification mismatch")
    artifacts = {str(bundle_path(name)): target for name, target in manifest["artifacts"].items()}
    if exact_target:
        artifacts = {executable: target for executable, target in artifacts.items()
                     if tuple(target[field] for field in SELECTORS[:-1]) == exact_target}
        tests = [test for test in tests if tuple(test[field] for field in SELECTORS[:-1]) == exact_target]
    discovered = []
    for executable, target in artifacts.items():
        cwd = (ROOT / target["cwd"]).resolve()
        if not cwd.is_relative_to(ROOT) or not cwd.is_dir():
            raise ValueError("Prepared package working directory mismatch")
        if verify_inventory:
            ignored = listed(executable, ignored=True, cwd=cwd)
            discovered.extend(dict(target, test=name, ignored=name in ignored)
                              for name in sorted(listed(executable, cwd=cwd)))
    if verify_inventory:
        classify(discovered, rules)
        if sorted(discovered, key=test_id) != sorted(tests, key=test_id):
            raise ValueError("Prepared executable test inventory mismatch")
    summary["counts"] = {group: dict(Counter(
        "ignored" if test["ignored"] else "active"
        for test in tests if test["group"] == group)) for group in GROUPS}
    return tests, artifacts


def harness_environment(executable, target):
    env = dict(os.environ, **target["runtime_env"])
    # Cargo adds the package dependency directory when it launches a harness.
    library_path = "DYLD_FALLBACK_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
    env[library_path] = os.pathsep.join([str(Path(executable).parent), env.get(library_path, "")])
    return env


def execute_prepared(group, tests, artifacts):
    """Run libtest directly, retaining Cargo's package cwd and target framing."""
    for executable, target in artifacts.items():
        selected = [test for test in tests if all(test[field] == target[field]
                    for field in SELECTORS[:-1])
                    and (group == "baseline" or test["group"] == group)]
        if not selected:
            continue
        env = harness_environment(executable, target)
        print(f"     Running prepared tests ({executable})", flush=True)
        command = [executable, "--format", "pretty", "--color", "never"]
        if group != "baseline":
            command += ["--exact"] + sorted({test["test"] for test in selected})
        code = subprocess.run(command, cwd=ROOT / target["cwd"], env=env, check=False).returncode
        if code:
            return code
    return 0


def run_exact(test, artifacts, extra_args, output, summary):
    targets = [(executable, target) for executable, target in artifacts.items()
               if all(test[field] == target[field] for field in SELECTORS[:-1])]
    if len(targets) != 1:
        raise ValueError("Exact test executable not found once")
    executable, target = targets[0]
    # Keep libtest's result line intact while retaining successful test output.
    # With --nocapture, a measurement may print between `test NAME ...` and `ok`.
    extra_args = ["--show-output" if argument == "--nocapture" else argument
                  for argument in extra_args]
    header = f"     Running prepared tests ({executable})"
    print(header, flush=True)
    command = [executable, "--format", "pretty", "--color", "never", "--exact", test["test"], *extra_args]
    code, seconds, lines = logged(command, output / "exact.log", harness_environment(executable, target),
                                  cwd=ROOT / target["cwd"])
    actual = executed([header, *lines], artifacts)
    if actual.keys() != {test_id(test)}:
        raise ValueError("Exact test did not execute once")
    if code == 0:
        verify_results([dict(test, ignored=test["ignored"] and "--ignored" not in extra_args)], actual)
    summary["runs"].append({"label": test["test"], "exit_code": code,
                            "wall_seconds": seconds,
                            "results": [dict(zip(SELECTORS, identity), status=status)
                                        for identity, status in actual.items()]})
    return code


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


def run_group(group, label, tests, artifacts, output, summary, prepared=None):
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
    if prepared:
        command = [sys.executable, str(Path(__file__).resolve()), "_execute",
                   "--prepared", str(prepared), "--group", group,
                   "--output", str(output / "driver")]
        if group != "self-contained":
            command = [str(ROOT / "deploy/ci/test-rust.sh"), "--command", *command]
    else:
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
    parser.add_argument("mode", choices=(*GROUPS, "split", "measure", "prepare", "exact",
                                        "pack-api", "restore-api", "_execute"))
    parser.add_argument("--output", type=Path, default=ROOT / "target/test-suites")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--prepared", type=Path)
    parser.add_argument("--group", choices=(*GROUPS, "baseline"))
    parser.add_argument("--package")
    parser.add_argument("--target-kind", choices=("lib", "bin", "test"))
    parser.add_argument("--target")
    parser.add_argument("--test-name")
    arguments = sys.argv[1:]
    separator = arguments.index("--") if "--" in arguments else len(arguments)
    args = parser.parse_args(arguments[:separator])
    extra_args = arguments[separator + 1:]
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if extra_args and args.mode != "exact":
        parser.error("libtest arguments are supported only by exact mode")
    if args.mode in ("prepare", "pack-api") and not args.bundle:
        parser.error("--bundle is required")
    if args.mode in ("restore-api", "exact", "_execute") and not args.prepared:
        parser.error("--prepared is required")
    if args.mode == "_execute" and not args.group:
        parser.error("--group is required")
    if args.mode == "exact" and not all((args.package, args.target_kind, args.target, args.test_name)):
        parser.error("exact requires --package, --target-kind, --target and --test-name")
    if args.mode in ("external", "split", "measure"):
        for name in ("NOTEGATE_TEST_DATABASE_URL", "NOTEGATE_TEST_S3_ENDPOINT"):
            if not os.environ.get(name, "").strip():
                parser.error(f"{name} is required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    summary = {"mode": args.mode, "runs": [], "commit": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()}
    try:
        if args.mode == "pack-api":
            pack_bundle(args.bundle.resolve(), "api", API_EXECUTABLES, summary)
            return 0
        if args.mode == "restore-api":
            restore_bundle(args.prepared.resolve(), "api", summary)
            return 0
        if args.prepared:
            exact_target = (args.package, args.target_kind, args.target) if args.mode == "exact" else None
            tests, artifacts = load_prepared(args.prepared.resolve(), summary, exact_target,
                                             verify_inventory=args.mode != "_execute")
            (output / "inventory.json").write_text(json.dumps(tests, indent=2) + "\n")
        else:
            tests, artifacts, companions = prepare(output, summary)
        if args.mode == "_execute":
            return execute_prepared(args.group, tests, artifacts)
        if args.mode == "exact":
            identity = (args.package, args.target_kind, args.target, args.test_name)
            selected = [test for test in tests if test_id(test) == identity]
            if len(selected) != 1:
                raise ValueError(f"Exact test identity not found once: {identity}")
            return run_exact(selected[0], artifacts, extra_args, output, summary)
        if args.mode == "measure":
            for index in range(args.repetitions):
                order = ("baseline", *GROUPS) if index % 2 == 0 else (*GROUPS, "baseline")
                for group in order:
                    run_group(group, f"round-{index + 1}-{group}", tests, artifacts, output, summary,
                              args.prepared)
        elif args.mode != "prepare":
            for group in GROUPS if args.mode == "split" else (args.mode,):
                run_group(group, group, tests, artifacts, output, summary, args.prepared)
        # Doctests belong to the self-contained phase, once per split execution.
        if args.mode != "external" and not args.prepared:
            env = dict(os.environ)
            for name in list(env):
                if name.startswith("NOTEGATE_TEST_"):
                    env.pop(name)
            code, seconds, _ = logged(["cargo", "test", "--locked", "--workspace", "--doc"],
                                      output / "doctests.log", env)
            summary["doctest_seconds"] = seconds
            if code:
                raise RuntimeError(f"Doctests failed with exit {code}")
        if args.mode == "prepare":
            export_prepared(args.bundle.resolve(), tests, artifacts, companions, summary)
        return 0
    except (ValueError, RuntimeError, OSError, KeyError, tarfile.TarError,
            subprocess.CalledProcessError) as error:
        summary["error"] = str(error)
        print(error, file=sys.stderr)
        return 1
    finally:
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    sys.exit(main())
