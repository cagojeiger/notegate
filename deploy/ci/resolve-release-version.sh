#!/usr/bin/env bash
set -euo pipefail

release_sha="${RELEASE_SHA:-${GITHUB_SHA:-}}"
current_main_sha="${CURRENT_MAIN_SHA:-}"
github_output="${GITHUB_OUTPUT:-}"

if [ -z "$release_sha" ]; then
  echo "RELEASE_SHA or GITHUB_SHA must be set" >&2
  exit 1
fi
if [ -z "$github_output" ]; then
  echo "GITHUB_OUTPUT must be set" >&2
  exit 1
fi
if [ -z "$current_main_sha" ]; then
  echo "CURRENT_MAIN_SHA must be set" >&2
  exit 1
fi

version="$(sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' VERSION)"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::VERSION must use numeric major.minor.patch format"
  exit 1
fi

tag="v${version}"
should_release=false

if [ "$release_sha" != "$current_main_sha" ]; then
  echo "::notice::CI completed for stale main commit $release_sha; current main is $current_main_sha"
  {
    echo "version=$version"
    echo "tag=$tag"
    echo "should_release=false"
  } >> "$github_output"
  exit 0
fi

version_changed=false
if git rev-parse "${release_sha}^1" >/dev/null 2>&1; then
  if ! git diff --quiet "${release_sha}^1" "$release_sha" -- VERSION; then
    version_changed=true
  fi
else
  version_changed=true
fi

if tagged_commit="$(git rev-parse --verify "refs/tags/${tag}^{commit}" 2>/dev/null)"; then
  if [ "$tagged_commit" = "$release_sha" ]; then
    echo "::notice::tag $tag already points to this commit; resuming release"
    should_release=true
  elif [ "$version_changed" = true ]; then
    echo "::error::tag $tag already points to a different commit; bump VERSION before merging"
    exit 1
  else
    echo "::notice::VERSION did not change and tag $tag already exists; skipping release"
  fi
else
  if [ "$version_changed" = false ]; then
    echo "::notice::tag $tag does not exist; releasing current main to recover the unpublished version"
  fi
  should_release=true
fi

{
  echo "version=$version"
  echo "tag=$tag"
  echo "should_release=$should_release"
} >> "$github_output"
