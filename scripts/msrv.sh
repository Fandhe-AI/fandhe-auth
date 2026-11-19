#!/usr/bin/env bash
# MSRV（ルートの Cargo.toml の [workspace.package] rust-version）を標準出力へ 1 行で出す。
# CI の build-test・lint が入れる toolchain の版をここから読み、版の文字列を Cargo.toml の 1 か所に保つ
# （D-2026-11-20-12）。呼び出し元: .github/workflows/ci.yml。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"

version="$(sed -n 's/^rust-version = "\([0-9][0-9.]*\)".*/\1/p' "${ROOT_DIR}/Cargo.toml")"
if [ -z "${version}" ] || [ "$(printf '%s\n' "${version}" | wc -l)" -ne 1 ]; then
  echo "error: could not read a single rust-version from Cargo.toml" >&2
  exit 1
fi
printf '%s\n' "${version}"
