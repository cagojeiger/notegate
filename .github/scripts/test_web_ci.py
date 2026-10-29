import base64
from pathlib import Path
import tempfile
import unittest

import web_ci as web

SHA = "a" * 40
MAIN = "b" * 40


class Fixture:
    def __init__(self):
        self.repository = {"id": web.SOURCE_REPOSITORY_ID, "full_name": web.SOURCE_REPOSITORY}
        self.run = {"id": 10, "run_number": 20, "run_attempt": 2, "workflow_id": web.CI_WORKFLOW_ID,
                    "path": ".github/workflows/ci.yml", "head_sha": SHA,
                    "head_repository": {"id": web.SOURCE_REPOSITORY_ID}, "head_branch": "main",
                    "event": "push", "status": "completed", "conclusion": "success"}
        self.runs = [self.run]
        self.jobs = [{"name": "Web", "run_id": 10, "run_attempt": 2, "head_sha": SHA,
                      "status": "completed", "conclusion": "success"}]
        self.comparison = {"status": "ahead", "merge_base_commit": {"sha": SHA}}
        self.files = {".node-version": "22.23.4\n", "package.json": '{"packageManager":"pnpm@10.29.3"}',
                      ".github/workflows/ci.yml": "jobs:\n  web:\n    steps:\n" + "".join(
                          f"      - name: Check\n        run: {command}\n" for command in web.WEB_COMMANDS)
                          + "  browser:\n    name: Browser E2E\n"}
        self.incomplete_runs = self.incomplete_jobs = False

    def call(self, path):
        core = f"repos/{web.SOURCE_REPOSITORY}"
        if path == core:
            return self.repository
        if path == f"{core}/actions/runs/10":
            return self.run
        if path.startswith(f"{core}/actions/workflows/"):
            return {"total_count": len(self.runs) + int(self.incomplete_runs), "workflow_runs": self.runs}
        if path == f"{core}/commits/main":
            return {"sha": MAIN}
        if path == f"{core}/compare/{SHA}...{MAIN}":
            return self.comparison
        if path == f"{core}/actions/runs/10/attempts/2/jobs?per_page=100":
            return {"total_count": len(self.jobs) + int(self.incomplete_jobs), "jobs": self.jobs}
        if path.startswith(f"{core}/contents/"):
            name, _, ref = path.removeprefix(f"{core}/contents/").partition("?ref=")
            assert ref == SHA
            return {"encoding": "base64", "content": base64.b64encode(self.files[name].encode()).decode()}
        raise AssertionError(path)


class WebGateTests(unittest.TestCase):
    def test_only_two_successful_web_results_are_a_successful_comparison(self):
        with tempfile.TemporaryDirectory() as directory:
            environment = {"SOURCE_SHA": SHA, "SOURCE_CI_RUN_ID": "10", "SOURCE_CI_ATTEMPT": "2",
                           "GITHUB_STEP_SUMMARY": str(Path(directory) / "summary")}
            for core in ("success", "failure", ""):
                for public in ("success", "failure", "cancelled", "skipped"):
                    for gate in ("success", "failure", "cancelled"):
                        values = {**environment, "CORE_RESULT": core, "PUBLIC_RESULT": public, "GATE_RESULT": gate}
                        with self.subTest(core=core, public=public, gate=gate):
                            if core == public == gate == "success":
                                web.compare_results(values)
                            else:
                                with self.assertRaises(web.GateError):
                                    web.compare_results(values)
            summary = Path(environment["GITHUB_STEP_SUMMARY"]).read_text()
            self.assertIn("Core Web: **failure**; public Web: **success**", summary)

    def test_shadow_is_not_blocked_by_unrelated_core_failure(self):
        fixture = Fixture()
        fixture.run["conclusion"] = "failure"
        plan = web.validate_source(SHA, "10", "2", fixture.call)
        self.assertEqual(plan["core_web_conclusion"], "success")
        fixture.jobs[0]["conclusion"] = "failure"
        self.assertEqual(web.validate_source(SHA, 10, 2, fixture.call)["core_web_conclusion"], "failure")

    def test_input_path_injection_short_sha_and_attempt_changes_are_rejected(self):
        for sha, run_id, attempt in (("main", 10, 2), (SHA.upper(), 10, 2), (SHA, "../runs", 2),
                                     (SHA, 10, "2?query"), (SHA, 10, 1), (SHA, 10, 0)):
            with self.subTest(sha=sha, run=run_id, attempt=attempt), self.assertRaises(web.GateError):
                web.validate_source(sha, run_id, attempt, Fixture().call)

    def test_repository_pr_run_and_cancellation_boundaries(self):
        for field, value in (("event", "pull_request"), ("head_branch", "other"),
                             ("head_repository", {"id": 1}), ("head_sha", MAIN), ("workflow_id", 1),
                             ("path", ".github/workflows/other.yml"), ("conclusion", "cancelled"),
                             ("status", "in_progress")):
            fixture = Fixture()
            fixture.run[field] = value
            with self.subTest(field=field), self.assertRaises(web.GateError):
                web.validate_source(SHA, 10, 2, fixture.call)
        fixture = Fixture()
        fixture.repository["id"] = 1
        with self.assertRaises(web.GateError):
            web.validate_source(SHA, 10, 2, fixture.call)

    def test_newer_attempt_incomplete_lists_and_duplicate_jobs_are_rejected(self):
        for change in (lambda f: f.runs.append({**f.run, "run_attempt": 3}),
                       lambda f: f.runs.append({**f.run, "id": 11, "run_number": 21}),
                       lambda f: setattr(f, "incomplete_runs", True),
                       lambda f: setattr(f, "incomplete_jobs", True),
                       lambda f: f.jobs.append(dict(f.jobs[0])),
                       lambda f: f.jobs[0].update(conclusion="cancelled"),
                       lambda f: f.jobs[0].update(run_attempt=1),
                       lambda f: f.comparison.update(merge_base_commit={"sha": MAIN})):
            fixture = Fixture()
            change(fixture)
            with self.subTest(change=change), self.assertRaises(web.GateError):
                web.validate_source(SHA, 10, 2, fixture.call)

    def test_tool_and_command_drift_require_an_explicit_shadow_update(self):
        for name, content in ((".node-version", "23.0.0"), ("package.json", '{"packageManager":"pnpm@11.0.0"}'),
                              (".github/workflows/ci.yml", Fixture().files[".github/workflows/ci.yml"].replace(
                                  "pnpm --filter web test", "pnpm --filter web other-test"))):
            fixture = Fixture()
            fixture.files[name] = content
            with self.subTest(name=name), self.assertRaises(web.GateError):
                web.validate_source(SHA, 10, 2, fixture.call)

    def test_execution_workflow_has_no_write_permissions_cache_or_artifacts(self):
        workflow = (Path(__file__).parents[1] / "workflows/web-ci.yml").read_text()
        execution = workflow.split("\n  web:\n", 1)[1].split("\n  compare:\n", 1)[0]
        self.assertIn("permissions: {}", execution)
        self.assertIn("persist-credentials: false", execution)
        self.assertLess(execution.index("https://api.github.com/installation/token"),
                        execution.index("pnpm/action-setup@"))
        self.assertNotIn("upload-artifact", workflow)
        self.assertIn("package-manager-cache: false", workflow)
        self.assertFalse(any(line.strip().startswith("cache:") for line in workflow.splitlines()))
        commands = [line.split("run: ", 1)[1] for line in execution.splitlines() if "run: pnpm " in line]
        self.assertEqual(commands, web.WEB_COMMANDS)
