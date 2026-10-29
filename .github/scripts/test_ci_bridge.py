"""Synthetic regressions for metadata-only public CI authorization."""
import copy
import io
import json
import os
import unittest
from unittest.mock import Mock, patch
from urllib.error import HTTPError
import zipfile

import ci_bridge as bridge

HEAD, MERGE, BASE, CONTROL, PUBLIC_HEAD, TREE = (letter * 40 for letter in "abcdef")


class Fixture:
    def __init__(self, event="push", kind="ci"):
        self.kind = kind
        self.repositories = {
            bridge.CORE: {"id": bridge.CORE_ID, "full_name": bridge.CORE, "private": False},
            bridge.PUBLIC: {"id": bridge.PUBLIC_ID, "full_name": bridge.PUBLIC, "private": False},
        }
        self.source = {
            "id": 10, "run_number": 10, "run_attempt": 1, "status": "in_progress",
            "workflow_id": bridge.WORKFLOWS[kind], "path": f".github/workflows/{kind}.yml",
            "event": event, "head_sha": HEAD, "head_branch": "main",
            "head_repository": {"id": bridge.CORE_ID}, "pull_requests": [],
        }
        self.pull = {
            "number": 7, "state": "open", "merged": False, "mergeable": True,
            "merge_commit_sha": MERGE, "changed_files": 1,
            "base": {"sha": BASE, "ref": "main", "repo": {"id": bridge.CORE_ID}},
            "head": {"sha": HEAD, "ref": "change", "repo": {"id": bridge.CORE_ID}},
        }
        if event == "pull_request":
            self.source["head_branch"] = "change"
            self.source["pull_requests"] = [{
                "number": 7, "head": {"sha": HEAD}, "base": {"sha": BASE},
            }]
        self.runs = [self.source]
        self.parents = [BASE, HEAD]
        self.files = [{"filename": "unknown.rs"}]
        self.trees = {CONTROL: TREE, PUBLIC_HEAD: TREE}
        self.public_workflow = {"id": 100, "path": f".github/workflows/{kind}.yml"}
        self.request = self.plan()
        self.public = {
            "id": 20, "run_attempt": 1, "status": "completed", "conclusion": "success",
            "workflow_id": 100, "path": self.public_workflow["path"], "event": "workflow_dispatch",
            "head_sha": PUBLIC_HEAD, "head_branch": "main",
            "head_repository": {"id": bridge.PUBLIC_ID},
            "actor": {"id": 349623475}, "triggering_actor": {"id": 349623475},
            "display_title": bridge.title(self.request),
        }
        self.public_rows = [self.public]
        self.jobs = self.make_jobs()
        self.artifacts = []
        self.calls = []

    def plan(self, options=None):
        return bridge.source_plan(10, 1, self.kind, options, CONTROL, self.read)

    def make_jobs(self):
        expected = {"gate": ("Validate core request", True),
                    **bridge.expected_jobs(self.request, bridge.selection(self.request, self.read))}
        return [{"name": name, "run_id": 20, "run_attempt": 1, "head_sha": PUBLIC_HEAD,
                 "status": "completed", "conclusion": "success" if enabled else "skipped"}
                for name, enabled in expected.values()]

    def read(self, path, data=None, token=None):
        if hasattr(self, "calls"):
            self.calls.append((path, data, token))
        core, public = f"repos/{bridge.CORE}", f"repos/{bridge.PUBLIC}"
        if path in (core, public):
            return self.repositories[path.removeprefix("repos/")]
        if "/compare/" in path:
            sha = path.split("/compare/")[1].split("...")[0]
            return {"status": "identical", "merge_base_commit": {"sha": sha}}
        if path.startswith(f"{core}/git/commits/"):
            return {"parents": [{"sha": sha} for sha in self.parents]}
        if path == f"{core}/pulls/7":
            return self.pull
        if path.startswith(f"{core}/pulls/7/files?"):
            return self.files
        if path.startswith(f"{core}/actions/workflows/"):
            return {"total_count": len(self.runs), "workflow_runs": self.runs}
        if path == f"{core}/actions/runs/10":
            return self.source
        if path == f"{core}/actions/runs/10/artifacts?per_page=100":
            return {"total_count": len(self.artifacts), "artifacts": self.artifacts}
        if path == f"{public}/commits/main":
            return {"sha": PUBLIC_HEAD}
        if path.startswith(f"{public}/git/trees/"):
            return {"tree": [{"path": ".github", "type": "tree", "sha": self.trees[path.rsplit("/", 1)[1]]}]}
        if path.endswith("/dispatches"):
            return None
        if path.startswith(f"{public}/actions/workflows/100/runs?"):
            return {"workflow_runs": self.public_rows}
        if path.startswith(f"{public}/actions/workflows/"):
            return self.public_workflow
        if path == f"{public}/actions/runs/20":
            return self.public
        if path.startswith(f"{public}/actions/runs/20/attempts/"):
            return {"total_count": len(self.jobs), "jobs": self.jobs}
        raise AssertionError("Unexpected fixture API path: " + path)


class BridgeTests(unittest.TestCase):
    def test_prepare_waits_only_for_queued_source_to_start(self):
        fixture = Fixture()
        queued = {**fixture.source, "status": "queued"}
        reader = Mock(side_effect=[queued, queued, fixture.source])
        pause = Mock()
        bridge.wait_for_source_start(10, 1, reader, pause)
        self.assertEqual(reader.call_count, 3)
        self.assertEqual(pause.call_count, 2)
        self.assertEqual(bridge.validate_request(fixture.request, fixture.read), fixture.request)

    def test_prepare_start_wait_is_bounded_and_rejects_changed_or_terminal_runs(self):
        fixture = Fixture()
        reader = Mock(return_value={**fixture.source, "status": "queued"})
        pause = Mock()
        with self.assertRaisesRegex(bridge.Stopped, "prepare deadline"):
            bridge.wait_for_source_start(10, 1, reader, pause)
        self.assertEqual(reader.call_count, 6)
        self.assertEqual(pause.call_count, 5)
        for change in ({"id": 11}, {"run_attempt": 2}, {"status": "completed"},
                       {"status": "cancelled"}):
            with self.subTest(change=change):
                reader = Mock(return_value={**fixture.source, **change})
                pause = Mock()
                with self.assertRaises(bridge.Stopped):
                    bridge.wait_for_source_start(10, 1, reader, pause)
                self.assertEqual(reader.call_count, 1)
                pause.assert_not_called()

    def test_public_main_and_pr_plans_bind_the_actual_tested_commit(self):
        main, pull = Fixture(), Fixture("pull_request")
        self.assertEqual(main.request["tested_sha"], HEAD)
        self.assertEqual(pull.request["tested_sha"], MERGE)
        self.assertEqual(pull.request["head_sha"], HEAD)
        self.assertEqual(pull.request["base_sha"], BASE)
        self.assertEqual(pull.request["pr_number"], 7)
        self.assertEqual(bridge.validate_request(pull.request, pull.read), pull.request)

    def test_every_supported_kind_has_a_trusted_main_plan(self):
        for kind in bridge.WORKFLOWS:
            with self.subTest(kind=kind):
                event = "push" if kind == "ci" else "workflow_dispatch"
                fixture = Fixture(event, kind)
                self.assertEqual(fixture.request["kind"], kind)

    def test_wrong_repository_identity_and_private_repositories_fail_closed(self):
        for repository in (bridge.CORE, bridge.PUBLIC):
            for field, value in (("id", 1), ("full_name", "other/repository"), ("private", True)):
                with self.subTest(repository=repository, field=field):
                    fixture = Fixture()
                    fixture.repositories[repository][field] = value
                    with self.assertRaises(bridge.Stopped):
                        if repository == bridge.CORE:
                            fixture.plan()
                        else:
                            bridge.validate_control(CONTROL, PUBLIC_HEAD, fixture.read)

    def test_stale_attempt_wrong_workflow_and_terminal_source_are_rejected(self):
        for field, value in (("run_attempt", 2), ("workflow_id", 1), ("path", ".github/workflows/other.yml"),
                             ("status", "completed"), ("head_branch", "other"), ("event", "pull_request_target")):
            with self.subTest(field=field):
                fixture = Fixture()
                fixture.source[field] = value
                with self.assertRaises(bridge.Stopped):
                    fixture.plan()

    def test_newer_source_run_blocks_an_older_successful_identity(self):
        fixture = Fixture()
        fixture.runs.append({**fixture.source, "id": 11, "run_number": 11})
        with self.assertRaisesRegex(bridge.Stopped, "newer source"):
            fixture.plan()

    def test_changed_pr_head_base_or_merge_parents_are_rejected(self):
        for mutation in ("head", "base", "closed", "not_ready", "parent_order"):
            with self.subTest(mutation=mutation):
                fixture = Fixture("pull_request")
                if mutation in {"head", "base"}:
                    fixture.pull[mutation]["sha"] = "1" * 40
                elif mutation == "closed":
                    fixture.pull["state"] = "closed"
                elif mutation == "not_ready":
                    fixture.pull["mergeable"] = None
                else:
                    fixture.parents.reverse()
                with self.assertRaises(bridge.Stopped):
                    fixture.plan()

    def test_request_unknown_duplicate_fields_types_and_size_are_rejected(self):
        request = Fixture().request
        for changed in ({**request, "extra": 1}, {**request, "run_id": True},
                        {**request, "options": {}}, {**request, "tested_sha": "main"}):
            with self.subTest(changed=changed):
                with self.assertRaises(bridge.Stopped):
                    bridge.decode_request(json.dumps(changed))
        with self.assertRaises(bridge.Stopped):
            bridge.decode_request('{"kind":"ci",' + json.dumps(request)[1:])
        with self.assertRaises(bridge.Stopped):
            bridge.decode_request(" " * 16385)

    def test_performance_options_are_bounded_and_kind_specific(self):
        fixture = Fixture("workflow_dispatch", "text-write-performance")
        options = dict.fromkeys(bridge.OPTIONS, "")
        options.update(candidate_sha=BASE, base_ref="v0.1.120")
        self.assertEqual(fixture.plan(options)["tested_sha"], BASE)
        for kind, bad in (("ci", {"candidate_sha": BASE}),
                          ("browser-performance", {"base_ref": "main"}),
                          ("text-write-performance", {"base_ref": "../secret"}),
                          ("ci", {"web_artifact_run_id": "10"})):
            options = {**dict.fromkeys(bridge.OPTIONS, ""), **bad}
            with self.subTest(kind=kind, options=options), self.assertRaises(bridge.Stopped):
                bridge.normalized_options(kind, options)

    def test_api_allows_compare_dots_but_rejects_path_traversal(self):
        with patch.object(bridge, "urlopen", return_value=io.BytesIO(b'{"status":"identical"}')) as opened:
            self.assertEqual(bridge.api(f"repos/{bridge.CORE}/compare/{HEAD}...main")["status"], "identical")
            self.assertIn("...main", opened.call_args.args[0].full_url)
        for path in ("https://evil.invalid/", "/repos/a/b", "repos/a/../secret", "repos/./a"):
            with self.subTest(path=path), self.assertRaises(bridge.Stopped):
                bridge.api(path)

    def test_api_error_does_not_expose_response_body_or_credential(self):
        failure = HTTPError("https://api.github.com/metadata", 403, "sensitive-response", {}, io.BytesIO(b"private-body"))
        with patch.object(bridge, "urlopen", side_effect=failure):
            with self.assertRaisesRegex(bridge.Stopped, r"^GitHub request failed \(HTTP 403\)$"):
                bridge.api("repos/a/b", token="test-only-credential")

    def test_control_tree_allows_readme_only_updates_and_rejects_definition_change(self):
        fixture = Fixture()
        bridge.validate_control(CONTROL, PUBLIC_HEAD, fixture.read)
        fixture.trees[PUBLIC_HEAD] = "1" * 40
        with self.assertRaisesRegex(bridge.Stopped, "definition differs"):
            bridge.validate_control(CONTROL, PUBLIC_HEAD, fixture.read)

    def test_duplicate_execution_wrong_actor_workflow_or_branch_is_rejected(self):
        for field, value in (("actor", {"id": 1}), ("triggering_actor", {"id": 1}),
                             ("workflow_id", 1), ("head_branch", "other"),
                             ("head_repository", {"id": 1}), ("path", ".github/workflows/other.yml")):
            with self.subTest(field=field):
                fixture = Fixture()
                fixture.public[field] = value
                with self.assertRaises(bridge.Stopped):
                    bridge.public_run(fixture.request, fixture.read)
        fixture = Fixture()
        fixture.public_rows.append(copy.deepcopy(fixture.public))
        with self.assertRaisesRegex(bridge.Stopped, "Duplicate"):
            bridge.public_run(fixture.request, fixture.read)

    def test_gate_requires_its_own_active_latest_attempt(self):
        fixture = Fixture()
        fixture.public["status"] = "in_progress"
        environment = {"GITHUB_RUN_ID": "20", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_SHA": PUBLIC_HEAD}
        with patch.dict(os.environ, environment):
            self.assertEqual(bridge.validate_gate_execution(fixture.request, fixture.read)["id"], 20)
            fixture.public["run_attempt"] = 2
            with self.assertRaises(bridge.Stopped):
                bridge.validate_gate_execution(fixture.request, fixture.read)

    def test_successful_native_results_require_all_jobs_and_gate(self):
        fixture = Fixture()
        results = bridge.verify_results(fixture.request, fixture.public, fixture.read)
        self.assertEqual(results["web"], "success")
        self.assertEqual(results["image"], "skipped")
        fixture.jobs = [job for job in fixture.jobs if job["name"] != "Validate core request"]
        with self.assertRaisesRegex(bridge.Stopped, "Missing"):
            bridge.verify_results(fixture.request, fixture.public, fixture.read)

    def test_required_job_skip_duplicate_and_wrong_attempt_are_rejected(self):
        for mutation in ("skip", "duplicate", "attempt", "run", "sha"):
            with self.subTest(mutation=mutation):
                fixture = Fixture()
                web = next(job for job in fixture.jobs if job["name"] == "Web")
                if mutation == "skip":
                    web["conclusion"] = "skipped"
                elif mutation == "duplicate":
                    fixture.jobs.append(copy.deepcopy(web))
                else:
                    web[{"attempt": "run_attempt", "run": "run_id", "sha": "head_sha"}[mutation]] = 99
                with self.assertRaises(bridge.Stopped):
                    bridge.verify_results(fixture.request, fixture.public, fixture.read)

    def test_docs_skip_is_computed_from_api_paths_and_partial_lists_retain_full_checks(self):
        fixture = Fixture("pull_request")
        fixture.files = [{"filename": "docs/example.md"}]
        fixture.jobs = fixture.make_jobs()
        self.assertEqual(bridge.verify_results(fixture.request, fixture.public, fixture.read)["web"], "skipped")
        fixture.pull["changed_files"] = 2
        fixture.files[0]["previous_filename"] = "docs/old.md"
        self.assertTrue(bridge.selection(fixture.request, fixture.read)["web"])

    def test_failed_cancelled_or_in_progress_public_run_is_never_success(self):
        for status, conclusion in (("completed", "failure"), ("completed", "cancelled"), ("in_progress", None)):
            with self.subTest(status=status, conclusion=conclusion):
                fixture = Fixture()
                fixture.public.update(status=status, conclusion=conclusion)
                with self.assertRaises(bridge.Stopped):
                    bridge.verify_results(fixture.request, fixture.public, fixture.read)

    def test_public_rerun_during_job_verification_is_rejected(self):
        fixture = Fixture()
        original = fixture.read
        def changed(path, *args, **kwargs):
            result = original(path, *args, **kwargs)
            if path == f"repos/{bridge.PUBLIC}/actions/runs/20":
                return {**result, "run_attempt": 2}
            return result
        with self.assertRaisesRegex(bridge.Stopped, "rerun"):
            bridge.verify_results(fixture.request, fixture.public, changed)

    def test_source_cancellation_and_wait_deadline_stop_the_bridge(self):
        fixture = Fixture()
        fixture.source["status"] = "completed"
        with self.assertRaises(bridge.Stopped):
            bridge.wait(fixture.request, fixture.read, pause=Mock())
        fixture = Fixture()
        with self.assertRaisesRegex(bridge.Stopped, "deadline"):
            bridge.wait(fixture.request, fixture.read, clock=Mock(side_effect=[0, 2]), timeout=1, pause=Mock())

    def test_wait_pins_public_run_and_keeps_poll_api_cost_bounded(self):
        fixture = Fixture()
        fixture.public.update(status="in_progress", conclusion=None)
        polls = []
        def pause(_):
            polls.append(True)
            if len(polls) == 10:
                fixture.public.update(status="completed", conclusion="success")
        result = bridge.wait(fixture.request, fixture.read, pause=pause)
        self.assertEqual(result["web"], "success")
        histories = [path for path, _, _ in fixture.calls if "/actions/workflows/100/runs?" in path]
        self.assertEqual(len(histories), 2)  # Discovery and final provenance check.
        self.assertLessEqual(len(fixture.calls), 55)

    def test_lightweight_poll_rejects_a_changed_pull_request(self):
        fixture = Fixture("pull_request")
        fixture.pull["head"]["sha"] = BASE
        with self.assertRaisesRegex(bridge.Stopped, "changed during"):
            bridge.active_source(fixture.request, fixture.read)

    def test_wait_emits_only_verified_conclusions_and_public_url(self):
        fixture = Fixture()
        result = bridge.wait(fixture.request, fixture.read, pause=Mock())
        self.assertEqual(result["public_run_url"], f"https://github.com/{bridge.PUBLIC}/actions/runs/20")
        self.assertTrue(all(value in {"success", "skipped"} for key, value in result.items() if key != "public_run_url"))

    def test_dispatch_revalidates_before_sending_only_metadata(self):
        fixture = Fixture()
        fixture.public_rows = []
        with patch.dict(os.environ, {"DISPATCH_TOKEN": "test-dispatch-only"}):
            bridge.dispatch(fixture.request, fixture.read)
        path, body, token = fixture.calls[-1]
        self.assertTrue(path.endswith("/ci.yml/dispatches"))
        self.assertEqual(set(body), {"ref", "inputs"})
        self.assertEqual(json.loads(body["inputs"]["request"]), fixture.request)
        self.assertEqual(token, "test-dispatch-only")

    def test_dispatcher_revalidates_bounded_artifact_before_token_mint(self):
        fixture = Fixture()
        fixture.artifacts = [{"id": 30, "name": "public-ci-request-ci-1", "expired": False, "size_in_bytes": 1000}]
        environment = {"GH_TOKEN": "test-read-only", "PUBLIC_CI_REVISION": CONTROL}
        loader = Mock(return_value=fixture.request)
        with patch.dict(os.environ, environment):
            self.assertEqual(bridge.dispatcher_request(10, 1, fixture.read, loader, Mock()), fixture.request)
        loader.assert_called_once_with(30, "test-read-only")
        fixture.artifacts[0]["size_in_bytes"] = 65537
        with patch.dict(os.environ, environment), self.assertRaises(bridge.Stopped):
            bridge.dispatcher_request(10, 1, fixture.read, loader, Mock())

    def test_missing_or_wrong_execution_artifact_is_rejected(self):
        fixture = Fixture()
        with self.assertRaisesRegex(bridge.Stopped, "not published"):
            bridge.dispatcher_request(10, 1, fixture.read, Mock(), Mock())
        fixture.artifacts = [{"id": 30, "name": "public-ci-request-ci-1", "expired": False, "size_in_bytes": 1000}]
        with patch.dict(os.environ, {"GH_TOKEN": "test-read-only", "PUBLIC_CI_REVISION": CONTROL}):
            with self.assertRaises(bridge.Stopped):
                bridge.dispatcher_request(10, 1, fixture.read, Mock(return_value={**fixture.request, "run_id": 11}), Mock())

    def test_frontend_producer_checks_gate_web_and_latest_attempt(self):
        fixture = Fixture()
        fixture.source.update(status="completed", conclusion="success")
        request = {**fixture.request, "kind": "browser-performance",
                   "options": {**fixture.request["options"], "web_artifact_run_id": "10"}}
        self.assertEqual(bridge.producer_run(request, fixture.read), "20")
        fixture.jobs = [job for job in fixture.jobs if job["name"] != "Validate core request"]
        with self.assertRaises(bridge.Stopped):
            bridge.producer_run(request, fixture.read)

    def test_frontend_producer_rejects_old_core_ids_or_wrong_source_sha(self):
        fixture = Fixture()
        fixture.source.update(status="completed", conclusion="success")
        request = {**fixture.request, "kind": "browser-performance",
                   "options": {**fixture.request["options"], "web_artifact_run_id": "10"}}
        fixture.source["head_sha"] = BASE
        with self.assertRaises(bridge.Stopped):
            bridge.producer_run(request, fixture.read)


class ArtifactTests(unittest.TestCase):
    def archive(self, names=None):
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as bundle:
            for name, data in (names or [("public-ci-request.json", json.dumps(Fixture().request))]):
                bundle.writestr(name, data)
        return output.getvalue()

    def opener(self, archive, location="https://example.blob.core.windows.net/request?sig=test-only"):
        redirect = HTTPError("https://api.github.com/request", 302, "", {"Location": location}, None)
        opener = Mock()
        opener.open.side_effect = [redirect, io.BytesIO(archive)]
        return opener

    def test_artifact_is_bounded_and_credentials_do_not_follow_redirect(self):
        opener = self.opener(self.archive())
        with patch.object(bridge, "build_opener", return_value=opener):
            self.assertEqual(bridge.artifact_request(30, "test-read-only"), Fixture().request)
        self.assertEqual(opener.open.call_args_list[0].args[0].get_header("Authorization"), "Bearer test-read-only")
        self.assertIsNone(opener.open.call_args_list[1].args[0].get_header("Authorization"))

    def test_zip_paths_multiple_members_expansion_and_download_size_are_rejected(self):
        archives = [
            self.archive([("../public-ci-request.json", "{}")]),
            self.archive([("public-ci-request.json", "{}"), ("extra", "")]),
            self.archive([("public-ci-request.json", " " * 16385)]),
            b"x" * 65537,
        ]
        for archive in archives:
            with self.subTest(size=len(archive)):
                with patch.object(bridge, "build_opener", return_value=self.opener(archive)):
                    with self.assertRaises(bridge.Stopped):
                        bridge.artifact_request(30, "test-read-only")

    def test_unapproved_artifact_redirect_hosts_are_rejected_before_download(self):
        for location in ("https://evil.invalid/request", "http://example.blob.core.windows.net/",
                         "https://user@example.blob.core.windows.net/request"):
            opener = self.opener(self.archive(), location)
            with self.subTest(location=location), patch.object(bridge, "build_opener", return_value=opener):
                with self.assertRaises(bridge.Stopped):
                    bridge.artifact_request(30, "test-read-only")
            self.assertEqual(opener.open.call_count, 1)


if __name__ == "__main__":
    unittest.main()
