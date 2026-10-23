#!/usr/bin/env python3
"""Exercise release selection against a real temporary Git history."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "baseline", Path(__file__).with_name("resolve-text-write-baseline.py"))
baseline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(baseline)


class BaselineTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.directory.name)
        self.addCleanup(self.directory.cleanup)
        self.addCleanup(os.chdir, self.previous)
        self.git("init", "-q")
        self.git("config", "user.email", "ci@example.com")
        self.git("config", "user.name", "CI")
        Path("README").write_text("first release")
        self.git("add", ".")
        self.git("commit", "-qm", "old layout")
        self.old = self.git("rev-parse", "HEAD")
        self.git("tag", "v0")
        for path in baseline.BASELINE_LAYOUT:
            Path(path).parent.mkdir(parents=True, exist_ok=True)
            Path(path).write_text("fixture")
        self.git("add", ".")
        self.git("commit", "-qm", "service test layout before benchmark")
        self.base = self.git("rev-parse", "HEAD")
        self.git("tag", "-a", "v1", "-m", "release one")
        Path("backend/crates/service/tests/text_write_performance.rs").write_text("harness")
        self.git("add", ".")
        self.git("commit", "-qm", "candidate harness")
        self.candidate = self.git("rev-parse", "HEAD")
        self.git("tag", "v2")

    def git(self, *args):
        return subprocess.check_output(["git", *args], text=True).strip()

    def release(self, tag, day, **flags):
        return {"tag_name": tag, "published_at": f"2026-10-{day:02}T00:00:00Z",
                "draft": False, "prerelease": False, **flags}

    def test_pins_previous_published_release_across_pages(self):
        pages = [[self.release("v2", 22), self.release("v1", 21, draft=True)],
                 [self.release("v1", 20, prerelease=True), self.release("v1", 19),
                  self.release("v0", 18)]]
        result = baseline.resolve(pages)
        self.assertEqual((result["sha"], result["ref"]), (self.base, "v1"))
        self.assertEqual(result["candidate_sha"], self.candidate)

    def test_no_compatible_release_is_explicit_skip(self):
        result = baseline.resolve([[self.release("v2", 22), self.release("v0", 20)]])
        self.assertEqual(result["sha"], "")
        self.assertIn("skipped", result["reason"])

    def test_future_release_is_not_a_baseline(self):
        self.git("commit", "--allow-empty", "-qm", "future")
        self.git("tag", "v3")
        self.git("checkout", "-q", self.candidate)
        result = baseline.resolve([[self.release("v3", 23), self.release("v1", 19)]])
        self.assertEqual(result["sha"], self.base)

    def test_manual_override_is_pinned(self):
        self.assertEqual(baseline.resolve([], "v1")["sha"], self.base)
        self.assertEqual(baseline.resolve([], self.base)["sha"], self.base)
        self.assertEqual(baseline.resolve([], "v0")["sha"], self.old)
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        self.assertEqual(baseline.resolve([], "main")["sha"], self.base)

    def test_manual_candidate_is_explicit_skip(self):
        result = baseline.resolve([], "v2")
        self.assertEqual(result["sha"], "")
        self.assertIn("equals candidate", result["reason"])

    def test_invalid_manual_override_fails(self):
        with self.assertRaises(ValueError):
            baseline.resolve([], "missing")

    def test_manual_nonancestor_is_rejected_before_build(self):
        self.git("commit", "--allow-empty", "-qm", "future")
        self.git("tag", "future")
        self.git("checkout", "-q", self.base)
        self.git("checkout", "-qb", "side-branch")
        self.git("commit", "--allow-empty", "-qm", "side branch")
        self.git("checkout", "-q", self.candidate)
        for ref in ["future", "side-branch"]:
            with self.subTest(ref=ref), self.assertRaisesRegex(ValueError, "ancestor"):
                baseline.resolve([], ref)

    def test_cli_reports_skip_and_retains_candidate(self):
        output = subprocess.check_output(
            [sys.executable, baseline.__file__], text=True,
            input=json.dumps([[self.release("v2", 22)]]),
            env={**os.environ, "BASE_REF": ""})
        self.assertEqual(output, "sha=\n")
        self.assertIn("comparison skipped", Path("text-write-results/baseline.md").read_text())
        retained = json.loads(Path("text-write-results/baseline.json").read_text())
        self.assertEqual(retained["candidate_sha"], self.candidate)

    def test_cli_manual_ref_uses_fixed_report_paths(self):
        output = subprocess.check_output(
            [sys.executable, baseline.__file__], text=True,
            input=json.dumps([[self.release("v2", 22)]]),
            env={**os.environ, "BASE_REF": "v1", "GITHUB_STEP_SUMMARY": "external.md"})
        self.assertEqual(output, f"sha={self.base}\n")
        self.assertFalse(Path("external.md").exists())
        self.assertIn("Manual baseline", Path("text-write-results/baseline.md").read_text())


if __name__ == "__main__":
    unittest.main()
