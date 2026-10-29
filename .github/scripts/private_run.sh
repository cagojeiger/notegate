#!/usr/bin/env bash
# GitHub Actions shell adapter: suppress source-derived step output.
set -u
original_umask=$(umask)
umask 077

if [[ $# != 1 || -z "${RUNNER_TEMP:-}" || ! -d "$RUNNER_TEMP" || ! -f "$1" || ! -r "$1" ]]; then
  printf '%s\n' 'Private step could not start.' >&2
  exit 1
fi

private_dir=$(mktemp -d -- "$RUNNER_TEMP/private-run.XXXXXXXX" 2>/dev/null) || {
  printf '%s\n' 'Private step could not start.' >&2
  exit 1
}
child=''
# These functions are invoked by traps.
# shellcheck disable=SC2329
cleanup() { rm -rf -- "$private_dir"; }
# shellcheck disable=SC2329
stop() {
  local code="$1"
  trap - INT TERM
  if [[ -n "$child" ]]; then
    kill -TERM "$child" 2>/dev/null || :
    wait "$child" 2>/dev/null || :
  fi
  exit "$code"
}
trap cleanup EXIT
trap 'stop 130' INT
trap 'stop 143' TERM

if ! { : > "$private_dir/stdout" && : > "$private_dir/stderr" && : > "$private_dir/summary"; } 2>/dev/null; then
  printf '%s\n' 'Private step could not start.' >&2
  exit 1
fi
(
  umask "$original_umask"
  export GITHUB_STEP_SUMMARY="$private_dir/summary"
  exec bash --noprofile --norc -e -o pipefail "$1"
) > "$private_dir/stdout" 2> "$private_dir/stderr" &
child=$!
if wait "$child"; then
  status=0
else
  status=$?
fi
child=''
printf 'Private step finished (exit %d).\n' "$status"
exit "$status"
