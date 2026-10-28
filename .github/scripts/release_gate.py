#!/usr/bin/env python3
"""Validate the source commit before a public NoteGate release."""

import argparse
import base64
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request


SOURCE_REPOSITORY = "cagojeiger/notegate-core"
SOURCE_REPOSITORY_ID = 1253185495
DISTRIBUTION_REPOSITORY = "cagojeiger/notegate"
DISTRIBUTION_REPOSITORY_ID = 1437742487
CI_WORKFLOW_ID = 294404687
REQUIRED_JOBS = {
    "Hygiene",
    "Main RustSec Audit",
    "Rust Build",
    "Rust",
    "PostgreSQL 18",
    "Web",
    "Browser E2E",
}
SHA_PATTERN = re.compile(r"[0-9a-f]{40}")
VERSION_PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


class GateError(RuntimeError):
    pass


def api(path, missing_ok=False):
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2026-03-10",
        "User-Agent": "notegate-release",
    }
    token = os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(f"https://api.github.com/{path}", headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if error.code == 404 and missing_ok:
            return None
        raise GateError(f"GitHub API request failed (HTTP {error.code})") from None
    except (urllib.error.URLError, TimeoutError):
        raise GateError("GitHub API request could not be completed") from None


def validate_context(environment):
    if environment.get("GITHUB_REPOSITORY") != DISTRIBUTION_REPOSITORY:
        raise GateError("Release must run in the public distribution repository")
    if environment.get("GITHUB_REF") != "refs/heads/main":
        raise GateError("Release must use the trusted main workflow")
    if environment.get("GITHUB_EVENT_NAME") != "workflow_dispatch":
        raise GateError("Release requires an explicit workflow dispatch")
    source_sha = environment.get("SOURCE_SHA", "")
    if not SHA_PATTERN.fullmatch(source_sha):
        raise GateError("SOURCE_SHA must be a complete lowercase commit SHA")
    publish = environment.get("PUBLISH", "")
    if publish not in {"true", "false"}:
        raise GateError("PUBLISH must be true or false")
    return source_sha, publish == "true"


def validate_source(source_sha, read=api):
    if not SHA_PATTERN.fullmatch(source_sha):
        raise GateError("Source commit must be a complete lowercase SHA")
    repository = read(f"repos/{SOURCE_REPOSITORY}")
    if (
        repository.get("id") != SOURCE_REPOSITORY_ID
        or repository.get("full_name") != SOURCE_REPOSITORY
        or repository.get("archived")
        or repository.get("private") is not False
    ):
        raise GateError("Source repository identity or public access changed")
    main = read(f"repos/{SOURCE_REPOSITORY}/commits/main")
    if main.get("sha") != source_sha:
        raise GateError("Source commit is no longer the current core main")
    version_file = read(f"repos/{SOURCE_REPOSITORY}/contents/VERSION?ref={source_sha}")
    try:
        if version_file.get("encoding") != "base64":
            raise ValueError
        version = base64.b64decode(version_file["content"]).decode("ascii").strip()
    except (KeyError, UnicodeError, ValueError):
        raise GateError("Source VERSION could not be read") from None
    if not VERSION_PATTERN.fullmatch(version):
        raise GateError("Source VERSION must be a stable major.minor.patch version")
    query = urllib.parse.urlencode(
        {"head_sha": source_sha, "event": "push", "branch": "main", "per_page": 100}
    )
    response = read(
        f"repos/{SOURCE_REPOSITORY}/actions/workflows/{CI_WORKFLOW_ID}/runs?{query}"
    )
    runs = response.get("workflow_runs", [])
    if not runs:
        raise GateError("No main push CI exists for the source commit")
    run = max(runs, key=lambda item: (item["run_number"], item["run_attempt"], item["id"]))
    if (
        run.get("workflow_id") != CI_WORKFLOW_ID
        or run.get("path") != ".github/workflows/ci.yml"
        or run.get("event") != "push"
        or run.get("head_branch") != "main"
        or run.get("head_sha") != source_sha
        or run.get("head_repository", {}).get("id") != SOURCE_REPOSITORY_ID
    ):
        raise GateError("Source CI does not match the requested repository and commit")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise GateError("The latest source CI must be completed and successful")
    jobs_response = read(
        f"repos/{SOURCE_REPOSITORY}/actions/runs/{run['id']}/attempts/"
        f"{run['run_attempt']}/jobs?per_page=100"
    )
    jobs = jobs_response.get("jobs", [])
    if jobs_response.get("total_count") != len(jobs):
        raise GateError("Source CI job list is incomplete")
    for name in sorted(REQUIRED_JOBS):
        matches = [job for job in jobs if job.get("name") == name]
        if (
            len(matches) != 1
            or matches[0].get("status") != "completed"
            or matches[0].get("conclusion") != "success"
        ):
            raise GateError(f"Required source CI job did not pass: {name}")
    return {"version": version, "tag": f"v{version}", "source_sha": source_sha, "ci_run_id": run["id"]}


def release_plan(source_sha, publish, read=api):
    source = validate_source(source_sha, read)
    repository = read(f"repos/{DISTRIBUTION_REPOSITORY}")
    if (
        repository.get("id") != DISTRIBUTION_REPOSITORY_ID
        or repository.get("full_name") != DISTRIBUTION_REPOSITORY
        or repository.get("private") is not False
    ):
        raise GateError("Distribution repository identity or public access changed")
    existing = read(
        f"repos/{DISTRIBUTION_REPOSITORY}/releases/tags/{source['tag']}", missing_ok=True
    )
    if existing is not None and publish:
        raise GateError("This release version already exists; it will not be overwritten")
    if publish:
        if read(
            f"repos/{DISTRIBUTION_REPOSITORY}/git/ref/tags/{source['tag']}", missing_ok=True
        ) is not None:
            raise GateError("This distribution tag already exists; it will not be overwritten")
        page = 1
        while True:
            releases = read(
                f"repos/{DISTRIBUTION_REPOSITORY}/releases?per_page=100&page={page}"
            )
            if any(release.get("tag_name") == source["tag"] for release in releases):
                raise GateError("A release or draft already reserves this version")
            if len(releases) < 100:
                break
            page += 1
    latest = read(f"repos/{DISTRIBUTION_REPOSITORY}/releases/latest", missing_ok=True)
    if latest is not None:
        latest_version = latest.get("tag_name", "").removeprefix("v")
        if not VERSION_PATTERN.fullmatch(latest_version):
            raise GateError("Latest distribution release has an invalid version")
        if publish and tuple(map(int, source["version"].split("."))) <= tuple(
            map(int, latest_version.split("."))
        ):
            raise GateError("Release version must be newer than the current public release")
    return {**source, "should_release": "true" if publish else "false"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-only", action="store_true", help="Recheck only the source and its CI")
    arguments = parser.parse_args()
    source_sha, publish = validate_context(os.environ)
    if arguments.check_only:
        validate_source(source_sha)
        print("Source main and required CI jobs verified")
        return
    result = release_plan(source_sha, publish)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in result.items():
            output.write(f"{name}={value}\n")
    mode = "Release approved" if publish else "Plan only: no builds, registry writes, or release publication"
    summary = f"{mode}. Core version {result['version']}; CI run {result['ci_run_id']}.\n"
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(summary)
    print(summary.strip())


if __name__ == "__main__":
    try:
        main()
    except GateError as error:
        print(f"::error::{error}", file=sys.stderr)
        raise SystemExit(1) from None
