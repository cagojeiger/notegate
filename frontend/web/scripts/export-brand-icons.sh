#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
web_dir="$(cd "${script_dir}/.." && pwd)"
public_dir="${web_dir}/public"
brand_dir="${public_dir}/brand"
app_icon="${brand_dir}/source/app-icon.svg"
maskable_icon="${brand_dir}/source/app-icon-maskable.svg"
symbol_light="${brand_dir}/svg/symbol-light.svg"
symbol_dark="${brand_dir}/svg/symbol-dark.svg"

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

for size in 16 32 64 128 180 192 256 384 512 1024; do
  render_png "${app_icon}" "${size}" "${brand_dir}/png/app-icon-${size}.png"
done

for size in 16 32 48; do
  render_png "${app_icon}" "${size}" "${brand_dir}/favicon/favicon-${size}.png"
done

render_png "${app_icon}" 150 "${brand_dir}/png/mstile-150.png"
render_png "${maskable_icon}" 192 "${brand_dir}/png/maskable-icon-192.png"
render_png "${maskable_icon}" 512 "${brand_dir}/png/maskable-icon-512.png"
render_png "${symbol_light}" 1024 "${brand_dir}/png/symbol-light.png"
render_png "${symbol_dark}" 1024 "${brand_dir}/png/symbol-dark.png"

cp "${brand_dir}/png/app-icon-180.png" "${public_dir}/apple-touch-icon.png"
cp "${app_icon}" "${brand_dir}/favicon/favicon.svg"
cp "${app_icon}" "${public_dir}/favicon.svg"

node "${script_dir}/create-png-ico.mjs" \
  "${public_dir}/favicon.ico" \
  "${brand_dir}/favicon/favicon-16.png" \
  "${brand_dir}/favicon/favicon-32.png" \
  "${brand_dir}/favicon/favicon-48.png"

echo "NoteGate platform icons exported."
