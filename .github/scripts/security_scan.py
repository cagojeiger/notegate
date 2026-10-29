#!/usr/bin/env python3
"""Run local scanners while exposing only aggregate outcomes."""
import argparse
import hashlib
from collections import Counter
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

FIXTURES = Path(__file__).resolve().parents[1] / "security-rules" / "known-fixtures.json"
SECRET_RULES = Path(__file__).resolve().parents[1] / "security-rules" / "gitleaks.toml"
RULES_COMMIT = "16b9e6d45f10649f6a531ec4838ed9924372c3bc"
RULES_PATHS = ("python/lang/security/audit", "javascript/lang/security/audit", "typescript/lang/security/audit")
VERSIONS = {"semgrep": "1.182.0", "gitleaks": "8.30.1"}
FINDINGS_EXIT = 10


def filter_fixtures(source, findings):
    policy = json.loads(FIXTURES.read_text())
    if not isinstance(policy, dict) or set(policy) != {"fixtures"} or not isinstance(policy["fixtures"], list):
        raise ValueError("Invalid fixture policy")
    entries = {}
    for item in policy["fixtures"]:
        if not isinstance(item, dict) or set(item) != {"path", "rule", "start_line", "end_line", "sha256"}:
            raise ValueError("Invalid fixture entry")
        path, rule, start, end, digest = (item[k] for k in ("path", "rule", "start_line", "end_line", "sha256"))
        if (not isinstance(path, str) or not path or path.startswith("/")
                or any(part in {"", ".", ".."} for part in path.split("/"))
                or not isinstance(rule, str) or not re.fullmatch(r"[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*", rule)
                or len(rule) > 128 or type(start) is not int or type(end) is not int or start < 1 or end < start
                or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)):
            raise ValueError("Invalid fixture fields")
        identity = (path, rule, start, end)
        if identity in entries:
            raise ValueError("Duplicate fixture")
        entries[identity] = digest
    remaining, used = [], set()
    root = source.resolve()
    for finding in findings:
        if not isinstance(finding, dict):
            raise ValueError("Invalid finding")
        path = finding.get("File")
        if isinstance(path, str) and Path(path).is_absolute():
            try:
                path = str(Path(path).relative_to(root))
            except ValueError:
                remaining.append(finding)
                continue
        start, end = finding.get("StartLine"), finding.get("EndLine")
        if type(start) is not int or type(end) is not int:
            remaining.append(finding)
            continue
        identity = (path, finding.get("RuleID"), start, end)
        try:
            digest = entries.get(identity)
        except TypeError:
            raise ValueError("Invalid finding identity") from None
        if digest is None:
            remaining.append(finding)
            continue
        if identity in used:
            raise ValueError("Duplicate fixture finding")
        used.add(identity)
        target = root / path
        if any((root.joinpath(*path.split("/")[:i])).is_symlink() for i in range(1, len(path.split("/")) + 1)):
            raise ValueError("Fixture path contains symlink")
        if not target.is_file() or not target.resolve().is_relative_to(root):
            raise ValueError("Invalid fixture file")
        lines = target.read_bytes().splitlines(keepends=True)
        if identity[3] > len(lines) or hashlib.sha256(b"".join(lines[identity[2] - 1:identity[3]])).hexdigest() != digest:
            remaining.append(finding)
    return remaining


def scan(scanner, source, rules_repo=None, run=subprocess.run, emit=lambda _: None):
    """Return (status, count); never return scanner text or paths."""
    def error(stage):
        emit(f"error stage: {stage}")
        return "error", 0

    if scanner not in VERSIONS:
        return "unsupported", 0
    if not source.is_dir() or not SECRET_RULES.is_file():
        return error("input")
    # Gitleaks always reads the target's ignore file, even with an explicit ignore path.
    if scanner == "gitleaks" and (source / ".gitleaksignore").exists():
        return error("ignore-policy")
    if scanner == "semgrep" and (source / ".semgrepignore").exists():
        return error("ignore-policy")
    stage = "temporary-storage"
    try:
        with tempfile.TemporaryDirectory(prefix="notegate-security-") as directory:
            private = Path(directory)
            if private.resolve().is_relative_to(source.resolve()):
                return error(stage)
            env = {"PATH": os.environ.get("PATH", ""), "HOME": directory,
                   "SEMGREP_SEND_METRICS": "off"}
            with (private / "stdout").open("w+b") as stdout, (private / "stderr").open("w+b") as stderr:
                if scanner == "semgrep":
                    stage = "rules-bundle"
                    if rules_repo is None or not rules_repo.is_dir() or rules_repo.resolve().is_relative_to(source.resolve()):
                        return error(stage)
                    git = ["git", "-c", "core.fsmonitor=false", "-C", str(rules_repo)]
                    for args, expected in ((["rev-parse", "HEAD"], RULES_COMMIT),
                                           (["status", "--porcelain", "--untracked-files=all"], "")):
                        checked = run(git + args, cwd=directory, env=env, stdout=stdout,
                                      stderr=stderr, timeout=30, check=False)
                        stdout.seek(0)
                        actual = stdout.read().decode("utf-8").strip()
                        stdout.seek(0)
                        stdout.truncate()
                        if checked.returncode != 0 or actual != expected:
                            return error(stage)
                    if any(not (rules_repo / path).is_dir() for path in RULES_PATHS):
                        return error(stage)
                stage = "version"
                version = run([scanner, "--version"], cwd=directory, env=env,
                              stdout=stdout, stderr=stderr, timeout=30, check=False)
                stdout.seek(0)
                value = stdout.read().decode("utf-8").strip()
                if scanner == "gitleaks" and value.startswith("gitleaks version "):
                    value = value.removeprefix("gitleaks version ")
                if version.returncode != 0 or value.lstrip("v") != VERSIONS[scanner]:
                    return error(stage)
                stdout.seek(0)
                stdout.truncate()
                if scanner == "semgrep":
                    command = ["semgrep", "scan", *sum((["--config", str(rules_repo / path)] for path in RULES_PATHS), []), "--metrics=off",
                               "--disable-version-check", "--disable-nosem", "--no-git-ignore", "--no-rewrite-rule-ids",
                               "--exclude", ".git", "--strict", "--json", str(source)]
                else:
                    command = ["gitleaks", "dir", str(source), "--config", str(SECRET_RULES), "--no-banner", "--no-color",
                               "--ignore-gitleaks-allow", "--gitleaks-ignore-path", directory,
                               "--redact", "--exit-code", str(FINDINGS_EXIT), "--report-format", "json",
                               "--report-path", str(private / "report.json")]
                stage = "scanner-execution"
                result = run(command, cwd=directory, env=env, stdout=stdout, stderr=stderr,
                             timeout=900, check=False)
                if scanner == "semgrep":
                    if result.returncode != 0:
                        emit(f"semgrep exit code: {result.returncode}")
                        return error(stage)
                    stdout.seek(0)
                    stage = "scanner-report"
                    data = json.load(stdout)
                    if not isinstance(data, dict) or not isinstance(data.get("results"), list) or data.get("errors") != []:
                        return error(stage)
                    findings, key = data["results"], "check_id"
                else:
                    if result.returncode not in (0, FINDINGS_EXIT):
                        return error(stage)
                    stage = "scanner-report"
                    with (private / "report.json").open() as report:
                        data = json.load(report)
                    if data is None and result.returncode == 0:
                        data = []
                    if not isinstance(data, list):
                        return error(stage)
                    findings, key = data, "RuleID"
                    if (result.returncode == FINDINGS_EXIT) != bool(findings):
                        return error(stage)
                    stage = "fixture-policy"
                    findings = filter_fixtures(source, findings)
                    if len(findings) < len(data):
                        emit(f"reviewed fixture findings: {len(data) - len(findings)}")
                    stage = "scanner-report"
                counts = Counter()
                for finding in findings:
                    rule = finding.get(key) if isinstance(finding, dict) else None
                    if not isinstance(rule, str) or not re.fullmatch(r"[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*", rule) or len(rule) > 128:
                        return error(stage)
                    counts[rule] += 1
                for rule, count in sorted(counts.items()):
                    emit(f"rule {rule}: {count}")
                count = len(findings)
                return ("findings" if count else "clean"), count
    except (OSError, ValueError, UnicodeError, subprocess.SubprocessError):
        return error(stage)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("scanner", choices=tuple(VERSIONS))
    parser.add_argument("source", type=Path)
    parser.add_argument("--rules-repo", type=Path, help="Clean semgrep-rules checkout at the pinned commit")
    args = parser.parse_args()
    status, count = scan(args.scanner, args.source.resolve(), args.rules_repo.resolve() if args.rules_repo else None, emit=print)
    print(f"{args.scanner}: {status}; findings={count}")
    return 0 if status == "clean" else 1


if __name__ == "__main__":
    raise SystemExit(main())
