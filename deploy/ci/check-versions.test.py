#!/usr/bin/env python3
"""Exercise version drift with small isolated repository fixtures."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("check_versions", Path(__file__).with_name("check-versions.py"))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class VersionChecks(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "crate").mkdir()
        self.write("VERSION", "0.1.2\n")
        self.write("Cargo.toml", '[workspace]\nmembers = ["crate"]\n[workspace.package]\nversion = "0.1.2"\n')
        self.write("crate/Cargo.toml", '[package]\nname = "notegate-test"\nversion.workspace = true\n')
        self.write("Cargo.lock", '[[package]]\nname = "notegate-test"\nversion = "0.1.2"\n')

    def write(self, path, text):
        (self.root / path).write_text(text)

    def test_consistent_release(self):
        self.assertEqual(checker.check(self.root), [])

    def test_workspace_drift(self):
        self.write("VERSION", "0.1.3\n")
        self.assertTrue(any("workspace.package.version" in e for e in checker.check(self.root)))

    def test_lockfile_drift(self):
        self.write("Cargo.lock", '[[package]]\nname = "notegate-test"\nversion = "0.1.1"\n')
        self.assertTrue(any("Cargo.lock" in e for e in checker.check(self.root)))

    def test_member_stops_inheriting(self):
        self.write("crate/Cargo.toml", '[package]\nname = "notegate-test"\nversion = "0.1.2"\n')
        self.assertTrue(any("inherit" in e for e in checker.check(self.root)))

    def test_registry_package_cannot_replace_local_entry(self):
        with (self.root / "Cargo.lock").open("a") as lock:
            lock.write('source = "registry+https://github.com/rust-lang/crates.io-index"\n')
        self.assertTrue(any("local entry" in e for e in checker.check(self.root)))

    def test_invalid_release_version(self):
        for version in ["v0.1.2", "0.1.2-rc.1", "00.1.2", "garbage"]:
            with self.subTest(version=version):
                self.write("VERSION", version)
                self.assertIn("VERSION must be a stable major.minor.patch version", checker.check(self.root))


if __name__ == "__main__":
    unittest.main()
