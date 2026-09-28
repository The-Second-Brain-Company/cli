#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  printf '%s\n' 'Usage: install.sh [--from-file PATH]' '' 'Build and install current working files, or install an existing Brain executable.' 'BIN_DIR sets the destination directory (default: ~/.local/bin).'
  exit 0
fi

if [[ "${1:-}" == "--from-file" && "$#" == 2 ]]; then
  binary="$2"
  if [[ ! -f "$binary" || ! -x "$binary" ]]; then
    printf '%s\n' 'The source must be an executable Brain binary.' >&2
    exit 1
  fi
  bin_dir="${BIN_DIR:-${HOME:?Set HOME or BIN_DIR}/.local/bin}"
  mkdir -p -- "$bin_dir"
  bin_dir="$(cd -- "$bin_dir" && pwd)"
  destination="$bin_dir/brain"
  if [[ -d "$destination" ]]; then
    printf '%s\n' "Cannot replace a directory at $destination." >&2
    exit 1
  fi
  temporary="$(mktemp "$bin_dir/.brain-install.XXXXXX")"
  trap 'rm -f -- "$temporary"' EXIT
  cp -- "$binary" "$temporary"
  chmod 755 "$temporary"
  version="$("$temporary" --version)"
  case "$version" in
    'brain '*) ;;
    *) printf '%s\n' 'The source did not identify itself as Brain; the installed version was preserved.' >&2; exit 1 ;;
  esac
  mv -f -- "$temporary" "$destination"
  printf '%s\n' "Installed $version to $destination"
  case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) printf 'Add the install directory to your shell and agent PATH: export PATH=%q:"$PATH"\n' "$bin_dir" ;;
  esac
  resolved="$(command -v brain || true)"
  if [[ -n "$resolved" && "$resolved" != "$destination" ]]; then
    printf '%s\n' "Another brain appears first on PATH: $resolved. Put $bin_dir before that directory."
  fi
  exit 0
fi

if [[ "$#" != 0 ]]; then
  printf '%s\n' 'Usage: install.sh [--from-file PATH]' >&2
  exit 2
fi

source_dir="${BRAIN_CLI_SOURCE_DIR:-}"
if [[ -z "$source_dir" ]]; then
  if [[ -z "${BASH_SOURCE[0]:-}" || ! -f "${BASH_SOURCE[0]}" ]]; then
    printf '%s\n' 'This preview requires a local CLI checkout. Set BRAIN_CLI_SOURCE_DIR to its absolute path, or run bash scripts/install.sh from the checkout.' >&2
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
