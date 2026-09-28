#!/usr/bin/env bash
set -euo pipefail

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

exec mise -C "$source_dir" run install "$@"
