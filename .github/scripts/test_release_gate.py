import base64
import copy
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import release_gate as gate


SOURCE_SHA = "a" * 40


class Fixture:
    def __init__(self):
        self.source = {
            "id": gate.SOURCE_REPOSITORY_ID,
            "full_name": gate.SOURCE_REPOSITORY,
            "private": False,
        }
        self.distribution = {
            "id": gate.DISTRIBUTION_REPOSITORY_ID,
            "full_name": gate.DISTRIBUTION_REPOSITORY,
            "private": False,
        }
        self.sha = SOURCE_SHA
        self.version = "0.1.120"
        self.run = {
            "id": 10, "run_number": 10, "run_attempt": 1,
            "workflow_id": gate.CI_WORKFLOW_ID, "path": ".github/workflows/ci.yml",
            "event": "push", "head_branch": "main", "head_sha": SOURCE_SHA,
            "head_repository": {"id": gate.SOURCE_REPOSITORY_ID},
            "status": "completed", "conclusion": "success",
        }
        self.runs = [self.run]
        self.jobs = [{"name": name, "status": "completed", "conclusion": "success"}
                     for name in gate.REQUIRED_JOBS]
        self.jobs.append({"name": "PR Image Gate", "status": "completed", "conclusion": "skipped"})
        self.existing = None
        self.tag = None
        self.releases = []
        self.latest = {"tag_name": "v0.1.119"}

    def read(self, path, missing_ok=False):
        source = f"repos/{gate.SOURCE_REPOSITORY}"
        distribution = f"repos/{gate.DISTRIBUTION_REPOSITORY}"
        if path == source:
            return self.source
        if path == distribution:
            return self.distribution
        if path == f"{source}/commits/main":
            return {"sha": self.sha}
        if path.startswith(f"{source}/contents/VERSION?"):
            return {"encoding": "base64", "content": base64.b64encode(self.version.encode()).decode()}
        if path.startswith(f"{source}/actions/workflows/"):
            return {"workflow_runs": self.runs}
        if path.startswith(f"{source}/actions/runs/"):
            return {"total_count": len(self.jobs), "jobs": self.jobs}
        if path == f"{distribution}/releases/latest":
            return self.latest
        if path.startswith(f"{distribution}/releases/tags/"):
            return self.existing
        if path.startswith(f"{distribution}/git/ref/tags/"):
            return self.tag
        if path.startswith(f"{distribution}/releases?per_page=100&page="):
            page = int(path.rsplit("=", 1)[1])
            return self.releases[(page - 1) * 100:page * 100]
        raise AssertionError(f"Unexpected request: {path}")


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.fixture = Fixture()

    def test_success_requires_all_main_jobs_but_allows_pr_skip(self):
        result = gate.release_plan(SOURCE_SHA, True, self.fixture.read)
        self.assertEqual(result["should_release"], "true")
        self.assertEqual(result["source_sha"], SOURCE_SHA)
        self.assertEqual(result["ci_run_id"], 10)

    def test_plan_never_approves_publication_even_for_existing_version(self):
        self.fixture.existing = {"draft": False}
        result = gate.release_plan(SOURCE_SHA, False, self.fixture.read)
        self.assertEqual(result["should_release"], "false")

    def test_latest_failed_pending_or_cancelled_run_blocks_older_success(self):
        for status, conclusion in [("completed", "failure"), ("in_progress", None), ("completed", "cancelled")]:
            with self.subTest(status=status, conclusion=conclusion):
                latest = {**self.fixture.run, "run_attempt": 2, "status": status, "conclusion": conclusion}
                self.fixture.runs = [self.fixture.run, latest]
                with self.assertRaises(gate.GateError):
                    gate.validate_source(SOURCE_SHA, self.fixture.read)

    def test_missing_skipped_failed_or_duplicate_required_job_blocks_green_run(self):
        jobs = copy.deepcopy(self.fixture.jobs)
        for mode in ("missing", "skipped", "failure", "duplicate"):
            with self.subTest(mode=mode):
                self.fixture.jobs = copy.deepcopy(jobs)
                if mode == "missing":
                    self.fixture.jobs.pop(0)
                elif mode == "duplicate":
                    self.fixture.jobs.append(self.fixture.jobs[0].copy())
                else:
                    self.fixture.jobs[0]["conclusion"] = mode
                with self.assertRaises(gate.GateError):
                    gate.validate_source(SOURCE_SHA, self.fixture.read)

    def test_stale_head_and_repository_identity_changes_block(self):
        for mutation in (lambda f: setattr(f, "sha", "b" * 40),
                         lambda f: f.source.update(id=1),
                         lambda f: f.source.update(private=True),
                         lambda f: f.source.update(archived=True)):
            fixture = Fixture()
            mutation(fixture)
            with self.assertRaises(gate.GateError):
                gate.validate_source(SOURCE_SHA, fixture.read)

    def test_other_workflow_repository_event_or_commit_cannot_supply_ci(self):
        for field, value in [("workflow_id", 1), ("path", ".github/workflows/other.yml"),
                             ("event", "pull_request"), ("head_branch", "other"),
                             ("head_sha", "b" * 40), ("head_repository", {"id": 1})]:
            with self.subTest(field=field):
                fixture = Fixture()
                fixture.run[field] = value
                with self.assertRaises(gate.GateError):
                    gate.validate_source(SOURCE_SHA, fixture.read)

    def test_missing_ci_and_invalid_versions_block(self):
        self.fixture.runs = []
        with self.assertRaises(gate.GateError):
            gate.validate_source(SOURCE_SHA, self.fixture.read)
        for version in ("1.2.3\nevil=value", "01.2.3", "v1.2.3", "1.2.3-rc.1"):
            fixture = Fixture()
            fixture.version = version
            with self.assertRaises(gate.GateError):
                gate.validate_source(SOURCE_SHA, fixture.read)

    def test_existing_tag_draft_release_and_non_increasing_version_block(self):
        for mutation in (lambda f: setattr(f, "existing", {}),
                         lambda f: setattr(f, "tag", {}),
                         lambda f: setattr(f, "releases", [{"tag_name": f"v0.0.{i}"} for i in range(100)] + [{"tag_name": "v0.1.120", "draft": True}]),
                         lambda f: setattr(f, "latest", {"tag_name": "v0.1.120"}),
                         lambda f: setattr(f, "latest", {"tag_name": "v0.2.0"}),
                         lambda f: f.distribution.update(id=1)):
            fixture = Fixture()
            mutation(fixture)
            with self.assertRaises(gate.GateError):
                gate.release_plan(SOURCE_SHA, True, fixture.read)

    def test_dispatch_context_is_explicit_and_trusted(self):
        context = {"GITHUB_REPOSITORY": gate.DISTRIBUTION_REPOSITORY,
                   "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                   "SOURCE_SHA": SOURCE_SHA, "PUBLISH": "false"}
        self.assertEqual(gate.validate_context(context), (SOURCE_SHA, False))
        for field, value in [("GITHUB_REPOSITORY", "someone/fork"), ("GITHUB_REF", "refs/heads/pr"),
                             ("GITHUB_EVENT_NAME", "pull_request"), ("SOURCE_SHA", "abc123"),
                             ("SOURCE_SHA", "A" * 40), ("PUBLISH", "yes")]:
            with self.subTest(field=field):
                with self.assertRaises(gate.GateError):
                    gate.validate_context({**context, field: value})

    def test_only_real_not_found_can_be_optional(self):
        for code in (401, 403, 429, 500):
            with self.subTest(code=code), patch("urllib.request.urlopen", side_effect=HTTPError("https://api.github.com", code, "failure", {}, None)):
                with self.assertRaises(gate.GateError):
                    gate.api("repos/example/missing", missing_ok=True)
        with patch("urllib.request.urlopen", side_effect=HTTPError("https://api.github.com", 404, "missing", {}, None)):
            self.assertIsNone(gate.api("repos/example/missing", missing_ok=True))


if __name__ == "__main__":
    unittest.main()
