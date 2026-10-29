"""Synthetic scanner boundary tests; no real source is scanned."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import security_scan


class FakeScanner:
    def __init__(self, scanner, data, code=0, version=None, revision=None, dirty=False):
        self.scanner, self.data, self.code = scanner, data, code
        self.version = version or security_scan.VERSIONS[scanner]
        self.revision, self.dirty = revision or security_scan.RULES_COMMIT, dirty
        self.calls = []

    def __call__(self, command, **kwargs):
        self.calls.append((command, kwargs))
        if command[0] == "git":
            if "rev-parse" in command:
                kwargs["stdout"].write((self.revision + "\n").encode())
            elif self.dirty:
                kwargs["stdout"].write(b"modified\n")
            return subprocess.CompletedProcess(command, 0)
        if command[1] == "--version":
            kwargs["stdout"].write((self.version + "\n").encode())
            return subprocess.CompletedProcess(command, 0)
        kwargs["stderr"].write(b"private test data must not be printed")
        if self.scanner == "semgrep":
            kwargs["stdout"].write(json.dumps(self.data).encode())
        else:
            Path(command[command.index("--report-path") + 1]).write_text(json.dumps(self.data))
        return subprocess.CompletedProcess(command, self.code)


class ScannerTests(unittest.TestCase):
    def test_semgrep_clean_findings_and_scanner_errors_are_distinct(self):
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as rules:
            for path in security_scan.RULES_PATHS:
                (Path(rules) / path).mkdir(parents=True)
            for data, code, expected in [({"results": [], "errors": []}, 0, ("clean", 0)),
                                         ({"results": [{"check_id": "python.security.test", "path": "private"}], "errors": []}, 0, ("findings", 1)),
                                         ({"results": [], "errors": [{"message": "private"}]}, 0, ("error", 0)),
                                         ({"results": [], "errors": []}, 2, ("error", 0))]:
                runner = FakeScanner("semgrep", data, code)
                self.assertEqual(security_scan.scan("semgrep", Path(source), Path(rules), runner), expected)
                command, args = runner.calls[-1]
                self.assertEqual(command.count("--config"), 3)
                for flag in ("--metrics=off", "--disable-version-check", "--disable-nosem", "--strict", "--json", "--no-git-ignore", "--no-rewrite-rule-ids"):
                    self.assertIn(flag, command)
                self.assertEqual(command[command.index("--exclude") + 1], ".git")
                self.assertNotIn("SEMGREP_APP_TOKEN", args["env"])
                self.assertNotIn("GH_TOKEN", args["env"])
                self.assertEqual(args["env"]["SEMGREP_SEND_METRICS"], "off")
                self.assertFalse(Path(args["cwd"]).exists())

    def test_gitleaks_report_and_exit_code_must_agree(self):
        with tempfile.TemporaryDirectory() as source:
            for data, code, expected in [(None, 0, ("clean", 0)), ([], 0, ("clean", 0)),
                                         ([{"RuleID": "test-rule", "Secret": "private"}], 10, ("findings", 1)),
                                         ([{}], 0, ("error", 0)), ([], 10, ("error", 0)),
                                         ([], 1, ("error", 0))]:
                runner = FakeScanner("gitleaks", data, code)
                self.assertEqual(security_scan.scan("gitleaks", Path(source), run=runner), expected)
                command, args = runner.calls[1]
                self.assertIn("--config", command)
                self.assertIn("--ignore-gitleaks-allow", command)
                self.assertIn("--redact", command)
                self.assertNotIn("GITLEAKS_CONFIG", args["env"])
                self.assertFalse(Path(args["cwd"]).exists())

    def test_wrong_version_missing_binary_and_source_ignore_fail_closed(self):
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as rules:
            root = Path(source)
            self.assertEqual(security_scan.scan("other", root), ("unsupported", 0))
            for path in security_scan.RULES_PATHS:
                (Path(rules) / path).mkdir(parents=True)
            self.assertEqual(security_scan.scan("semgrep", root, Path(rules), FakeScanner("semgrep", {}, version="0.0.0")), ("error", 0))
            with patch("security_scan.subprocess.run", side_effect=FileNotFoundError):
                # Pass the patched runner explicitly because the default is bound at definition time.
                self.assertEqual(security_scan.scan("semgrep", root, Path(rules), subprocess.run), ("error", 0))
            (root / ".gitleaksignore").write_text("ignored")
            runner = FakeScanner("gitleaks", [])
            self.assertEqual(security_scan.scan("gitleaks", root, run=runner), ("error", 0))
            self.assertFalse(runner.calls)

    def test_semgrep_rejects_wrong_dirty_or_missing_bundle(self):
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as rules:
            for path in security_scan.RULES_PATHS:
                (Path(rules) / path).mkdir(parents=True)
            for runner in (FakeScanner("semgrep", {}, revision="0" * 40), FakeScanner("semgrep", {}, dirty=True)):
                self.assertEqual(security_scan.scan("semgrep", Path(source), Path(rules), runner), ("error", 0))
                self.assertFalse(any(command[0] == "semgrep" for command, _ in runner.calls))
            self.assertEqual(security_scan.scan("semgrep", Path(source)), ("error", 0))

    def test_only_safe_rule_counts_and_fixed_error_stages_are_emitted(self):
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as rules:
            for path in security_scan.RULES_PATHS:
                (Path(rules) / path).mkdir(parents=True)
            output = []
            data = {"results": [{"check_id": "python.rule", "path": "private", "extra": {"message": "secret"}},
                                {"check_id": "python.rule"}], "errors": []}
            self.assertEqual(security_scan.scan("semgrep", Path(source), Path(rules), FakeScanner("semgrep", data), output.append), ("findings", 2))
            self.assertEqual(output, ["rule python.rule: 2"])
            output.clear()
            data["results"][0]["check_id"] = "private/path\nsecret"
            self.assertEqual(security_scan.scan("semgrep", Path(source), Path(rules), FakeScanner("semgrep", data), output.append), ("error", 0))
            self.assertEqual(output, ["error stage: scanner-report"])
            output.clear()
            (Path(source) / ".semgrepignore").write_text("*")
            self.assertEqual(security_scan.scan("semgrep", Path(source), Path(rules), FakeScanner("semgrep", data), output.append), ("error", 0))
            self.assertEqual(output, ["error stage: ignore-policy"])


if __name__ == "__main__":
    unittest.main()
