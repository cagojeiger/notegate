#!/usr/bin/env python3
"""Regression contracts for resource partitioning and observed test coverage."""

import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "rust_test_suites", Path(__file__).with_name("rust-test-suites.py"))
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def test(name="unit", **values):
    return dict(dict(package="example", target_kind="lib", target="example", test=name,
                     ignored=False, group="self-contained"), **values)


def rule(group="self-contained", **values):
    return dict({field: "*" for field in runner.SELECTORS}, group=group, **values)


class SuiteContracts(unittest.TestCase):
    def setUp(self):
        self.path = str((runner.ROOT / "target/debug/deps/example-123").resolve())
        self.target = {key: test()[key] for key in runner.SELECTORS[:-1]}
        self.artifacts = {self.path: self.target}
        self.header = f"     Running unittests src/lib.rs ({self.path})"

    def test_missing_or_invalid_resource_rule_fails_closed(self):
        for rules in ([], [rule(group="typo")], [rule(package="other")]):
            with self.subTest(rules=rules), self.assertRaisesRegex(ValueError, "Unclassified"):
                runner.classify([test()], rules)

    def test_v2_resource_route_keeps_its_observed_database_requirement(self):
        manifest = json.loads((runner.ROOT / "deploy/ci/rust-test-resources.json").read_text())
        observed = test("auth::bearer_tests::v2_exposes_agent_resources_but_not_management_routes",
                        package="notegate-api", target_kind="bin", target="notegate-api")
        runner.classify([observed], manifest["rules"])
        self.assertEqual(observed["group"], "external")

    def test_global_exact_filter_cannot_cross_resource_groups(self):
        cases = [test(), test(target_kind="bin", target="example-cli")]
        runner.classify(cases, [rule()])  # The same name in one group is safe.
        with self.assertRaisesRegex(ValueError, "crosses resource groups"):
            runner.classify(cases, [rule("external", target_kind="bin"), rule()])

    def test_duplicate_compiled_identity_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Duplicate test identity"):
            runner.classify([test(), test()], [rule()])

    def test_cargo_target_identity_panic_and_ignored_results(self):
        second = self.path + "-bin"
        artifacts = dict(self.artifacts, **{second: dict(self.target, target_kind="bin")})
        cases = [test(), test("diagnostic", ignored=True), test(target_kind="bin")]
        lines = [self.header, "test unit - should panic ... ok",
                 "test diagnostic ... ignored, run explicitly",
                 f"     Running unittests src/main.rs ({second})", "test unit ... ok"]
        runner.verify_results(cases, runner.executed(lines, artifacts))

    def test_unknown_unframed_and_repeated_execution_are_rejected(self):
        outputs = (["test unit ... ok"],
                   ["     Running tests/a.rs (target/debug/deps/unknown)", "test unit ... ok"],
                   [self.header, "test unit ... ok", "test unit ... ok"])
        for lines in outputs:
            with self.subTest(lines=lines), self.assertRaises(ValueError):
                runner.executed(lines, self.artifacts)

    def test_missing_extra_failed_and_wrong_ignored_status_fail(self):
        identity = runner.test_id(test())
        for actual in ({}, {identity: "ok", runner.test_id(test("extra")): "ok"},
                       {identity: "FAILED"}, {identity: "ignored"}):
            with self.subTest(actual=actual), self.assertRaises(ValueError):
                runner.verify_results([test()], actual)
        with self.assertRaisesRegex(ValueError, "Unexpected test result"):
            runner.verify_results([test(ignored=True)], {identity: "ok"})

    def test_self_contained_execution_clears_services_without_hiding_ci(self):
        inherited = dict(CI="true", NOTEGATE_TEST_DATABASE_URL="postgres://example",
                         NOTEGATE_TEST_S3_ENDPOINT="http://s3", NOTEGATE_TEST_S3_REGION="region",
                         NOTEGATE_TEST_S3_SECRET_KEY="secret", NOTEGATE_TEST_RUN_ID="old",
                         NOTEGATE_TEST_SKIP_INTEGRATION="1", UNRELATED="keep")
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, inherited, clear=True), \
                patch.object(runner, "logged", return_value=(0, .1, [self.header, "test unit ... ok"])) as logged:
            runner.run_group("self-contained", "unit", [test()], self.artifacts,
                             Path(directory), {"runs": []})
            env = logged.call_args.args[2]
            self.assertEqual((env["CI"], env["UNRELATED"]), ("true", "keep"))
            self.assertFalse(set(inherited) & set(env) - {"CI", "UNRELATED"})
            self.assertTrue(Path(env["NOTEGATE_TEST_TIMING_FILE"]).is_absolute())

    def test_baseline_and_split_cover_the_same_active_and_ignored_ids(self):
        cases = [test(), test("db", group="external"), test("perf", group="external", ignored=True)]

        def logged(command, _path, _env):
            names = command[command.index("--exact") + 1:] if "--exact" in command else None
            lines = [self.header] + [f"test {case['test']} ... {'ignored, explicit only' if case['ignored'] else 'ok'}"
                                     for case in cases if names is None or case["test"] in names]
            return 0, .1, lines

        summary = {"runs": []}
        with tempfile.TemporaryDirectory() as directory, patch.object(runner, "logged", side_effect=logged), \
                patch.object(runner, "database_timings", return_value={"setup_count": 1}):
            for group in ("baseline", *runner.GROUPS):
                runner.run_group(group, group, cases, self.artifacts, Path(directory), summary)
        baseline = {runner.test_id(case) for case in summary["runs"][0]["results"]}
        split = [runner.test_id(case) for result in summary["runs"][1:] for case in result["results"]]
        self.assertEqual(set(split), baseline)
        self.assertEqual(len(split), len(baseline))


if __name__ == "__main__":
    unittest.main()
