#!/usr/bin/env python3
"""Regression contracts for resource partitioning and observed test coverage."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
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

    def test_prepared_driver_argv_is_fixed_and_inputs_travel_as_environment(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(
                runner, "logged", return_value=(0, .1, [self.header, "test unit ... ok"])) as logged:
            output = Path(directory)
            prepared = output / "--bundle with spaces.tar"
            runner.run_group("self-contained", "unit", [test()], self.artifacts,
                             output, {"runs": []}, prepared)
            command, _path, env = logged.call_args.args
            self.assertEqual(command, [sys.executable, str(Path(runner.__file__).resolve()), "_execute"])
            self.assertEqual(env["CI_RUST_PREPARED_PATH"], str(prepared))
            self.assertEqual(env["CI_RUST_GROUP"], "self-contained")
            self.assertEqual(env["CI_RUST_OUTPUT_PATH"], str(output / "driver"))


class PreparedContracts(unittest.TestCase):
    """Exercise real bundle transport and libtest processes without compiling Rust."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.root_patch = patch.object(runner, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        scripts = self.root / "deploy/ci"
        scripts.mkdir(parents=True)
        for name in ("rust-test-suites.py", "test-rust.sh"):
            shutil.copy2(Path(__file__).with_name(name), scripts / name)
        self.script = scripts / "rust-test-suites.py"
        self.rules = [rule("external", test="external"), rule()]
        (scripts / "rust-test-resources.json").write_text(json.dumps(
            {"schema_version": 1, "rules": self.rules}))
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "test"\n')
        (self.root / ".gitignore").write_text("target/\n")
        self.cwd = self.root / "backend/crates/example"
        self.cwd.mkdir(parents=True)
        (self.cwd / "Cargo.toml").write_text("# test fixture\n")
        self.executable = self.root / "target/debug/deps/example-123"
        self.executable.parent.mkdir(parents=True)
        self.executable.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
names = ["unit", "external", "diagnostic"]
if "--list" in sys.argv:
    print("\\n".join(name + ": test" for name in names if "--ignored" not in sys.argv or name == "diagnostic"))
    sys.exit(0)
selected = [name for name in names if "--exact" not in sys.argv or name in sys.argv]
record = dict(names=selected, cwd=os.getcwd(), args=sys.argv[1:],
              env={name: value for name, value in os.environ.items()
                   if name.startswith(("NOTEGATE_TEST_", "CARGO_"))})
with open(os.environ["HARNESS_LOG"], "a") as log:
    log.write(json.dumps(record) + "\\n")
code = 0
for name in selected:
    status = "ignored, explicit only" if name == "diagnostic" and "--ignored" not in sys.argv else "ok"
    if name == "external":
        if not os.environ.get("NOTEGATE_TEST_DATABASE_URL"):
            status, code = "FAILED", 101
        elif os.environ.get("FAIL_EXTERNAL"):
            status, code = "FAILED", 23
        elif os.environ.get("NOTEGATE_TEST_TIMING_FILE"):
            pathlib.Path(os.environ["NOTEGATE_TEST_TIMING_FILE"]).write_text(
                json.dumps(dict(schema="owned", phase="setup")) + "\\n" +
                json.dumps(dict(schema="owned", phase="cleanup", schema_dropped=True)) + "\\n")
    if name == "diagnostic" and "--nocapture" in sys.argv:
        print("test " + name + " ... Command completion: measured\\nok", flush=True)
    else:
        print("test " + name + " ... " + status, flush=True)
        if name == "diagnostic" and "--show-output" in sys.argv:
            print("\\n---- diagnostic stdout ----\\nCommand completion: measured", flush=True)
sys.exit(code)
''')
        self.executable.chmod(0o755)
        self.companion = self.root / "target/debug/example-cli"
        self.companion.write_text("#!/bin/sh\nexit 0\n")
        self.companion.chmod(0o755)
        self.target = dict(package="example", target_kind="lib", target="example",
                           cwd="backend/crates/example",
                           runtime_env={"CARGO_MANIFEST_DIR": str(self.cwd),
                                        "CARGO_PKG_NAME": "example"})
        self.cases = [dict(self.target, test=name, ignored=name == "diagnostic")
                      for name in ("unit", "external", "diagnostic")]
        runner.classify(self.cases, self.rules)
        self.artifacts = {str(self.executable): self.target}
        self.bundle = self.root / "target/tests.tar"
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=CI test",
                        "-c", "user.email=ci@example.test", "commit", "--quiet", "-m", "fixture"], check=True)
        self.summary = {}
        runner.export_prepared(self.bundle, self.cases, self.artifacts,
                               {str(self.companion)}, self.summary)
        self.harness_log = self.root / "target/harness.jsonl"
        self.calls = self.root / "target/calls"
        self.bin = self.root / "target/bin"
        self.bin.mkdir()
        (self.bin / "cargo").write_text('#!/bin/sh\necho cargo >> "$RUNNER_CALLS"\nexit 99\n')
        (self.bin / "psql").write_text('''#!/bin/sh
case "$*" in
  *"SELECT 1"*) echo preflight >> "$RUNNER_CALLS";;
  *) echo cleanup >> "$RUNNER_CALLS"; cat >/dev/null;;
esac
''')
        (self.bin / "objcopy").write_text('''#!/usr/bin/env python3
import os, pathlib, sys
if sys.argv[1:] == ["--version"]:
    print(os.environ.get("TEST_OBJCOPY_VERSION", "GNU objcopy (fixture)"))
    sys.exit(0)
assert len(sys.argv) == 4 and sys.argv[1] == "--strip-debug"
source, output = map(pathlib.Path, sys.argv[2:])
assert not output.exists(), "Only one temporary payload may exist"
if os.environ.get("TEST_OBJCOPY_FAILURE"):
    sys.exit(17)
output.write_bytes(b"\\n".join(line for line in source.read_bytes().splitlines()
                             if not line.startswith(b"# DEBUG-PADDING")) + b"\\n")
''')
        for command in self.bin.iterdir():
            command.chmod(0o755)

    def consume(self, mode, *arguments, **extra_env):
        env = {name: value for name, value in os.environ.items() if not name.startswith("NOTEGATE_TEST_")}
        env.update(PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                   HARNESS_LOG=str(self.harness_log), RUNNER_CALLS=str(self.calls), **extra_env)
        return subprocess.run([sys.executable, str(self.script), mode, "--prepared", str(self.bundle),
                               "--output", str(self.root / "target/results" / mode), *arguments],
                              env=env, cwd=self.root, text=True, capture_output=True, check=False)

    def records(self):
        return [json.loads(line) for line in self.harness_log.read_text().splitlines()]

    def test_bundle_restores_companion_executable_and_exact_inventory(self):
        self.executable.unlink()
        self.companion.unlink()
        cases, artifacts = runner.load_prepared(self.bundle, {})
        self.assertEqual(cases, self.cases)
        self.assertEqual(artifacts, self.artifacts)
        self.assertTrue(os.access(self.companion, os.X_OK))
        self.assertTrue(os.access(self.executable, os.X_OK))
        self.assertEqual(self.summary["bundle"]["executable_count"], 2)

    def test_internal_driver_requires_valid_environment_before_execution(self):
        for group in ("", "unexpected-group"):
            completed = self.consume("_execute", CI_RUST_PREPARED_PATH=str(self.bundle),
                                     CI_RUST_OUTPUT_PATH=str(self.root / "target/driver"),
                                     CI_RUST_GROUP=group)
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("requires valid CI_RUST_* environment", completed.stderr)
            self.assertFalse(self.harness_log.exists())

    def test_prepared_paths_reject_outside_sibling_traversal_and_symlink_escapes(self):
        escape = self.root / "target/escape"
        escape.symlink_to(self.root / "backend", target_is_directory=True)
        invalid = (self.root / "outside", self.root / "target-evil/file",
                   self.root / "target/../outside", escape / "file")
        for path in invalid:
            with self.subTest(path=path):
                with self.assertRaisesRegex(ValueError, "Prepared paths"):
                    runner.target_tree_path(path)
                for option in ("--prepared", "--output"):
                    completed = self.consume("self-contained", option, str(path))
                    self.assertNotEqual(completed.returncode, 0)
                    self.assertIn("Prepared paths", completed.stderr)
                for variable in ("CI_RUST_PREPARED_PATH", "CI_RUST_OUTPUT_PATH"):
                    values = dict(CI_RUST_PREPARED_PATH=str(self.bundle),
                                  CI_RUST_OUTPUT_PATH=str(self.root / "target/driver"),
                                  CI_RUST_GROUP="self-contained")
                    values[variable] = str(path)
                    completed = self.consume("_execute", **values)
                    self.assertNotEqual(completed.returncode, 0)
                    self.assertIn("Prepared paths", completed.stderr)
                self.assertFalse(self.harness_log.exists())
                self.assertFalse(path.exists())

    def test_target_directory_symlink_cannot_redefine_allowed_root(self):
        (self.root / "target").rename(self.root / "saved-target")
        outside = self.root / "outside-target"
        outside.mkdir()
        (self.root / "target").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "Prepared paths"):
            runner.target_tree_path(self.root / "target/report")
        self.assertFalse((outside / "report").exists())

    def test_legacy_nonprepared_output_path_remains_configurable(self):
        output = self.root / "legacy-output"
        with patch.object(sys, "argv", [str(self.script), "self-contained", "--output", str(output)]), \
                patch.object(runner, "prepare", return_value=(self.cases, self.artifacts, set())), \
                patch.object(runner, "run_group"), \
                patch.object(runner, "logged", return_value=(0, .1, [])):
            self.assertEqual(runner.main(), 0)
        self.assertTrue((output / "summary.json").is_file())

    def test_cli_modes_dispatch_requested_resource_groups_once(self):
        for mode, expected in (("self-contained", ("self-contained",)),
                               ("external", ("external",)), ("split", runner.GROUPS)):
            with self.subTest(mode=mode), \
                    patch.object(sys, "argv", [str(self.script), mode, "--output",
                                               str(self.root / "target/routing" / mode)]), \
                    patch.dict(os.environ, NOTEGATE_TEST_DATABASE_URL="postgres://example",
                               NOTEGATE_TEST_S3_ENDPOINT="http://s3"), \
                    patch.object(runner, "prepare", return_value=(self.cases, self.artifacts, set())), \
                    patch.object(runner, "run_group") as run_group, \
                    patch.object(runner, "logged", return_value=(0, .1, [])):
                self.assertEqual(runner.main(), 0)
                self.assertEqual([(call.args[0], call.args[1]) for call in run_group.call_args_list],
                                 [(group, group) for group in expected])

    def test_prepared_split_retains_baseline_active_and_ignored_coverage_without_cargo(self):
        results = []
        for mode in runner.GROUPS:
            completed = self.consume(mode, NOTEGATE_TEST_DATABASE_URL="postgres://example",
                                     NOTEGATE_TEST_S3_ENDPOINT="http://s3", NOTEGATE_TEST_RUN_ID="inherited",
                                     NOTEGATE_TEST_SKIP_INTEGRATION="1")
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
            summary = json.loads((self.root / "target/results" / mode / "summary.json").read_text())
            results.extend(summary["runs"][0]["results"])
        self.assertEqual({runner.test_id(case) for case in results},
                         {runner.test_id(case) for case in self.cases})
        self.assertEqual(len(results), len(self.cases))
        self.assertEqual(self.calls.read_text().splitlines(), ["preflight", "cleanup"])
        self_contained, external = self.records()
        self.assertEqual(self_contained["cwd"], str(self.cwd))
        self.assertEqual(self_contained["env"]["CARGO_PKG_NAME"], "example")
        self.assertNotIn("NOTEGATE_TEST_DATABASE_URL", self_contained["env"])
        self.assertNotIn("NOTEGATE_TEST_S3_ENDPOINT", self_contained["env"])
        self.assertNotIn("NOTEGATE_TEST_RUN_ID", self_contained["env"])
        self.assertNotIn("NOTEGATE_TEST_SKIP_INTEGRATION", external["env"])
        self.assertRegex(external["env"]["NOTEGATE_TEST_RUN_ID"], r"^[0-9a-f]{16}$")

    def test_external_harness_failure_still_cleans_up_without_cargo(self):
        completed = self.consume("external", NOTEGATE_TEST_DATABASE_URL="postgres://example",
                                 NOTEGATE_TEST_S3_ENDPOINT="http://s3", FAIL_EXTERNAL="1")
        self.assertEqual(completed.returncode, 1, completed.stdout + completed.stderr)
        summary = json.loads((self.root / "target/results/external/summary.json").read_text())
        self.assertEqual(summary["runs"][0]["exit_code"], 23)
        self.assertEqual(self.calls.read_text().splitlines(), ["preflight", "cleanup"])

    def test_exact_runs_real_test_with_inherited_env_and_propagates_failure(self):
        completed = self.consume("exact", "--package", "example", "--target-kind", "lib",
                                 "--target", "example", "--test-name", "external", "--", "--nocapture",
                                 NOTEGATE_TEST_SKIP_INTEGRATION="inherited")
        self.assertEqual(completed.returncode, 101, completed.stdout + completed.stderr)
        record, = self.records()
        self.assertEqual(record["names"], ["external"])
        self.assertEqual(record["env"]["NOTEGATE_TEST_SKIP_INTEGRATION"], "inherited")
        self.assertIn("--show-output", record["args"])
        self.assertNotIn("--nocapture", record["args"])
        self.assertFalse(self.calls.exists())

    def test_unknown_exact_identity_fails_before_execution(self):
        completed = self.consume("exact", "--package", "example", "--target-kind", "lib",
                                 "--target", "example", "--test-name", "missing")
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("Exact test identity not found", completed.stderr)
        self.assertFalse(self.harness_log.exists())

    def test_exact_executes_selected_ignored_measurement(self):
        completed = self.consume("exact", "--package", "example", "--target-kind", "lib",
                                 "--target", "example", "--test-name", "diagnostic", "--",
                                 "--ignored", "--nocapture", "--test-threads=1")
        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
        record, = self.records()
        self.assertEqual(record["names"], ["diagnostic"])
        self.assertIn("test diagnostic ... ok", completed.stdout)
        self.assertIn("Command completion: measured", completed.stdout)
        self.assertIn("--show-output", record["args"])
        self.assertNotIn("--nocapture", record["args"])
        self.assertFalse(self.calls.exists())

    def test_missing_inventory_identity_is_rejected_against_real_harness(self):
        runner.export_prepared(self.bundle, self.cases[1:], self.artifacts,
                               {str(self.companion)}, {})
        completed = self.consume("self-contained")
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("test inventory mismatch", completed.stderr)
        self.assertFalse(self.harness_log.exists())

    def test_exact_rejects_unsupported_libtest_arguments_before_execution(self):
        for argument in ("--skip", "--test-threads=2", "--format=json", "unexpected-filter"):
            with self.subTest(argument=argument):
                completed = self.consume("exact", "--package", "example", "--target-kind", "lib",
                                         "--target", "example", "--test-name", "unit", "--", argument)
                self.assertNotEqual(completed.returncode, 0)
                self.assertIn("Unsupported exact libtest argument", completed.stderr)
                self.assertFalse(self.harness_log.exists())

    def test_source_and_environment_mismatch_fail_before_execution(self):
        completed = self.consume("self-contained", GITHUB_RUN_ID="different-run")
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("provenance mismatch", completed.stderr)
        (self.cwd / "Cargo.toml").write_text("changed source\n")
        completed = self.consume("self-contained")
        self.assertNotEqual(completed.returncode, 0)
        self.assertFalse(self.harness_log.exists())

    def test_corrupt_executable_is_rejected_before_execution(self):
        self.executable.unlink()
        with self.bundle.open("r+b") as archive:
            with tarfile.open(fileobj=archive, mode="r:") as contents:
                member = contents.getmember(str(self.executable.relative_to(self.root)))
            archive.seek(member.offset_data)
            archive.write(b"!")
        completed = self.consume("self-contained")
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("digest mismatch", completed.stderr)
        self.assertFalse(self.harness_log.exists())

    def test_api_bundle_restores_only_fixed_executables_and_rejects_test_bundle(self):
        for name in runner.API_EXECUTABLES:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.companion, path)
        api_bundle = self.root / "target/api.tar"
        runner.pack_bundle(api_bundle, "api", runner.API_EXECUTABLES, {})
        for name in runner.API_EXECUTABLES:
            (self.root / name).unlink()
        runner.restore_bundle(api_bundle, "api", {})
        self.assertTrue(all(os.access(self.root / name, os.X_OK) for name in runner.API_EXECUTABLES))
        with self.assertRaisesRegex(ValueError, "Unsupported prepared bundle"):
            runner.restore_bundle(self.bundle, "api", {})

    def test_archive_paths_cannot_escape_target_directory(self):
        for name in ("../outside", "backend/crates/example/file", "/tmp/absolute"):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "Invalid prepared executable path"):
                runner.bundle_path(name)

    def test_strip_debug_packages_copies_preserves_originals_and_restores_runnable_payload(self):
        originals = {}
        for source in (self.executable, self.companion):
            with source.open("ab") as binary:
                binary.write(b"\n# DEBUG-PADDING " + b"x" * 32768 + b"\n")
            originals[source] = (source.read_bytes(), source.stat().st_mode)
        expected_provenance = runner.provenance()
        summary = {}
        with patch.dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"]), \
                patch.object(runner.platform, "system", return_value="Linux"), \
                patch.object(runner, "provenance", return_value=expected_provenance):
            runner.export_prepared(self.bundle, self.cases, self.artifacts,
                                   {str(self.companion)}, summary, strip_debug=True)
        self.assertTrue(summary["bundle"]["strip_debug"])
        self.assertGreater(summary["bundle"]["original_executable_bytes"],
                           summary["bundle"]["executable_bytes"])
        self.assertEqual(summary["bundle"]["original_executable_bytes"],
                         sum(len(data) for data, _mode in originals.values()))
        for source, (data, mode) in originals.items():
            self.assertEqual((source.read_bytes(), source.stat().st_mode), (data, mode))
            source.unlink()
        with tarfile.open(self.bundle) as archive:
            self.assertEqual(archive.getnames()[-1], runner.BUNDLE_MANIFEST)
        completed = self.consume("self-contained")
        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
        self.assertNotIn(b"DEBUG-PADDING", self.executable.read_bytes())
        self.assertTrue(os.access(self.companion, os.X_OK))

    def test_strip_debug_failure_does_not_publish_bundle_or_modify_cargo_outputs(self):
        original = self.executable.read_bytes()
        failed = self.root / "target/failed.tar"
        with patch.dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        TEST_OBJCOPY_FAILURE="1"), \
                patch.object(runner.platform, "system", return_value="Linux"):
            with self.assertRaises(subprocess.CalledProcessError):
                runner.export_prepared(failed, self.cases, self.artifacts, set(), {}, strip_debug=True)
        self.assertEqual(self.executable.read_bytes(), original)
        self.assertFalse(failed.exists())
        self.assertFalse(failed.with_name(failed.name + ".pending").exists())

    def test_strip_debug_requires_linux_and_gnu_objcopy(self):
        with patch.object(runner.platform, "system", return_value="Darwin"):
            with self.assertRaisesRegex(ValueError, "requires Linux"):
                runner.pack_bundle(self.bundle, "tests", [], {}, strip_debug=True)
        with patch.dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        TEST_OBJCOPY_VERSION="llvm-objcopy"), \
                patch.object(runner.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(ValueError, "requires GNU objcopy"):
                runner.pack_bundle(self.bundle, "tests", [], {}, strip_debug=True)


if __name__ == "__main__":
    unittest.main()
