#!/usr/bin/env python3
"""Exercise dependency violations using Cargo metadata-shaped fixtures."""

import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("check_architecture", Path(__file__).with_name("check-architecture.py"))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def metadata(source, dependency_name, **attributes):
    dependency = {"name": dependency_name, "kind": None, **attributes}
    names = set(checker.ALLOWED) | {source}
    return {
        "workspace_members": sorted(names),
        "packages": [
            {"id": name, "name": name, "dependencies": [dependency] if name == source else []}
            for name in sorted(names)
        ],
    }


class ArchitectureChecks(unittest.TestCase):
    def test_documented_dependencies_are_allowed(self):
        for source, target in [
            ("notegate-api", "notegate-service"),
            ("notegate-service", "notegate-db"),
            ("notegate-db", "notegate-jobs"),
            ("notegate-model", "notegate-text"),
            ("notegate-cli", "notegate-command"),
            ("notegate-jobs", "sqlx"),
        ]:
            with self.subTest(source=source, target=target):
                self.assertEqual(checker.check(metadata(source, target)), [])

    def test_reverse_and_application_dependencies_are_rejected(self):
        for source, target in [
            ("notegate-db", "notegate-service"),
            ("notegate-service", "notegate-api"),
            ("notegate-text", "notegate-db"),
            ("notegate-jobs", "notegate-model"),
            ("notegate-reconciliation", "notegate-db"),
            ("notegate-cli", "notegate-api"),
        ]:
            with self.subTest(source=source, target=target):
                errors = checker.check(metadata(source, target))
                self.assertEqual(len(errors), 1)
                self.assertIn(f"{source} -> {target}", errors[0])

    def test_alias_optional_target_and_build_dependencies_cannot_bypass_policy(self):
        for attributes in [
            {"rename": "storage"},
            {"optional": True},
            {"target": 'cfg(target_os = "windows")'},
            {"kind": "build"},
        ]:
            with self.subTest(attributes=attributes):
                self.assertTrue(checker.check(metadata("notegate-jobs", "notegate-db", **attributes)))

    def test_dev_dependency_may_use_real_database_test_support(self):
        self.assertEqual(checker.check(metadata("notegate-reconciliation", "notegate-db", kind="dev")), [])

    def test_new_workspace_crate_requires_a_boundary(self):
        errors = checker.check(metadata("notegate-new", "sqlx"))
        self.assertEqual(len(errors), 1)
        self.assertIn("define its dependency boundary", errors[0])

    def test_unclassified_local_dependency_is_rejected(self):
        self.assertTrue(checker.check(metadata("notegate-jobs", "local-domain", path="/workspace/local-domain")))


if __name__ == "__main__":
    unittest.main()
