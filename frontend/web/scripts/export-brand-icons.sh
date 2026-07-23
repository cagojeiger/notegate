#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
web_dir="$(cd "${script_dir}/.." && pwd)"
public_dir="${web_dir}/public"
brand_dir="${public_dir}/brand"
app_icon="${brand_dir}/source/app-icon.svg"
maskable_icon="${brand_dir}/source/app-icon-maskable.svg"

if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "rsvg-convert is required to export NoteGate icons." >&2
  exit 1
fi

render_png() {
  local source="$1"
  local size="$2"
  local output="$3"
  rsvg-convert --width "${size}" --height "${size}" --output "${output}" "${source}"
}

render_png "${app_icon}" 150 "${brand_dir}/png/mstile-150.png"
render_png "${app_icon}" 180 "${brand_dir}/png/app-icon-180.png"
render_png "${app_icon}" 192 "${brand_dir}/png/app-icon-192.png"
render_png "${app_icon}" 384 "${brand_dir}/png/app-icon-384.png"
render_png "${maskable_icon}" 192 "${brand_dir}/png/maskable-icon-192.png"
render_png "${maskable_icon}" 512 "${brand_dir}/png/maskable-icon-512.png"

cp "${brand_dir}/png/app-icon-180.png" "${public_dir}/apple-touch-icon.png"

if command -v sips >/dev/null 2>&1; then
  sips -s format ico "${brand_dir}/favicon/favicon-32.png" --out "${public_dir}/favicon.ico" >/dev/null
else
  echo "sips is unavailable; favicon.ico was not regenerated." >&2
fi

echo "NoteGate platform icons exported."
