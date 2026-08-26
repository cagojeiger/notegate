#!/bin/sh
set -eu

REPOSITORY="cagojeiger/notegate"
RELEASE_BASE="https://github.com/${REPOSITORY}/releases/latest/download"
BIN_DIR="${HOME}/.local/bin"
BIN_NAME="notegate-cli"

usage() {
  cat <<'EOF'
Install the official notegate-cli binary.

Usage:
  install.sh [--bin-dir DIR]

Options:
  --bin-dir DIR   Install notegate-cli into DIR. Defaults to ~/.local/bin.
  -h, --help      Show this help.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin-dir)
      if [ "$#" -lt 2 ]; then
        echo "install.sh: --bin-dir requires a directory" >&2
        exit 2
      fi
      BIN_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "install.sh: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    *)
      echo "install.sh: unsupported CPU architecture: $arch" >&2
      exit 1
      ;;
  esac
  case "$os" in
    Darwin) os="apple-darwin" ;;
    Linux) os="unknown-linux-gnu" ;;
    *)
      echo "install.sh: unsupported operating system: $os" >&2
      exit 1
      ;;
  esac
  printf '%s-%s\n' "$arch" "$os"
}

download() {
  url="$1"
  output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$output"
  else
    echo "install.sh: curl or wget is required" >&2
    exit 1
  fi
}

checksum_file() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "install.sh: sha256sum or shasum is required" >&2
    exit 1
  fi
}

candidate_version() {
  output="$("$1" --version)"
  version="$(printf '%s\n' "$output" | awk '/^notegate-cli [0-9]+\.[0-9]+\.[0-9]+$/ {print $2}')"
  if [ -z "$version" ]; then
    echo "install.sh: downloaded notegate-cli candidate returned an invalid version: $output" >&2
    exit 1
  fi
  printf '%s\n' "$version"
}

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

TARGET="$(detect_target)"
ASSET="${BIN_NAME}-${TARGET}"

mkdir -p "$BIN_DIR"
BIN_DIR="$(cd "$BIN_DIR" && pwd -P)"
INSTALL_PATH="${BIN_DIR}/${BIN_NAME}"
RECEIPT_PATH="${BIN_DIR}/notegate-cli-install-receipt.json"
tmp="$(mktemp "${BIN_DIR}/.notegate-cli.XXXXXX")"
checksum_tmp="$(mktemp "${BIN_DIR}/.notegate-cli.sha256.XXXXXX")"
receipt_tmp=""
cleanup() {
  if [ -n "$receipt_tmp" ]; then
    rm -f "$receipt_tmp"
  fi
  rm -f "$tmp" "$checksum_tmp"
}
trap cleanup EXIT INT TERM

download "${RELEASE_BASE}/${ASSET}" "$tmp"
download "${RELEASE_BASE}/${ASSET}.sha256" "$checksum_tmp"
expected="$(awk '{print $1}' "$checksum_tmp")"
actual="$(checksum_file "$tmp")"
if [ "$expected" != "$actual" ]; then
  echo "install.sh: checksum mismatch for ${ASSET}" >&2
  exit 1
fi

chmod 755 "$tmp"
VERSION="$(candidate_version "$tmp")"
mv "$tmp" "$INSTALL_PATH"
rm -f "$checksum_tmp"

escaped_path="$(json_escape "$INSTALL_PATH")"
receipt_tmp="$(mktemp "${BIN_DIR}/.notegate-cli-receipt.XXXXXX")"
cat > "$receipt_tmp" <<EOF
{
  "schema_version": 1,
  "managed_by": "notegate-cli-installer",
  "repository": "${REPOSITORY}",
  "install_path": "${escaped_path}",
  "target": "${TARGET}",
  "installed_version": "${VERSION}"
}
EOF
mv "$receipt_tmp" "$RECEIPT_PATH"
trap - EXIT INT TERM

path_on_path=false
case ":${PATH}:" in
  *:"${BIN_DIR}":*) path_on_path=true ;;
esac

if [ "$path_on_path" = true ]; then
  printf '{"status":"installed","path":"%s","target":"%s","version":"%s","path_on_path":true,"hint":null}\n' "$INSTALL_PATH" "$TARGET" "$VERSION"
else
  hint="$(json_escape "add ${BIN_DIR} to PATH before running notegate-cli")"
  printf '{"status":"installed","path":"%s","target":"%s","version":"%s","path_on_path":false,"hint":"%s"}\n' "$INSTALL_PATH" "$TARGET" "$VERSION" "$hint"
fi
