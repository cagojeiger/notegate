#!/usr/bin/env python3
"""Bind public CI execution to one current core workflow attempt."""

import argparse
import copy
import io
import json
import os
from pathlib import Path
import random
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener
import zipfile

import ci_impact


CORE = "cagojeiger/notegate-core"
CORE_ID = 1253185495
PUBLIC = "cagojeiger/notegate"
PUBLIC_ID = 1437742487
DISPATCH_ACTORS = {349623475, 18637774}  # Release App and repository owner bootstrap.
SHA = re.compile(r"[0-9a-f]{40}")
WORKFLOWS = {
    "ci": 294404687,
    "dependency-security": 334800780,
    "image-security": 365645870,
    "security-dast": 386342590,
    "browser-performance": 384552834,
    "text-write-performance": 375133012,
}
PERFORMANCE = {"browser-performance", "text-write-performance"}
FIELDS = {"kind", "run_id", "attempt", "event", "pr_number", "head_sha", "base_sha",
          "tested_sha", "options", "control_sha"}
OPTIONS = {"candidate_sha", "base_ref", "web_artifact_run_id"}


class Stopped(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Stopped(message)


def positive(value):
    require(re.fullmatch(r"[1-9][0-9]*", str(value)) is not None, "Invalid numeric identity")
    return int(value)


def api(path, data=None, token=None, missing=False, poll_cache=None):
    require(isinstance(path, str) and not path.startswith(("/", "http"))
            and all(part not in {".", ".."} for part in path.split("?", 1)[0].split("/")),
            "Invalid GitHub API path")
    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}
    core_path = path.split("?", 1)[0]
    core_prefix = f"repos/{CORE}"
    is_core = core_path == core_prefix or core_path.startswith(core_prefix + "/")
    credential = token or (os.environ.get("CORE_TOKEN") if is_core else None) or os.environ.get("GH_TOKEN")
    if credential:
        headers["Authorization"] = "Bearer " + credential
    body = None if data is None else json.dumps(data).encode()
    if body is not None:
        headers["Content-Type"] = "application/json"
    # Only cache authenticated polling reads, and revalidate every use with GitHub.
    cache_key = (credential, path) if body is None and credential and poll_cache is not None else None
    cached = poll_cache.get(cache_key) if cache_key is not None else None
    if cached:
        headers["If-None-Match"] = cached[0]
    for attempt in range(3):
        try:
            with build_opener(NoRedirect).open(Request("https://api.github.com/" + path, data=body, headers=headers), timeout=30) as response:
                result = response.read(8 * 1024 * 1024 + 1)
                require(len(result) <= 8 * 1024 * 1024, "GitHub response exceeded its limit")
                value = json.loads(result) if result else None
                etag = getattr(response, "headers", {}).get("ETag")
                if cache_key is not None and etag:
                    poll_cache[cache_key] = (etag, copy.deepcopy(value))
                elif cache_key is not None:
                    poll_cache.pop(cache_key, None)
                return value
        except HTTPError as error:
            if error.code == 304 and cached is not None:
                return copy.deepcopy(cached[1])
            if missing and error.code == 404:
                return None
            rate_limited = error.code == 429 or (error.code == 403 and
                (error.headers.get("Retry-After") or error.headers.get("X-RateLimit-Remaining") == "0"))
            transient = error.code in {500, 502, 503, 504}
            if body is None and attempt < 2 and (rate_limited or transient):
                try:
                    delay = float(error.headers.get("Retry-After", "60" if rate_limited else str(2 ** attempt)))
                    if rate_limited and error.headers.get("X-RateLimit-Remaining") == "0":
                        delay = max(delay, float(error.headers.get("X-RateLimit-Reset", "0")) - time.time())
                except (TypeError, ValueError):
                    delay = 61
                if 0 <= delay <= 60:
                    time.sleep(delay + random.uniform(0, 0.25))
                    continue
            raise Stopped(f"GitHub request failed (HTTP {error.code})") from None
        except (URLError, TimeoutError):
            if body is None and attempt < 2:
                time.sleep(2 ** attempt + random.uniform(0, 0.25))
                continue
            raise Stopped("GitHub request could not be completed") from None


def checked_repository(name, identity, read=api):
    repository = read(f"repos/{name}")
    require(repository.get("id") == identity and repository.get("full_name") == name
            and not repository.get("archived"), "Repository identity changed")
    require(type(repository.get("private")) is bool and (name == CORE or repository["private"] is False),
            "Unexpected repository visibility")
    return repository


def main_ancestor(repository, sha, read=api):
    require(isinstance(sha, str) and SHA.fullmatch(sha), "Invalid source SHA")
    comparison = read(f"repos/{repository}/compare/{sha}...main")
    require(comparison.get("status") in {"ahead", "identical"}
            and comparison.get("merge_base_commit", {}).get("sha") == sha,
            "Commit is not on the trusted main history")


def control_tree(sha, read=api):
    require(isinstance(sha, str) and SHA.fullmatch(sha), "Invalid public control revision")
    tree = read(f"repos/{PUBLIC}/git/trees/{sha}")
    require(not tree.get("truncated"), "Incomplete public control tree")
    entries = [item for item in tree.get("tree", []) if item.get("path") == ".github"
               and item.get("type") == "tree"]
    require(len(entries) == 1 and SHA.fullmatch(entries[0].get("sha", "")), "Missing public CI control tree")
    return entries[0]["sha"]


def validate_control(revision, actual, read=api):
    checked_repository(PUBLIC, PUBLIC_ID, read)
    main_ancestor(PUBLIC, revision, read)
    main_ancestor(PUBLIC, actual, read)
    require(control_tree(revision, read) == control_tree(actual, read),
            "Public CI definition differs from the pinned control revision")


def unique_fields(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate request field")
        result[key] = value
    return result


def decode_request(text):
    require(isinstance(text, str) and len(text.encode()) <= 16384, "Invalid request size")
    request = json.loads(text, object_pairs_hook=unique_fields)
    require(isinstance(request, dict) and set(request) == FIELDS, "Unexpected request fields")
    require(request.get("kind") in WORKFLOWS, "Unknown workflow kind")
    for key in ("run_id", "attempt"):
        require(type(request.get(key)) is int and request[key] > 0, "Invalid request run or attempt")
    require(type(request.get("pr_number")) is int and request["pr_number"] >= 0, "Invalid PR identity")
    require(isinstance(request.get("options"), dict) and set(request["options"]) == OPTIONS
            and all(isinstance(value, str) for value in request["options"].values()), "Invalid request options")
    for key in ("head_sha", "tested_sha", "control_sha"):
        require(isinstance(request.get(key), str) and SHA.fullmatch(request[key]), "Invalid request commit")
    require(request.get("base_sha") == "" or isinstance(request.get("base_sha"), str)
            and SHA.fullmatch(request["base_sha"]), "Invalid base commit")
    return request


def normalized_options(kind, options):
    require(set(options) == OPTIONS, "Unexpected request options")
    candidate, baseline, producer = (options[key] for key in
                                     ("candidate_sha", "base_ref", "web_artifact_run_id"))
    require(all(isinstance(value, str) for value in options.values()), "Invalid option type")
    if candidate:
        require(kind in PERFORMANCE and SHA.fullmatch(candidate), "Candidate is restricted to performance runs")
    if baseline:
        require(kind == "text-write-performance" and len(baseline) <= 200
                and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", baseline)
                and ".." not in baseline and not baseline.endswith("/"), "Invalid performance baseline")
    if producer:
        require(kind == "browser-performance", "Unexpected frontend artifact producer")
        positive(producer)
    return options


def wait_for_source_start(run_id, attempt, read=api, pause=time.sleep, tries=6):
    run_id, attempt = positive(run_id), positive(attempt)
    for index in range(tries):
        run = read(f"repos/{CORE}/actions/runs/{run_id}")
        require(run.get("id") == run_id and run.get("run_attempt") == attempt,
                "Source workflow is no longer the active attempt")
        if run.get("status") == "in_progress":
            return
        require(run.get("status") == "queued", "Source workflow is no longer the active attempt")
        if index + 1 < tries:
            pause(5)
    raise Stopped("Source workflow did not start before the prepare deadline")


def source_plan(run_id, attempt, kind, options=None, control_sha=None, read=api):
    require(kind in WORKFLOWS, "Unknown source workflow")
    run_id, attempt = positive(run_id), positive(attempt)
    checked_repository(CORE, CORE_ID, read)
    run = read(f"repos/{CORE}/actions/runs/{run_id}")
    require(run.get("id") == run_id and run.get("workflow_id") == WORKFLOWS[kind]
            and run.get("path", "").split("@")[0] == f".github/workflows/{kind}.yml",
            "Unexpected source workflow")
    require(run.get("run_attempt") == attempt and run.get("status") == "in_progress",
            "Source workflow is no longer the active attempt")
    event = run.get("event")
    require(event in {"pull_request", "push", "schedule", "workflow_dispatch"}, "Unsupported source event")
    require(event != "push" or kind == "ci", "Unexpected source push")
    require(event != "pull_request" or kind not in PERFORMANCE, "Performance requests require trusted main")
    head = run.get("head_sha", "")
    require(SHA.fullmatch(head), "Invalid source workflow commit")
    options = normalized_options(kind, options or dict.fromkeys(OPTIONS, ""))
    base, number = "", 0
    if event == "pull_request":
        pulls = run.get("pull_requests", [])
        require(len(pulls) == 1, "Source run must identify one pull request")
        number = positive(pulls[0].get("number"))
        pull = read(f"repos/{CORE}/pulls/{number}")
        require(pull.get("state") == "open" and not pull.get("merged")
                and pull.get("base", {}).get("repo", {}).get("id") == CORE_ID
                and pull["base"].get("ref") == "main"
                and pull.get("head", {}).get("sha") == head
                and pull["head"].get("repo", {}).get("id") == CORE_ID
                and run.get("head_repository", {}).get("id") == CORE_ID
                and pull["head"].get("ref") == run.get("head_branch"), "Pull request identity changed")
        author = pull.get("user") or {}
        require((author.get("id"), author.get("login")) in {(18637774, "cagojeiger"),
                                                               (49699333, "dependabot[bot]")},
                "Pull request author is not trusted")
        base, tested = pull["base"].get("sha", ""), pull.get("merge_commit_sha", "")
        require(SHA.fullmatch(base) and isinstance(tested, str) and SHA.fullmatch(tested)
                and pull.get("mergeable") is True, "Pull request merge is not ready")
        require(pulls[0].get("head", {}).get("sha") == head
                and pulls[0].get("base", {}).get("sha") == base, "Source run PR tuple is stale")
        commit = read(f"repos/{CORE}/git/commits/{tested}")
        require([item.get("sha") for item in commit.get("parents", [])] == [base, head],
                "Synthetic merge parents do not match the requested PR")
    else:
        require(run.get("head_repository", {}).get("id") == CORE_ID
                and run.get("head_branch") == "main", "Source must run from trusted core main")
        main_ancestor(CORE, head, read)
        tested = options["candidate_sha"] or head
        main_ancestor(CORE, tested, read)
    query = urlencode({"head_sha": head, "event": event, "per_page": 100})
    response = read(f"repos/{CORE}/actions/workflows/{WORKFLOWS[kind]}/runs?{query}")
    runs = response.get("workflow_runs", [])
    require(response.get("total_count") == len(runs) and runs, "Incomplete source run history")
    if number:
        runs = [item for item in runs if any(pull.get("number") == number for pull in item.get("pull_requests", []))]
    require(runs, "Source run association disappeared")
    latest = max(runs, key=lambda item: (item["run_number"], item["id"]))
    require(latest["id"] == run_id and latest.get("run_attempt") == attempt, "A newer source run or attempt exists")
    require(isinstance(control_sha, str) and SHA.fullmatch(control_sha), "Pinned public control revision is required")
    return {"kind": kind, "run_id": run_id, "attempt": attempt, "event": event,
            "pr_number": number, "head_sha": head, "base_sha": base, "tested_sha": tested,
            "options": options, "control_sha": control_sha}


def validate_request(request, read=api):
    request = decode_request(json.dumps(request))
    require(source_plan(request["run_id"], request["attempt"], request["kind"],
                        request["options"], request["control_sha"], read) == request,
            "Source request changed")
    return request


def selection(request, read=api):
    paths = None
    if request["event"] == "pull_request":
        pull = read(f"repos/{CORE}/pulls/{request['pr_number']}")
        count = pull.get("changed_files")
        if type(count) is int and 0 < count <= 3000:
            paths = []
            received = 0
            for page in range(1, (count + 99) // 100 + 1):
                files = read(f"repos/{CORE}/pulls/{request['pr_number']}/files?per_page=100&page={page}")
                if not isinstance(files, list) or any(not isinstance(item, dict) for item in files):
                    paths = None
                    break
                received += len(files)
                paths.extend(item.get("filename") for item in files)
                # Renames affect both paths; unknown prior paths must retain full checks.
                paths.extend(item["previous_filename"] for item in files if item.get("previous_filename"))
            if received != count:
                paths = None
    return ci_impact.classify(request["event"], paths)


def title(request):
    return f"Core {request['kind']} {request['run_id']}.{request['attempt']} {request['tested_sha']}"


def public_run(request, read=api):
    workflow = read(f"repos/{PUBLIC}/actions/workflows/{request['kind']}.yml")
    require(workflow.get("path") == f".github/workflows/{request['kind']}.yml", "Unexpected public workflow")
    matches = []
    for page in range(1, 6):
        response = read(f"repos/{PUBLIC}/actions/workflows/{workflow['id']}/runs?event=workflow_dispatch&per_page=100&page={page}")
        rows = response.get("workflow_runs", [])
        matches.extend(item for item in rows if item.get("display_title") == title(request))
        if len(rows) < 100:
            break
    else:
        raise Stopped("Incomplete public execution history")
    require(len(matches) <= 1, "Duplicate public executions require investigation")
    if not matches:
        return None
    run = read(f"repos/{PUBLIC}/actions/runs/{matches[0]['id']}")
    require(run.get("workflow_id") == workflow["id"] and run.get("display_title") == title(request)
            and run.get("path", "").split("@")[0] == workflow["path"]
            and run.get("head_repository", {}).get("id") == PUBLIC_ID
            and run.get("head_branch") == "main" and run.get("event") == "workflow_dispatch"
            and run.get("actor", {}).get("id") in DISPATCH_ACTORS
            and run.get("triggering_actor", {}).get("id") in DISPATCH_ACTORS,
            "Unexpected public execution provenance")
    validate_control(request["control_sha"], run.get("head_sha", ""), read)
    return run


def validate_gate_execution(request, read=api):
    run = public_run(request, read)
    require(run is not None and run.get("id") == positive(os.environ.get("GITHUB_RUN_ID"))
            and run.get("run_attempt") == positive(os.environ.get("GITHUB_RUN_ATTEMPT"))
            and run.get("head_sha") == os.environ.get("GITHUB_SHA")
            and run.get("status") == "in_progress", "Gate is not the current authorized public execution")
    return run


def expected_jobs(request, impact):
    event = request["event"]
    if request["kind"] == "ci":
        return {
            "hygiene": ("Hygiene", True), "security": ("Source Security", True), "rust_build": ("Rust Build", impact["rust_build"]),
            "rust": ("Rust", impact["rust"]), "postgres18": ("PostgreSQL 18", impact["postgres"]),
            "web": ("Web", impact["web"]), "browser": ("Browser E2E", impact["browser"]),
            "main_audit": ("Main RustSec Audit", event == "push"),
            "image": ("PR Image Gate", event == "pull_request" and impact["image"]),
            "browser_ui": ("Browser UI", impact["browser"]), "browser_api": ("Browser API", impact["browser"]),
        }
    return {
        "dependency-security": {"rust_audit": ("RustSec Audit", True),
                                "npm_audit": ("Npm Audit", True)},
        "image-security": {"image_resolve": ("Resolve latest image", True),
                           "image_amd64": ("Trivy linux/amd64", True), "image_arm64": ("Trivy linux/arm64", True)},
        "security-dast": {"dast": ("Isolated adversarial scenarios", True)},
        "browser-performance": {"browser_performance": ("Authenticated Browser Baseline", True)},
        "text-write-performance": {"text_performance": ("Text Write Comparison", True)},
    }[request["kind"]]


def verify_results(request, run, read=api):
    require(run.get("status") == "completed" and run.get("conclusion") == "success",
            "Public CI did not complete successfully")
    attempt = positive(run.get("run_attempt"))
    response = read(f"repos/{PUBLIC}/actions/runs/{run['id']}/attempts/{attempt}/jobs?per_page=100")
    jobs = response.get("jobs", [])
    require(response.get("total_count") == len(jobs), "Incomplete public job list")
    required = {"gate": ("Validate core request", True), **expected_jobs(request, selection(request, read))}
    results = {}
    for key, (name, enabled) in required.items():
        matching = [job for job in jobs if job.get("name") == name]
        require(len(matching) == 1, "Missing or duplicate public job: " + name)
        job = matching[0]
        require(job.get("run_id") == run["id"] and job.get("run_attempt") == attempt
                and job.get("head_sha") == run["head_sha"] and job.get("status") == "completed",
                "Public job belongs to another execution")
        require(job.get("conclusion") == ("success" if enabled else "skipped"),
                "Public job does not match the trusted execution plan: " + name)
        results[key] = job["conclusion"]
    latest = read(f"repos/{PUBLIC}/actions/runs/{run['id']}")
    require(latest.get("run_attempt") == attempt and latest.get("status") == "completed"
            and latest.get("conclusion") == "success" and latest.get("head_sha") == run["head_sha"],
            "Public CI was rerun during verification")
    validate_request(request, read)
    return results


def producer_run(request, read=api):
    source_id = positive(request["options"]["web_artifact_run_id"])
    source = read(f"repos/{CORE}/actions/runs/{source_id}")
    require(source.get("id") == source_id and source.get("workflow_id") == WORKFLOWS["ci"]
            and source.get("path") == ".github/workflows/ci.yml"
            and source.get("event") == "push" and source.get("head_branch") == "main"
            and source.get("head_repository", {}).get("id") == CORE_ID
            and source.get("head_sha") == request["tested_sha"] and source.get("status") == "completed"
            and source.get("conclusion") == "success", "Frontend producer is not matching successful core main CI")
    producer = {**request, "kind": "ci", "run_id": source_id, "attempt": positive(source.get("run_attempt")),
                "event": "push", "head_sha": source["head_sha"], "base_sha": "", "pr_number": 0,
                "options": dict.fromkeys(OPTIONS, "")}
    run = public_run(producer, read)
    require(run is not None and run.get("status") == "completed" and run.get("conclusion") == "success",
            "Frontend producer has no matching successful public CI")
    jobs = read(f"repos/{PUBLIC}/actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs?per_page=100")
    rows = jobs.get("jobs", [])
    require(jobs.get("total_count") == len(rows), "Incomplete frontend producer job list")
    for name in ("Validate core request", "Web"):
        matching = [job for job in rows if job.get("name") == name]
        require(len(matching) == 1 and matching[0].get("conclusion") == "success"
                and matching[0].get("status") == "completed" and matching[0].get("run_id") == run["id"]
                and matching[0].get("run_attempt") == run["run_attempt"]
                and matching[0].get("head_sha") == run["head_sha"], "Public frontend producer job is not successful")
    require(read(f"repos/{CORE}/actions/runs/{source_id}") == source
            and read(f"repos/{PUBLIC}/actions/runs/{run['id']}") == run,
            "Frontend producer changed during verification")
    return str(run["id"])


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def artifact_request(artifact_id, token):
    headers = {"Authorization": "Bearer " + token, "X-GitHub-Api-Version": "2022-11-28"}
    request = Request(f"https://api.github.com/repos/{CORE}/actions/artifacts/{positive(artifact_id)}/zip", headers=headers)
    try:
        build_opener(NoRedirect).open(request, timeout=30)
        raise Stopped("Expected a signed request artifact redirect")
    except HTTPError as error:
        require(error.code == 302, "Request artifact could not be read")
        location = error.headers.get("Location", "")
    parsed = urlparse(location)
    require(parsed.scheme == "https" and parsed.username is None and parsed.password is None
            and parsed.hostname and (parsed.hostname.endswith(".blob.core.windows.net")
                                     or parsed.hostname.endswith(".actions.githubusercontent.com")),
            "Unexpected artifact storage host")
    try:
        with build_opener(NoRedirect).open(Request(location), timeout=30) as response:
            archive = response.read(65537)
    except (HTTPError, URLError, TimeoutError):
        raise Stopped("Signed request artifact could not be read") from None
    require(len(archive) <= 65536, "Request artifact is too large")
    with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
        members = bundle.infolist()
        require(len(members) == 1 and members[0].filename == "public-ci-request.json"
                and members[0].file_size <= 16384 and not members[0].flag_bits & 1,
                "Unexpected request artifact content")
        content = bundle.read(members[0])
        require(len(content) <= 16384, "Expanded request artifact is too large")
        return decode_request(content.decode("utf-8"))


def dispatcher_request(run_id, attempt, read=api, load=artifact_request, pause=time.sleep):
    run_id, attempt = positive(run_id), positive(attempt)
    checked_repository(CORE, CORE_ID, read)
    checked_repository(PUBLIC, PUBLIC_ID, read)
    for _ in range(24):
        run = read(f"repos/{CORE}/actions/runs/{run_id}")
        kinds = [kind for kind, identity in WORKFLOWS.items() if identity == run.get("workflow_id")]
        require(run.get("id") == run_id and len(kinds) == 1, "Unexpected source workflow")
        kind = kinds[0]
        require(run.get("path", "").split("@")[0] == f".github/workflows/{kind}.yml",
                "Unexpected source workflow")
        current_attempt = positive(run.get("run_attempt"))
        require(current_attempt >= attempt, "Source event refers to a future attempt")
        if current_attempt > attempt or run.get("status") == "completed":
            return None
        require(run.get("status") == "in_progress", "Source request is no longer active")
        response = read(f"repos/{CORE}/actions/runs/{run_id}/artifacts?per_page=100")
        artifacts = response.get("artifacts", [])
        require(response.get("total_count") == len(artifacts), "Incomplete request artifact list")
        matches = [item for item in artifacts if item.get("name") == f"public-ci-request-{kind}-{attempt}"
                   and not item.get("expired")]
        require(len(matches) <= 1, "Duplicate request artifacts")
        if matches:
            require(matches[0].get("size_in_bytes", 65537) <= 65536, "Request artifact exceeds its limit")
            request = load(matches[0]["id"], os.environ["GH_TOKEN"])
            require(request["run_id"] == run_id and request["attempt"] == attempt and request["kind"] == kind,
                    "Request artifact belongs to another execution")
            require(request["control_sha"] == os.environ["PUBLIC_CI_REVISION"], "Unexpected requested control revision")
            return validate_request(request, read)
        pause(10)
    raise Stopped("Core request artifact was not published in time")


def dispatch(request, read=api):
    validate_request(request, read)
    main = read(f"repos/{PUBLIC}/commits/main").get("sha", "")
    validate_control(request["control_sha"], main, read)
    if public_run(request, read) is not None:
        return False
    token = os.environ.get("DISPATCH_TOKEN")
    require(token, "Public dispatch token is missing")
    read(f"repos/{PUBLIC}/actions/workflows/{request['kind']}.yml/dispatches",
         {"ref": "main", "inputs": {"request": json.dumps(request, separators=(",", ":"))}}, token=token)
    return True


def active_source(request, read=api):
    """Cheap polling check; full immutable provenance is checked before and after."""
    run = read(f"repos/{CORE}/actions/runs/{request['run_id']}")
    require(run.get("id") == request["run_id"] and run.get("run_attempt") == request["attempt"]
            and run.get("status") == "in_progress" and run.get("head_sha") == request["head_sha"],
            "Source workflow is no longer the active attempt")
    if request["event"] == "pull_request":
        pull = read(f"repos/{CORE}/pulls/{request['pr_number']}")
        require(pull.get("state") == "open" and not pull.get("merged") and pull.get("mergeable") is True
                and pull.get("head", {}).get("sha") == request["head_sha"]
                and pull.get("base", {}).get("sha") == request["base_sha"]
                and pull.get("merge_commit_sha") == request["tested_sha"], "Pull request changed during public CI")


def wait(request, read=api, pause=time.sleep, clock=time.monotonic, timeout=5100, discovery_timeout=900):
    validate_request(request, read)
    started = clock()
    deadline = started + timeout
    discovery_deadline = started + min(timeout, discovery_timeout)
    run = None
    poll_cache = {}
    poll_read = (lambda path: api(path, poll_cache=poll_cache)) if read is api else read
    while (now := clock()) < deadline:
        if run is None and now >= discovery_deadline:
            raise Stopped("Public CI was not found before the discovery deadline; check the dispatcher and GitHub before retrying")
        active_source(request, poll_read)
        if run:
            run_id = positive(run['id'])
            run = poll_read(f"repos/{PUBLIC}/actions/runs/{run_id}")
            require(isinstance(run, dict) and run.get("id") == run_id, "Public execution identity changed during polling")
        else:
            run = public_run(request, read)
        if run and run.get("status") == "completed":
            if run.get("conclusion") != "success":
                conclusion = run.get("conclusion")
                if conclusion not in {"failure", "cancelled", "timed_out", "action_required", "neutral", "skipped", "stale", "startup_failure"}:
                    conclusion = "unknown"
                raise Stopped(f"Public CI finished with {conclusion}: https://github.com/{PUBLIC}/actions/runs/{positive(run['id'])}")
            # Discover again to reject duplicates and recheck actor, definition and latest attempt.
            verified = public_run(request, read)
            require(verified is not None and verified.get("id") == run.get("id")
                    and verified.get("run_attempt") == run.get("run_attempt"),
                    "Public execution changed during verification")
            results = verify_results(request, verified, read)
            return {**results, "public_run_url": f"https://github.com/{PUBLIC}/actions/runs/{run['id']}"}
        pause(30)
    if run:
        raise Stopped(f"Public CI did not finish before the core wait deadline: https://github.com/{PUBLIC}/actions/runs/{positive(run['id'])}")
    raise Stopped("Public CI was not found before the core wait deadline; check the dispatcher and GitHub before retrying")


def core_context():
    require(os.environ.get("GITHUB_REPOSITORY") == CORE, "Unexpected core execution repository")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "dispatcher-plan", "dispatch", "gate", "wait"))
    parser.add_argument("--kind", choices=tuple(WORKFLOWS))
    args = parser.parse_args()
    if args.command == "prepare":
        core_context()
        options = {"candidate_sha": os.environ.get("CANDIDATE_SHA", ""),
                   "base_ref": os.environ.get("BASE_REF", ""),
                   "web_artifact_run_id": os.environ.get("WEB_ARTIFACT_RUN_ID", "")}
        wait_for_source_start(os.environ["SOURCE_RUN_ID"], os.environ["SOURCE_ATTEMPT"])
        request = source_plan(os.environ["SOURCE_RUN_ID"], os.environ["SOURCE_ATTEMPT"], args.kind,
                              options, os.environ.get("PUBLIC_CI_REVISION"))
        require(request["tested_sha"] == os.environ.get("TESTED_SHA"), "Workflow checkout commit changed")
        print(json.dumps(request, sort_keys=True))
    elif args.command == "dispatcher-plan":
        core_context()
        require(os.environ.get("GITHUB_REF") == "refs/heads/main"
                and os.environ.get("GITHUB_EVENT_NAME") == "workflow_run", "Dispatcher requires trusted core main")
        request = dispatcher_request(os.environ["SOURCE_RUN_ID"], os.environ["SOURCE_ATTEMPT"])
        if request is None:
            print("Ignored completed or superseded source event", file=sys.stderr)
            print("should_dispatch=false")
            return
        main_sha = api(f"repos/{PUBLIC}/commits/main")["sha"]
        validate_control(request["control_sha"], main_sha)
        path = Path(os.environ["REQUEST_FILE"])
        require(path == Path(os.environ["RUNNER_TEMP"]) / "public-ci-dispatch.json", "Unexpected dispatch plan path")
        path.write_text(json.dumps(request))
        path.chmod(0o600)
        print("Validated source-free request before minting public dispatch credentials", file=sys.stderr)
        print("should_dispatch=true")
    elif args.command == "gate":
        require(os.environ.get("GITHUB_REPOSITORY") == PUBLIC and os.environ.get("GITHUB_REF") == "refs/heads/main"
                and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "Gate requires trusted public main")
        request = validate_request(decode_request(os.environ["REQUEST_JSON"]))
        require(args.kind == request["kind"], "Request selected a different workflow")
        validate_gate_execution(request)
        output = {"source_event": request["event"], **{key: request[key] for key in
                  ("tested_sha", "head_sha", "base_sha", "pr_number", "run_id", "attempt")},
                  "base_ref": request["options"]["base_ref"], "web_artifact_run_id": ""}
        if request["kind"] == "ci":
            impact = selection(request)
            output.update({key: str(impact[key]).lower() for key in ci_impact.JOBS})
            output.update(impact_scope=impact["scope"], impact_reason=impact["reason"])
        if request["options"]["web_artifact_run_id"]:
            output["web_artifact_run_id"] = producer_run(request)
        for key, value in output.items():
            print(f"{key}={value}")
    else:
        core_context()
        path = Path(os.environ["REQUEST_FILE"])
        expected_name = "public-ci-dispatch.json" if args.command == "dispatch" else "public-ci-request.json"
        require(path == Path(os.environ["RUNNER_TEMP"]) / expected_name, "Unexpected request path")
        request = decode_request(path.read_text())
        require(request["control_sha"] == os.environ["PUBLIC_CI_REVISION"], "Control revision changed")
        if args.command == "dispatch":
            require(os.environ.get("GITHUB_REF") == "refs/heads/main"
                    and os.environ.get("GITHUB_EVENT_NAME") == "workflow_run", "Dispatch requires trusted core main")
            created = dispatch(request)
            print("Requested immutable public CI execution" if created
                  else "Validated existing public CI execution; no dispatch sent")
        else:
            require(args.kind == request["kind"], "Unexpected requested workflow")
            for key, value in wait(request).items():
                print(f"{key}={value}")


if __name__ == "__main__":
    try:
        main()
    except Stopped as error:
        sys.exit(f"Public CI stopped: {error}")
    except (OSError, KeyError, TypeError, ValueError, zipfile.BadZipFile):
        sys.exit("Public CI stopped: malformed metadata or unavailable GitHub response")
