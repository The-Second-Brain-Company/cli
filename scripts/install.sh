#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' 'Usage: install.sh [--release | --source | --from-file PATH]' '' 'Install the official release by default, build current working files with --source, or install an existing Cortex executable.' 'BIN_DIR sets the destination directory (default: ~/.local/bin).' 'CORTEX_VERSION pins an official release instead of downloading the latest version.'
}

temporary=""
download_dir=""
cleanup() {
  if [[ -n "$temporary" ]]; then
    rm -f -- "$temporary"
  fi
  if [[ -n "$download_dir" ]]; then
    rm -rf -- "$download_dir"
  fi
}
trap cleanup EXIT

install_binary() {
  local binary="$1" expected_version="${2:-}" bin_dir destination version resolved
  if [[ ! -f "$binary" || ! -x "$binary" ]]; then
    printf '%s\n' 'The source must be an executable Cortex binary.' >&2
    exit 1
  fi
  bin_dir="${BIN_DIR:-${HOME:?Set HOME or BIN_DIR}/.local/bin}"
  mkdir -p -- "$bin_dir"
  bin_dir="$(cd -- "$bin_dir" && pwd)"
  destination="$bin_dir/cortex"
  if [[ -d "$destination" ]]; then
    printf '%s\n' "Cannot replace a directory at $destination." >&2
    exit 1
  fi
  temporary="$(mktemp "$bin_dir/.cortex-install.XXXXXX")"
  cp -- "$binary" "$temporary"
  chmod 755 "$temporary"
  version="$("$temporary" --version)"
  case "$version" in
    'cortex '*) ;;
    *) printf '%s\n' 'The source did not identify itself as Cortex; the installed version was preserved.' >&2; exit 1 ;;
  esac
  if [[ -n "$expected_version" && "$version" != "cortex $expected_version" ]]; then
    printf '%s\n' 'The downloaded CLI does not match the release version.' >&2
    exit 1
  fi
  mv -f -- "$temporary" "$destination"
  temporary=""
  printf '%s\n' "Installed $version to $destination"
  case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) printf 'Add the install directory to your shell and agent PATH: export PATH=%q:"$PATH"\n' "$bin_dir" ;;
  esac
  resolved="$(command -v cortex || true)"
  if [[ -n "$resolved" && "$resolved" != "$destination" ]]; then
    printf '%s\n' "Another cortex appears first on PATH: $resolved. Put $bin_dir before that directory."
  fi
}

case "$#:${1:-}" in
  0:|1:--release) mode="release" ;;
  1:--source) mode="source" ;;
  2:--from-file) install_binary "$2"; exit 0 ;;
  1:--help|1:-h) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

if [[ "$mode" == "source" ]]; then
  source_dir="${CORTEX_CLI_SOURCE_DIR:-}"
  if [[ -z "$source_dir" ]]; then
    if [[ -z "${BASH_SOURCE[0]:-}" || ! -f "${BASH_SOURCE[0]}" ]]; then
      printf '%s\n' 'Building from source requires CORTEX_CLI_SOURCE_DIR or a saved installer in the CLI source directory.' >&2
      exit 1
    fi
    source_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
  if [[ ! -f "$source_dir/Cargo.toml" || ! -f "$source_dir/mise.toml" ]]; then
    printf '%s\n' 'The CLI source directory must contain Cargo.toml and mise.toml.' >&2
    exit 1
  fi
  if ! command -v mise >/dev/null 2>&1; then
    printf '%s\n' 'Install mise first: https://mise.jdx.dev/' >&2
    exit 1
  fi
  exec mise -C "$source_dir" run install
fi

case "$(uname -s):$(uname -m)" in
  Darwin:arm64) platform="aarch64-apple-darwin" ;;
  Darwin:x86_64) platform="x86_64-apple-darwin" ;;
  Linux:aarch64|Linux:arm64) platform="aarch64-unknown-linux-gnu" ;;
  Linux:x86_64|Linux:amd64) platform="x86_64-unknown-linux-gnu" ;;
  *) printf '%s\n' 'No CLI release for this platform. Use the Cortex plugin MCP connection.' >&2; exit 1 ;;
esac

if [[ "${CORTEX_VERSION+x}" == x ]]; then
  release_version="$CORTEX_VERSION"
else
  release_version="$(curl --fail --show-error --silent --location https://github.com/The-Second-Brain-Company/cli/releases/latest/download/latest.txt)"
fi
version_core='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
prerelease_identifier='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
version_pattern="^$version_core(-$prerelease_identifier(\.$prerelease_identifier)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$"
if [[ ! "$release_version" =~ $version_pattern ]]; then
  printf '%s\n' 'The release version must be a valid semantic version.' >&2
  exit 1
fi

release_url="https://github.com/The-Second-Brain-Company/cli/releases/download/v$release_version"
download_dir="$(mktemp -d)"
curl --fail --show-error --silent --location "$release_url/cortex-$platform" --output "$download_dir/cortex-$platform"
curl --fail --show-error --silent --location "$release_url/cortex-$platform.sha256" --output "$download_dir/cortex-$platform.sha256"
checksum_record="$(cat -- "$download_dir/cortex-$platform.sha256")"
checksum_pattern="^[0-9a-fA-F]{64}  cortex-$platform$"
if [[ ! "$checksum_record" =~ $checksum_pattern ]]; then
  printf '%s\n' 'The checksum file does not identify the downloaded CLI.' >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  (cd -- "$download_dir" && sha256sum --check "cortex-$platform.sha256")
else
  (cd -- "$download_dir" && shasum --algorithm 256 --check "cortex-$platform.sha256")
fi
chmod 755 "$download_dir/cortex-$platform"
install_binary "$download_dir/cortex-$platform" "$release_version"
