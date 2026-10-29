import base64
import copy
import hashlib
import io
import json
import os
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import release_gate as gate


SOURCE_SHA = "a" * 40
SOURCE_TAG = "v0.1.120"
TAG_SHA = "c" * 40


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
        self.reference = {"ref": f"refs/tags/{SOURCE_TAG}", "object": {"type": "tag", "sha": TAG_SHA}}
        self.annotation = {"sha": TAG_SHA, "tag": SOURCE_TAG, "message": "Public changes.\n",
                           "object": {"type": "commit", "sha": SOURCE_SHA}}
        self.comparison = {"status": "identical", "merge_base_commit": {"sha": SOURCE_SHA}}
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
        if path == f"{source}/git/ref/tags/{SOURCE_TAG}":
            return self.reference
        if path.startswith(f"{source}/git/tags/"):
            return self.annotation
        if path.startswith(f"{source}/compare/"):
            return self.comparison
        if path.startswith(f"{source}/contents/VERSION?"):
            return {"encoding": "base64", "content": base64.b64encode(self.version.encode()).decode()}
        if path.startswith(f"{source}/actions/workflows/"):
            return {"workflow_runs": self.runs, "total_count": len(self.runs)}
        if path == f"{source}/actions/runs/10":
            return max(self.runs, key=lambda item: item["run_attempt"])
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
        self.environment = patch.dict(os.environ, {"CORE_TOKEN": "", "SOURCE_TAG_OBJECT_SHA": "", "RELEASE_NOTES_SHA256": ""})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.fixture = Fixture()

    def validate(self, fixture=None):
        return gate.validate_source(SOURCE_SHA, SOURCE_TAG, 10, (fixture or self.fixture).read)

    def test_cli_outputs_only_metadata_without_opening_environment_paths(self):
        result = gate.release_plan(SOURCE_SHA, SOURCE_TAG, 10, False, self.fixture.read)
        output = io.StringIO()
        environment = {"GITHUB_REPOSITORY": gate.DISTRIBUTION_REPOSITORY,
                       "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                       "SOURCE_SHA": SOURCE_SHA, "SOURCE_TAG": SOURCE_TAG,
                       "SOURCE_CI_RUN_ID": "10", "PUBLISH": "false",
                       "GITHUB_OUTPUT": "/nonexistent/output", "GITHUB_STEP_SUMMARY": "/nonexistent/summary"}
        with patch.dict(os.environ, environment), patch.object(gate, "release_plan", return_value=result), \
                patch("sys.argv", ["release_gate.py"]), patch("sys.stdout", output):
            gate.main()
        self.assertIn("should_release=false", output.getvalue())
        self.assertIn(f"source_sha={SOURCE_SHA}", output.getvalue())
        self.assertNotIn("release_notes=", output.getvalue())
        self.assertNotIn(result["release_notes"], output.getvalue())

    def test_success_requires_all_main_jobs_but_allows_pr_skip(self):
        result = gate.release_plan(SOURCE_SHA, SOURCE_TAG, 10, True, self.fixture.read)
        self.assertEqual(result["should_release"], "true")
        self.assertEqual(result["source_sha"], SOURCE_SHA)
        self.assertEqual(result["ci_run_id"], 10)

    def test_plan_never_approves_publication_even_for_existing_version(self):
        self.fixture.existing = {"draft": False}
        result = gate.release_plan(SOURCE_SHA, SOURCE_TAG, 10, False, self.fixture.read)
        self.assertEqual(result["should_release"], "false")

    def test_latest_failed_pending_or_cancelled_run_blocks_older_success(self):
        for status, conclusion in [("completed", "failure"), ("in_progress", None), ("completed", "cancelled")]:
            with self.subTest(status=status, conclusion=conclusion):
                latest = {**self.fixture.run, "run_attempt": 2, "status": status, "conclusion": conclusion}
                self.fixture.runs = [self.fixture.run, latest]
                with self.assertRaises(gate.GateError):
                    self.validate()

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
                    self.validate()

    def test_main_can_advance_without_changing_the_approved_source(self):
        self.fixture.sha = "b" * 40
        self.fixture.comparison["status"] = "ahead"
        self.assertEqual(self.validate()["source_sha"], SOURCE_SHA)

    def test_nonancestor_and_repository_identity_changes_block(self):
        for mutation in (lambda f: f.comparison.update(status="diverged"),
                         lambda f: f.comparison.update(merge_base_commit={"sha": "b" * 40}),
                         lambda f: f.source.update(id=1),
                         lambda f: f.source.update(archived=True)):
            fixture = Fixture()
            mutation(fixture)
            with self.assertRaises(gate.GateError):
                self.validate(fixture)

    def test_private_core_requires_an_explicit_source_token(self):
        self.fixture.source["private"] = True
        with self.assertRaises(gate.GateError):
            self.validate()
        with patch.dict(os.environ, {"CORE_TOKEN": "source-only"}):
            self.assertEqual(self.validate()["source_sha"], SOURCE_SHA)

    def test_tag_must_be_annotated_and_identify_the_approved_version_and_commit(self):
        for mutation in (lambda f: f.reference["object"].update(type="commit"),
                         lambda f: f.reference.update(ref="refs/tags/v0.0.1"),
                         lambda f: f.reference["object"].update(sha="bad"),
                         lambda f: f.annotation.update(sha="b" * 40),
                         lambda f: f.annotation.update(tag="v0.0.1"),
                         lambda f: f.annotation["object"].update(type="tag"),
                         lambda f: f.annotation["object"].update(sha="b" * 40),
                         lambda f: setattr(f, "version", "0.1.121")):
            fixture = Fixture()
            mutation(fixture)
            with self.assertRaises(gate.GateError):
                self.validate(fixture)

    def test_public_notes_are_bounded_and_bound_to_the_tag_object(self):
        result = self.validate()
        self.assertEqual(result["release_notes"], self.fixture.annotation["message"])
        self.assertEqual(result["notes_sha256"], hashlib.sha256(b"Public changes.\n").hexdigest())
        for notes in (None, "", " \n", "x" * 16385, "한" * 5462):
            with self.subTest(notes_type=type(notes).__name__):
                self.fixture.annotation["message"] = notes
                with self.assertRaises(gate.GateError):
                    self.validate()

    def test_late_checks_reject_a_moved_tag_or_replaced_notes(self):
        for field, value in (("SOURCE_TAG_OBJECT_SHA", "b" * 40), ("RELEASE_NOTES_SHA256", "0" * 64)):
            with patch.dict(os.environ, {field: value}), self.assertRaises(gate.GateError):
                self.validate()
        with patch.dict(os.environ, {"SOURCE_TAG_OBJECT_SHA": TAG_SHA,
                                    "RELEASE_NOTES_SHA256": hashlib.sha256(b"Public changes.\n").hexdigest()}):
            self.assertEqual(self.validate()["tag_object_sha"], TAG_SHA)

    def test_delivered_run_must_be_the_latest_ci_for_the_tagged_commit(self):
        self.fixture.runs.append({**self.fixture.run, "id": 11, "run_number": 11})
        with self.assertRaises(gate.GateError):
            self.validate()

    def test_rerun_attempt_changes_and_incomplete_run_or_job_lists_block(self):
        source = f"repos/{gate.SOURCE_REPOSITORY}"
        for mode in ("attempt", "runs", "jobs"):
            with self.subTest(mode=mode):
                def read(path, missing_ok=False):
                    response = self.fixture.read(path, missing_ok)
                    if mode == "attempt" and path == f"{source}/actions/runs/10":
                        return {**response, "run_attempt": 2}
                    if mode == "runs" and "/actions/workflows/" in path:
                        return {**response, "total_count": 101}
                    if mode == "jobs" and "/attempts/" in path:
                        return {**response, "total_count": 101}
                    return response
                with self.assertRaises(gate.GateError):
                    gate.validate_source(SOURCE_SHA, SOURCE_TAG, 10, read)

    def test_other_workflow_repository_event_or_commit_cannot_supply_ci(self):
        for field, value in [("workflow_id", 1), ("path", ".github/workflows/other.yml"),
                             ("event", "pull_request"), ("head_branch", "other"),
                             ("head_sha", "b" * 40), ("head_repository", {"id": 1})]:
            with self.subTest(field=field):
                fixture = Fixture()
                fixture.run[field] = value
                with self.assertRaises(gate.GateError):
                    self.validate(fixture)

    def test_missing_ci_and_invalid_versions_block(self):
        self.fixture.runs = []
        with self.assertRaises(gate.GateError):
            self.validate()
        for version in ("1.2.3\nevil=value", "01.2.3", "v1.2.3", "1.2.3-rc.1"):
            fixture = Fixture()
            fixture.version = version
            with self.assertRaises(gate.GateError):
                self.validate(fixture)

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
                gate.release_plan(SOURCE_SHA, SOURCE_TAG, 10, True, fixture.read)

    def test_dispatch_context_is_explicit_and_trusted(self):
        context = {"GITHUB_REPOSITORY": gate.DISTRIBUTION_REPOSITORY,
                   "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch",
                   "SOURCE_SHA": SOURCE_SHA, "SOURCE_TAG": SOURCE_TAG, "SOURCE_CI_RUN_ID": "10", "PUBLISH": "false"}
        self.assertEqual(gate.validate_context(context), (SOURCE_SHA, SOURCE_TAG, 10, False))
        for field, value in [("GITHUB_REPOSITORY", "someone/fork"), ("GITHUB_REF", "refs/heads/pr"),
                             ("GITHUB_EVENT_NAME", "pull_request"), ("SOURCE_SHA", "abc123"),
                             ("SOURCE_SHA", "A" * 40), ("SOURCE_TAG", "v01.2.3"),
                             ("SOURCE_CI_RUN_ID", "0"), ("SOURCE_CI_RUN_ID", "1\nevil=1"), ("PUBLISH", "yes")]:
            with self.subTest(field=field):
                with self.assertRaises(gate.GateError):
                    gate.validate_context({**context, field: value})

    def test_source_token_is_never_sent_to_distribution_or_other_repository_paths(self):
        def respond(request, timeout):
            token = request.get_header("Authorization")
            path = request.full_url.removeprefix("https://api.github.com/")
            if path == f"repos/{gate.SOURCE_REPOSITORY}" or path.startswith(f"repos/{gate.SOURCE_REPOSITORY}/"):
                self.assertEqual(token, "Bearer source-only")
            else:
                self.assertEqual(token, "Bearer distribution-only")
            return io.BytesIO(json.dumps({}).encode())
        with patch.dict(os.environ, {"CORE_TOKEN": "source-only", "GH_TOKEN": "distribution-only"}), patch("urllib.request.urlopen", side_effect=respond):
            for path in (f"repos/{gate.SOURCE_REPOSITORY}", f"repos/{gate.SOURCE_REPOSITORY}/git/ref/tags/{SOURCE_TAG}",
                         f"repos/{gate.DISTRIBUTION_REPOSITORY}", "repos/cagojeiger/notegate-core-extra"):
                gate.api(path)

    def test_only_real_not_found_can_be_optional(self):
        for code in (401, 403, 429, 500):
            with self.subTest(code=code), patch("urllib.request.urlopen", side_effect=HTTPError("https://api.github.com", code, "failure", {}, None)):
                with self.assertRaises(gate.GateError):
                    gate.api("repos/example/missing", missing_ok=True)
        with patch("urllib.request.urlopen", side_effect=HTTPError("https://api.github.com", 404, "missing", {}, None)):
            self.assertIsNone(gate.api("repos/example/missing", missing_ok=True))


if __name__ == "__main__":
    unittest.main()
