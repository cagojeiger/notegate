#!/usr/bin/env python3
"""Check production workspace dependencies against the documented layer boundaries."""

import json
from pathlib import Path
import subprocess
import sys


# Internal production dependencies only; dev dependencies may exercise real DB adapters.
# Keep changes to this policy together with ADR 0003 and the affected runtime contract.
ALLOWED = {
    "core": set(),
    "text": set(),
    "media": set(),
    "command": set(),
    "jobs": set(),
    "reconciliation": set(),
    "model": {"core", "text"},
    "db": {"core", "model", "jobs"},
    "service": {"core", "model", "db", "text", "jobs"},
    "search": {"core", "model", "db", "text"},
    "cli": {"command"},
    "api": {
        "command", "core", "db", "jobs", "media", "model",
        "reconciliation", "search", "service", "text",
    },
}
ALLOWED = {
    f"notegate-{name}": {f"notegate-{dependency}" for dependency in dependencies}
    for name, dependencies in ALLOWED.items()
}


def check(metadata: dict) -> list[str]:
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    names = {package["name"] for package in packages}
    errors = []
    for package in packages:
        name = package["name"]
        if name not in ALLOWED:
            errors.append(f"{name}: define its dependency boundary in check-architecture.py")
            continue
        for dependency in package["dependencies"]:
            if dependency["kind"] == "dev":
                continue
            target = dependency["name"]
            # Cargo resolves package aliases; inspect optional and target-specific
            # declarations too, even when the CI host does not enable them.
            if (target in names or dependency.get("path")) and target not in ALLOWED[name]:
                kind = dependency["kind"] or "normal"
                errors.append(f"{name} -> {target} ({kind}): violates the production layer boundary")
    return errors


if __name__ == "__main__":
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=Path(__file__).resolve().parents[2],
        check=True,
        capture_output=True,
        text=True,
    )
    problems = check(json.loads(result.stdout))
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)
    print("Production workspace dependencies respect the documented layer boundaries")
