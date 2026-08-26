#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="${script_dir}/resolve-release-version.sh"

tmp_root="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_root"
}
trap cleanup EXIT

failures=0

run_case() {
  local name="$1"
  shift
  local repo="${tmp_root}/${name}"
  mkdir -p "$repo"
  (
    set -euo pipefail
    cd "$repo"
    git init --quiet
    git config user.email "ci@example.test"
    git config user.name "CI Test"
    printf '0.1.0\n' > VERSION
    git add VERSION
    git commit --quiet -m "initial version"
    "$@"
  )
}

assert_output() {
  local output_file="$1"
  local key="$2"
  local expected="$3"
  if ! grep -Fx "${key}=${expected}" "$output_file" >/dev/null; then
    echo "expected ${key}=${expected} in ${output_file}" >&2
    return 1
  fi
}

case_new_version_releases() {
  local output="${tmp_root}/new-version.out"
  printf '0.1.1\n' > VERSION
  git add VERSION
  git commit --quiet -m "bump version"
  RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" version 0.1.1
  assert_output "$output" tag v0.1.1
  assert_output "$output" should_release true
}

case_ordinary_main_commit_skips_existing_version() {
  local output="${tmp_root}/ordinary.out"
  git tag v0.1.0
  printf 'docs\n' > README.md
  git add README.md
  git commit --quiet -m "docs"
  RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" should_release false
}

case_existing_tag_on_same_commit_resumes() {
  local output="${tmp_root}/resume.out"
  git tag v0.1.0
  RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" should_release true
}

case_unchanged_unreleased_version_releases() {
  local output="${tmp_root}/unreleased-unchanged.out"
  printf 'docs\n' > README.md
  git add README.md
  git commit --quiet -m "docs before initial release"
  RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" should_release true
}

case_stale_version_bump_skips() {
  local output="${tmp_root}/stale-version.out"
  printf '0.1.1\n' > VERSION
  git add VERSION
  git commit --quiet -m "bump version"
  local release_sha
  release_sha="$(git rev-parse HEAD)"
  printf 'docs\n' > README.md
  git add README.md
  git commit --quiet -m "newer main commit"
  RELEASE_SHA="$release_sha" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" should_release false
}

case_current_main_recovers_stale_version_bump() {
  local stale_output="${tmp_root}/recover-stale.out"
  local current_output="${tmp_root}/recover-current.out"
  printf '0.1.1\n' > VERSION
  git add VERSION
  git commit --quiet -m "bump version"
  local release_sha
  release_sha="$(git rev-parse HEAD)"
  printf 'docs\n' > README.md
  git add README.md
  git commit --quiet -m "newer main commit"
  local current_main_sha
  current_main_sha="$(git rev-parse HEAD)"

  RELEASE_SHA="$release_sha" CURRENT_MAIN_SHA="$current_main_sha" \
    GITHUB_OUTPUT="$stale_output" "$script"
  assert_output "$stale_output" should_release false

  RELEASE_SHA="$current_main_sha" CURRENT_MAIN_SHA="$current_main_sha" \
    GITHUB_OUTPUT="$current_output" "$script"
  assert_output "$current_output" version 0.1.1
  assert_output "$current_output" tag v0.1.1
  assert_output "$current_output" should_release true
}

case_stale_tagged_resume_skips() {
  local output="${tmp_root}/stale-resume.out"
  printf '0.1.1\n' > VERSION
  git add VERSION
  git commit --quiet -m "bump version"
  git tag v0.1.1
  local release_sha
  release_sha="$(git rev-parse HEAD)"
  printf 'docs\n' > README.md
  git add README.md
  git commit --quiet -m "newer main commit"
  RELEASE_SHA="$release_sha" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"
  assert_output "$output" should_release false
}

case_reused_tag_on_version_change_fails() {
  local output="${tmp_root}/reused.out"
  git tag v0.1.1
  printf '0.1.1\n' > VERSION
  printf 'change\n' > app.txt
  git add VERSION app.txt
  git commit --quiet -m "reuse version"
  if RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"; then
    echo "expected reused tag to fail" >&2
    return 1
  fi
}

case_invalid_version_fails() {
  local output="${tmp_root}/invalid-version.out"
  printf '0. 1.1\n' > VERSION
  git add VERSION
  git commit --quiet -m "invalid version"
  if RELEASE_SHA="$(git rev-parse HEAD)" CURRENT_MAIN_SHA="$(git rev-parse HEAD)" \
    GITHUB_OUTPUT="$output" "$script"; then
    echo "expected invalid version to fail" >&2
    return 1
  fi
}

for test_case in \
  case_new_version_releases \
  case_ordinary_main_commit_skips_existing_version \
  case_existing_tag_on_same_commit_resumes \
  case_unchanged_unreleased_version_releases \
  case_stale_version_bump_skips \
  case_current_main_recovers_stale_version_bump \
  case_stale_tagged_resume_skips \
  case_reused_tag_on_version_change_fails \
  case_invalid_version_fails
do
  case_log="${tmp_root}/${test_case}.log"
  if ! run_case "$test_case" "$test_case" >"$case_log" 2>&1; then
    cat "$case_log" >&2
    echo "FAIL ${test_case}" >&2
    failures=$((failures + 1))
  fi
done

if [ "$failures" -ne 0 ]; then
  exit 1
fi

echo "resolve-release-version tests passed"
