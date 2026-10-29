#!/usr/bin/env python3
"""Temporary public-core-only scanner diagnosis; never print scanner text."""
from collections import Counter
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import unittest
from urllib.request import Request, urlopen

import security_scan

CORE_ID = 1253185495
SOURCE_SHA = "02c538b499cc7cc5d70156e1f3f005c83dbaaf0c"
SAFE_PATH = re.compile(r"[A-Za-z0-9_.@+/-]{1,300}")
SAFE_RULE = re.compile(r"[A-Za-z0-9_.-]{1,128}")
ERROR_TYPES = {"ParseError", "PatternParseError", "RuleParseError", "SemgrepError", "Timeout", "OutOfMemory"}
LEVELS = {"error", "warn", "warning", "info"}


def public_core():
    req = Request("https://api.github.com/repos/cagojeiger/notegate-core",
                  headers={"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"})
    with urlopen(req, timeout=15) as response:
        data = json.load(response)
    return data.get("id") == CORE_ID and data.get("private") is False and data.get("visibility") == "public"


def safe_finding(item, source):
    if not isinstance(item, dict):
        return None
    path, rule = item.get("File"), item.get("RuleID")
    start, end = item.get("StartLine"), item.get("EndLine")
    if not isinstance(path, str) or not isinstance(rule, str) or not SAFE_RULE.fullmatch(rule):
        return None
    if type(start) is not int or type(end) is not int or not (1 <= start <= end <= 10000000):
        return None
    root = source.resolve()
    target = (root / path).resolve()
    try:
        relative = target.relative_to(root).as_posix()
    except ValueError:
        return None
    if not SAFE_PATH.fullmatch(relative) or any(part in (".", "..", ".git") for part in PurePosixPath(relative).parts):
        return None
    if not target.is_file():
        return None
    return f"{relative}:{rule}:{start} lines={start}-{end}"


def error_summary(payload):
    if not isinstance(payload, dict) or not isinstance(payload.get("errors"), list):
        return ["semgrep report: unavailable"]
    counts = Counter()
    for error in payload["errors"]:
        if not isinstance(error, dict):
            counts[("other", "other", "unknown")] += 1
            continue
        typ, level, code = error.get("type"), error.get("level"), error.get("code")
        typ = typ if isinstance(typ, str) and typ in ERROR_TYPES else "other"
        level = level.lower() if isinstance(level, str) and level.lower() in LEVELS else "other"
        code = str(code) if type(code) is int and 0 <= code <= 255 else "unknown"
        counts[(typ, level, code)] += 1
    return [f"semgrep error type={typ} level={level} code={code} count={count}"
            for (typ, level, code), count in sorted(counts.items())]


def diagnose(source, rules):
    lines = []

    def runner(command, **kwargs):
        result = subprocess.run(command, **kwargs)
        if command[0] == "semgrep" and len(command) > 1 and command[1] == "scan":
            lines.append(f"semgrep exit={result.returncode}")
            try:
                stream = kwargs["stdout"]
                stream.flush()
                stream.seek(0)
                lines.extend(error_summary(json.load(stream)))
            except (ValueError, OSError):
                lines.append("semgrep report: unavailable")
        elif command[0] == "gitleaks" and len(command) > 1 and command[1] == "dir":
            lines.append(f"gitleaks exit={result.returncode}")
            try:
                report = Path(command[command.index("--report-path") + 1])
                findings = json.loads(report.read_text())
                if findings is None:
                    findings = []
                if not isinstance(findings, list):
                    raise ValueError()
                for item in findings:
                    location = safe_finding(item, source)
                    lines.append("gitleaks " + location if location else "gitleaks location: unavailable")
            except (ValueError, OSError):
                lines.append("gitleaks report: unavailable")
        return result

    results = [security_scan.scan("semgrep", source, rules, runner),
               security_scan.scan("gitleaks", source, run=runner)]
    return lines, results


class SanitizationTests(unittest.TestCase):
    def test_error_values_are_allowlisted(self):
        result = error_summary({"errors": [{"type": "private-source", "level": "secret", "code": "x",
                                            "message": "do not print"},
                                           {"type": "ParseError", "level": "error", "code": 3}]})
        self.assertEqual(result, ["semgrep error type=ParseError level=error code=3 count=1",
                                  "semgrep error type=other level=other code=unknown count=1"])

    def test_path_validation_and_untrusted_fields(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "example.py").write_text("test")
            item = {"File": "example.py", "RuleID": "test-rule", "StartLine": 2, "EndLine": 3,
                    "Secret": "private", "Match": "private", "Fingerprint": "untrusted"}
            self.assertEqual(safe_finding(item, root), "example.py:test-rule:2 lines=2-3")
            for key, value in (("File", "../escape"), ("File", "example.py\nsecret"),
                               ("RuleID", "secret\nvalue"), ("StartLine", 0), ("EndLine", True)):
                self.assertIsNone(safe_finding({**item, key: value}, root))


def main():
    if sys.argv[1:] == ["--self-test"]:
        unittest.main(argv=[sys.argv[0]])
        return
    try:
        if not public_core():
            raise ValueError()
        source, rules = Path("source").resolve(), Path(".rules").resolve()
        commit = subprocess.run(["git", "-C", str(source), "rev-parse", "HEAD"],
                                capture_output=True, text=True, timeout=15, check=True).stdout.strip()
        if commit != SOURCE_SHA:
            raise ValueError()
        lines, results = diagnose(source, rules)
        if not public_core():
            raise ValueError()
        for line in lines:
            print(line)
        for scanner, (status, count) in zip(("semgrep", "gitleaks"), results):
            print(f"{scanner}: {status}; findings={count}")
        if any(status != "clean" for status, _ in results):
            raise SystemExit(1)
    except Exception:
        raise SystemExit("Diagnostic unavailable: public source or metadata validation failed") from None


if __name__ == "__main__":
    main()
