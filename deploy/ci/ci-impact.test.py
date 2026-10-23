#!/usr/bin/env python3
"""Contracts for conservative CI exclusions and malformed diff handling."""

import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("ci_impact", Path(__file__).with_name("ci-impact.py"))
impact = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(impact)


class ImpactContracts(unittest.TestCase):
    def selected(self, paths, event="pull_request"):
        result = impact.classify(event, paths)
        return {job for job in impact.JOBS if result[job]}

    def assert_full(self, paths, event="pull_request"):
        self.assertEqual(self.selected(paths, event), set(impact.JOBS))

    def test_main_manual_and_other_events_always_keep_all_jobs(self):
        for event in ("push", "workflow_dispatch", "schedule", "pull_request_target", ""):
            with self.subTest(event=event):
                self.assert_full(["docs/guide.md"], event)

    def test_documentation_allowlist_is_deliberately_narrow(self):
        paths = ["README.md", "SECURITY.md", "CLAUDE.md", "CONTRIBUTING.md", "AGENTS.md",
                 "docs/guide.md", "docs/development/한국어 안내.md"]
        self.assertEqual(self.selected(paths), set())
        for path in ("crates/api/README.md", "docs/example.json", "frontend/web/README.md",
                     "unknown.md", "docs-not/guide.md"):
            with self.subTest(path=path):
                self.assert_full([path])

    def test_frontend_unit_tests_only_run_web_even_with_documentation(self):
        self.assertEqual(self.selected(["docs/guide.md", "frontend/web/src/date.test.ts",
                                        "frontend/web/src/components/Clock.test.tsx"]), {"web"})

    def test_frontend_runtime_support_and_fixtures_keep_browser_and_image(self):
        for path in ("frontend/web/src/date.ts", "frontend/web/src/test/setup.ts",
                     "frontend/web/src/test/fixtures/data.json", "frontend/web/public/icon.svg",
                     "frontend/web/e2e/helpers.ts", "frontend/web/index.html"):
            with self.subTest(path=path):
                self.assertEqual(self.selected([path]), {"rust_build", "web", "browser", "image"})

    def test_mixed_unit_and_runtime_uses_union(self):
        self.assertEqual(self.selected(["frontend/web/src/date.test.ts", "docs/guide.md",
                                        "frontend/web/src/date.ts"]),
                         {"rust_build", "web", "browser", "image"})

    def test_rust_embedded_frontend_inputs_keep_full_suite(self):
        for path in ("frontend/web/src/design/theme.css",
                     "frontend/web/public/brand/svg/logo-horizontal-light.svg",
                     "frontend/web/public/brand/svg/logo-horizontal-dark.svg"):
            with self.subTest(path=path):
                self.assert_full([path, "frontend/web/src/date.test.ts"])

    def test_dependency_configuration_and_unknown_paths_keep_full_suite(self):
        for path in ("Cargo.lock", "Cargo.toml", "frontend/web/package.json", "pnpm-lock.yaml",
                     "frontend/web/vite.config.ts", "frontend/web/tsconfig.json", "crates/new/src/lib.rs",
                     "deploy/ci/ci-impact.py", ".github/workflows/ci.yml", "shared/schema.json",
                     "frontend/new-app/src/main.ts", "Makefile"):
            with self.subTest(path=path):
                self.assert_full(["docs/guide.md", path])

    def test_rename_source_and_destination_are_both_considered(self):
        self.assert_full(["crates/api/old.md", "docs/new.md"])
        self.assert_full(["docs/old.md", "crates/api/new.md"])
        # --no-renames reports deleted paths with the same framing as added paths.
        self.assertEqual(self.selected(["frontend/web/src/deleted.ts"]),
                         {"rust_build", "web", "browser", "image"})

    def test_missing_empty_and_noncanonical_paths_keep_full_suite(self):
        for paths in (None, [], "README.md", [None], [""], ["/docs/a.md"], ["docs//a.md"],
                      ["docs/./a.md"], ["docs/../README.md"], ["docs/a.md/"],
                      ["docs\\a.md"], ["docs/a\n.md"], ["docs/a\0.md"],
                      ["docs/a\u0085.md"], ["docs/a\u2028.md"]):
            with self.subTest(paths=paths):
                self.assert_full(paths)

    def test_nul_diff_requires_valid_utf8_complete_nonempty_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "paths"
            for data in (b"", b"README.md", b"\0", b"README.md\0\0", b"docs/\xff.md\0"):
                with self.subTest(data=data):
                    path.write_bytes(data)
                    self.assert_full(impact.read_paths(path))
            path.write_bytes("docs/한국어 안내.md\0README.md\0".encode())
            self.assertEqual(self.selected(impact.read_paths(path)), set())
            self.assert_full(impact.read_paths(Path(directory) / "missing"))
            self.assert_full(impact.read_paths(None))

    def test_cli_output_has_only_fixed_keys_and_safe_values(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "paths"
            path.write_bytes(b"frontend/web/src/a.test.ts\0")
            output = io.StringIO()
            with patch.object(sys, "argv", ["ci-impact.py", "--event", "pull_request",
                                             "--paths-file", str(path)]), \
                    contextlib.redirect_stdout(output):
                impact.main()
            self.assertEqual(output.getvalue().splitlines(), [
                "rust_build=false", "rust=false", "web=true", "browser=false", "postgres=false",
                "image=false", "scope=frontend-unit", "reason=frontend-unit-only"])

    def test_cli_missing_diff_and_output_injection_path_keep_full_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "paths"
            path.write_bytes(b"docs/a.md\nweb=false\0")
            for extra in ([], ["--paths-file", str(path)]):
                with self.subTest(extra=extra):
                    output = io.StringIO()
                    with patch.object(sys, "argv", ["ci-impact.py", "--event", "pull_request", *extra]), \
                            contextlib.redirect_stdout(output):
                        impact.main()
                    lines = output.getvalue().splitlines()
                    self.assertEqual(lines[:6], [f"{job}=true" for job in impact.JOBS])
                    self.assertEqual(lines[6], "scope=full")
                    self.assertEqual(len(lines), 8)


if __name__ == "__main__":
    unittest.main()
