#!/usr/bin/env python3
"""Validate the exact core main Web job before a public shadow execution."""

import argparse
import base64
import json
import os
import re
import sys
from urllib.parse import urlencode

from release_gate import (CI_WORKFLOW_ID, DISTRIBUTION_REPOSITORY, DISTRIBUTION_REPOSITORY_ID,
                          SOURCE_REPOSITORY, SOURCE_REPOSITORY_ID, SHA_PATTERN, GateError, api)

WEB_COMMANDS = [
    "pnpm install --frozen-lockfile", "pnpm audit --audit-level moderate",
    "pnpm --filter web check:contrast", "pnpm --filter web typecheck", "pnpm --filter web lint",
    "pnpm --filter web test", "pnpm --filter web build", "pnpm --filter web check:preview-bundle",
]


def require(condition, message):
    if not condition:
        raise GateError(message)


def positive_integer(value):
    require(re.fullmatch(r"[1-9][0-9]*", str(value)), "Expected a positive run ID or attempt")
    return int(value)


def validate_source(sha, run_id, attempt, read=api):
    require(SHA_PATTERN.fullmatch(sha), "Source commit must be a complete lowercase SHA")
    run_id, attempt = positive_integer(run_id), positive_integer(attempt)
    core = f"repos/{SOURCE_REPOSITORY}"
    repository = read(core)
    require(repository.get("id") == SOURCE_REPOSITORY_ID and repository.get("full_name") == SOURCE_REPOSITORY
            and not repository.get("archived"), "Unexpected source repository")
    run = read(f"{core}/actions/runs/{run_id}")
    require(run.get("id") == run_id and run.get("run_attempt") == attempt
            and run.get("workflow_id") == CI_WORKFLOW_ID and run.get("path") == ".github/workflows/ci.yml"
            and run.get("head_sha") == sha and run.get("head_repository", {}).get("id") == SOURCE_REPOSITORY_ID
            and run.get("head_branch") == "main" and run.get("event") == "push"
            and run.get("status") == "completed" and run.get("conclusion") in {"success", "failure"},
            "Source must identify a completed main push CI attempt")
    query = urlencode({"head_sha": sha, "event": "push", "branch": "main", "per_page": 100})
    response = read(f"{core}/actions/workflows/{CI_WORKFLOW_ID}/runs?{query}")
    runs = response.get("workflow_runs", [])
    require(runs and response.get("total_count") == len(runs), "Incomplete source CI run list")
    latest = max(runs, key=lambda item: (item["run_number"], item["run_attempt"], item["id"]))
    require(latest.get("id") == run_id and latest.get("run_attempt") == attempt
            and latest.get("status") == "completed", "Source CI run or attempt was superseded")
    main_sha = read(f"{core}/commits/main").get("sha", "")
    require(SHA_PATTERN.fullmatch(main_sha), "Invalid core main commit")
    comparison = read(f"{core}/compare/{sha}...{main_sha}")
    require(comparison.get("status") in {"ahead", "identical"}
            and comparison.get("merge_base_commit", {}).get("sha") == sha, "Source is not on core main")
    response = read(f"{core}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100")
    jobs = response.get("jobs", [])
    require(response.get("total_count") == len(jobs), "Incomplete source job list")
    web = [job for job in jobs if job.get("name") == "Web"]
    require(len(web) == 1 and web[0].get("status") == "completed"
            and web[0].get("conclusion") in {"success", "failure"}
            and web[0].get("run_id") == run_id and web[0].get("run_attempt") == attempt
            and web[0].get("head_sha") == sha, "A unique completed Web job is required")
    def source_file(path):
        blob = read(f"{core}/contents/{path}?ref={sha}")
        require(blob.get("encoding") == "base64", "Unexpected source file encoding")
        return base64.b64decode(blob["content"], validate=False).decode("utf-8")
    require(source_file(".node-version").strip() == "22.23.4"
            and json.loads(source_file("package.json")).get("packageManager") == "pnpm@10.29.3",
            "Core Web tool versions changed; update the shadow comparison first")
    workflow = source_file(".github/workflows/ci.yml")
    web_job = re.search(r"^  web:\n(.*?)(?=^  [A-Za-z0-9_-]+:|\Z)", workflow, re.MULTILINE | re.DOTALL)
    require(web_job and re.findall(r"^        run: (.+)$", web_job[1], re.MULTILINE) == WEB_COMMANDS,
            "Core Web commands changed; update the shadow comparison first")
    return {"source_sha": sha, "source_ci_run_id": run_id, "source_ci_attempt": attempt,
            "core_web_conclusion": web[0]["conclusion"]}


def compare_results(environment):
    e = environment
    print(f"### Web shadow comparison\n\nSource: `{e['SOURCE_SHA']}`\n\n"
          f"Core CI: https://github.com/cagojeiger/notegate-core/actions/runs/{e['SOURCE_CI_RUN_ID']} "
          f"(attempt {e['SOURCE_CI_ATTEMPT']})\n\n"
          f"Core Web: **{e['CORE_RESULT'] or 'unavailable'}**; public Web: **{e['PUBLIC_RESULT']}**.\n\n"
          "Core Web, its frontend artifact, branch protection, and release requirements remain unchanged. "
          "This run uploads no source artifacts and uses no Actions cache.\n", end="")
    require(e["GATE_RESULT"] == "success" and e["CORE_RESULT"] == e["PUBLIC_RESULT"] == "success",
            "Core and public Web must both pass for a successful comparison")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compare", action="store_true")
    arguments = parser.parse_args()
    require(os.environ.get("GITHUB_REPOSITORY") == DISTRIBUTION_REPOSITORY
            and os.environ.get("GITHUB_REPOSITORY_ID") == str(DISTRIBUTION_REPOSITORY_ID)
            and os.environ.get("GITHUB_REF") == "refs/heads/main"
            and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "Web shadow must use trusted public main")
    if arguments.compare:
        compare_results(os.environ)
        return
    repository = api(f"repos/{DISTRIBUTION_REPOSITORY}")
    require(repository.get("id") == DISTRIBUTION_REPOSITORY_ID
            and repository.get("full_name") == DISTRIBUTION_REPOSITORY
            and repository.get("private") is False, "Web shadow must run in the public distribution repository")
    plan = validate_source(os.environ["SOURCE_SHA"], os.environ["SOURCE_CI_RUN_ID"], os.environ["SOURCE_CI_ATTEMPT"])
    for key, value in plan.items():
        print(f"{key}={value}")


if __name__ == "__main__":
    try:
        main()
    except (GateError, KeyError, ValueError, OSError) as error:
        sys.exit(f"Web shadow stopped: {error}")
