#!/usr/bin/env bash
set -euo pipefail

bin_dir="${RUNNER_TEMP:?}/notegate-minio-bin"
mkdir -p "$bin_dir" "${RUNNER_TEMP}/notegate-minio-data"

minio_release=RELEASE.2025-09-07T16-13-09Z
mc_release=RELEASE.2025-08-13T08-35-41Z
curl --fail --location --retry 3 --silent --show-error \
  "https://github.com/minio/minio/releases/download/$minio_release/minio.linux-amd64.$minio_release" \
  --output "$bin_dir/minio"
curl --fail --location --retry 3 --silent --show-error \
  "https://github.com/minio/mc/releases/download/$mc_release/mc.linux-amd64.$mc_release" \
  --output "$bin_dir/mc"
printf '%s  %s\n' \
  '7c5bd8512c6e966455b1d198209358b2d191c77a83ab377c4073281065fb855f' "$bin_dir/minio" \
  '01f866e9c5f9b87c2b09116fa5d7c06695b106242d829a8bb32990c00312e891' "$bin_dir/mc" \
  | sha256sum --check --status
chmod +x "$bin_dir/minio" "$bin_dir/mc"

export MINIO_ROOT_USER=minio-root
export MINIO_ROOT_PASSWORD=minio-root-secret
"$bin_dir/minio" server "${RUNNER_TEMP}/notegate-minio-data" \
  >"${RUNNER_TEMP}/notegate-minio.log" 2>&1 &

for attempt in $(seq 1 30); do
  if curl --fail --silent http://127.0.0.1:9000/minio/health/live >/dev/null; then
    PATH="$bin_dir:$PATH" MINIO_ENDPOINT=http://127.0.0.1:9000 deploy/minio/init.sh
    exit 0
  fi
  if [ "$attempt" = 30 ]; then
    cat "${RUNNER_TEMP}/notegate-minio.log"
    exit 1
  fi
  sleep 1
done
