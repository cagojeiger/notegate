#!/usr/bin/env python3
"""Conservatively select PR jobs; unrecognized inputs retain the full CI suite."""

import argparse
from pathlib import Path


JOBS = ("rust_build", "rust", "web", "browser", "postgres", "image")
DOCS = {"README.md", "SECURITY.md", "CLAUDE.md", "CONTRIBUTING.md", "AGENTS.md"}
FRONTEND_PREFIXES = ("frontend/web/src/", "frontend/web/public/", "frontend/web/e2e/")
EMBEDDED = {
    "frontend/web/src/design/theme.css",
    "frontend/web/public/brand/svg/logo-horizontal-light.svg",
    "frontend/web/public/brand/svg/logo-horizontal-dark.svg",
}


def result(enabled, scope, reason):
    return {**{job: job in enabled for job in JOBS}, "scope": scope, "reason": reason}


def full(reason):
    return result(JOBS, "full", reason)


def canonical(path):
    return (isinstance(path, str) and bool(path) and "\\" not in path
            and path.isprintable()
            and all(part not in ("", ".", "..") for part in path.split("/")))


def classify(event, paths):
    if event != "pull_request":
        return full("non-pr-event")
    if not isinstance(paths, (list, tuple)) or not paths:
        return full("unavailable-diff")
    enabled = set()
    for path in paths:
        if not canonical(path):
            return full("invalid-path")
        if path in EMBEDDED:
            return full("embedded-rust-input")
        if path in DOCS or (path.startswith("docs/") and path.endswith(".md")):
            continue
        if path.startswith("frontend/web/src/") and path.endswith((".test.ts", ".test.tsx")):
            enabled.add("web")
        elif path.startswith(FRONTEND_PREFIXES) or path == "frontend/web/index.html":
            enabled.update(("rust_build", "web", "browser", "image"))
        else:
            return full("unknown-path")
    if "browser" in enabled:
        return result(enabled, "frontend", "frontend-change")
    if "web" in enabled:
        return result(enabled, "frontend-unit", "frontend-unit-only")
    return result(enabled, "docs", "docs-only")


def read_paths(filename):
    if not filename:
        return None
    try:
        data = Path(filename).read_bytes()
        if not data or not data.endswith(b"\0"):
            return None
        paths = data[:-1].decode("utf-8").split("\0")
        return paths if all(paths) else None
    except (OSError, UnicodeError):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", required=True)
    parser.add_argument("--paths-file")
    args = parser.parse_args()
    selection = classify(args.event, read_paths(args.paths_file))
    for key, value in selection.items():
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")


if __name__ == "__main__":
    main()
