#!/usr/bin/env python3
"""Check release versions without resolving or downloading dependencies (Python 3.11+)."""

from pathlib import Path
import re
import sys
import tomllib


def check(root: Path) -> list[str]:
    version = (root / "VERSION").read_text().strip()
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    packages = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    errors = []
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        errors.append("VERSION must be a stable major.minor.patch version")
    if workspace["package"]["version"] != version:
        errors.append("Cargo.toml workspace.package.version must match VERSION")
    for member in workspace["members"]:
        manifest = tomllib.loads((root / member / "Cargo.toml").read_text())["package"]
        name = manifest["name"]
        if manifest["version"] != {"workspace": True}:
            errors.append(f"{member}/Cargo.toml must inherit version.workspace = true")
        locked = [p["version"] for p in packages if p["name"] == name and "source" not in p]
        if locked != [version]:
            errors.append(f"Cargo.lock {name} must have exactly one local entry at {version}")
    return errors


if __name__ == "__main__":
    problems = check(Path(__file__).resolve().parents[2])
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)
    print("Release versions agree: VERSION, workspace manifests, Cargo.lock")
