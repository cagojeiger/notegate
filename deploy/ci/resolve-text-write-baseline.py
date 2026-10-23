#!/usr/bin/env python3
"""Pin a published release baseline before the paired text-write measurement."""

import json
import os
from pathlib import Path
import subprocess
import sys


BASELINE_LAYOUT = (
    "backend/crates/service/Cargo.toml",
    "backend/crates/service/tests/common/mod.rs",
)


def git(*args):
    return subprocess.run(["git", *args], text=True, capture_output=True, check=False)


def commit(ref):
    result = git("rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}")
    return result.stdout.strip() if result.returncode == 0 else None


def resolve(release_pages, base_ref=""):
    candidate = commit("HEAD")
    if not candidate:
        raise ValueError("Candidate checkout is not a commit")
    if base_ref:
        baseline = commit(base_ref) or commit(f"refs/remotes/origin/{base_ref}")
        if not baseline:
            raise ValueError(f"Cannot resolve baseline ref: {base_ref}")
        if baseline == candidate:
            return {"candidate_sha": candidate, "sha": "", "ref": "",
                    "reason": "Baseline equals candidate; comparison skipped"}
        if git("merge-base", "--is-ancestor", baseline, candidate).returncode:
            raise ValueError("Manual baseline must be an ancestor of the candidate")
        choices = [(baseline, base_ref)]
    else:
        releases = [release for page in release_pages for release in page
                    if not release["draft"] and not release["prerelease"]
                    and release.get("published_at")]
        releases.sort(key=lambda release: release["published_at"], reverse=True)
        choices = [(commit(f"refs/tags/{release['tag_name']}"), release["tag_name"])
                   for release in releases]
    for baseline, ref in choices:
        if not baseline or baseline == candidate:
            continue
        if not base_ref and git("merge-base", "--is-ancestor", baseline, candidate).returncode:
            continue
        # The comparison copies the candidate harness. Historical harness presence
        # is irrelevant; compilation still verifies API compatibility.
        if not base_ref and any(git("cat-file", "-e", f"{baseline}:{path}").returncode
                                for path in BASELINE_LAYOUT):
            continue
        return {"candidate_sha": candidate, "sha": baseline, "ref": ref,
                "reason": "Manual baseline" if base_ref else "Previous published release with required test layout"}
    return {"candidate_sha": candidate, "sha": "", "ref": "",
            "reason": "No distinct published baseline with required test layout; comparison skipped"}


def main():
    baseline = resolve(json.load(sys.stdin), os.environ.get("BASE_REF", ""))
    results = Path("text-write-results")
    results.mkdir(exist_ok=True)
    (results / "baseline.json").write_text(json.dumps(baseline, indent=2) + "\n")
    summary = (f"### Text write comparison\n\n{baseline['reason']}\n\n"
               f"Candidate: `{baseline['candidate_sha']}`\n\n")
    if baseline["sha"]:
        summary += f"Baseline: `{baseline['ref']}` (`{baseline['sha']}`)\n\n"
    (results / "baseline.md").write_text(summary)
    print(f"sha={baseline['sha']}")


if __name__ == "__main__":
    main()
