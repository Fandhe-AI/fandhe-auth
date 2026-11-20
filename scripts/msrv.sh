#!/usr/bin/env bash
# MSRV（Cargo.toml の [workspace.package] の rust-version）を標準出力へ 1 行で出す。
# 使い方: scripts/msrv.sh [MANIFEST]（MANIFEST の既定はルートの Cargo.toml。試験用に差し替えられる）。
# 版の文字列を Cargo.toml の 1 か所に保つ（D-2026-11-20-12）。呼び出し元: scripts/install-msrv.sh。
# [workspace.package] の表の中だけを読む（他の表の rust-version を拾わない）。`rust-version="1.100"` のように
# 空白が無い書き方・行末コメントも取る。1 件も取れない・2 件以上は終了コード 2（検査不能）で止める。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

if [ "$#" -gt 1 ]; then
  echo "usage: $(basename "$0") [MANIFEST]" >&2
  exit 2
fi
manifest="${1:-${ROOT_DIR}/Cargo.toml}"
if [ ! -r "${manifest}" ]; then
  echo "error: cannot read manifest: ${manifest}" >&2
  exit 2
fi

version="$(awk '
  /^[[:space:]]*\[/ { in_table = ($0 ~ /^[[:space:]]*\[workspace\.package\][[:space:]]*(#.*)?$/); next }
  in_table && match($0, /^[[:space:]]*rust-version[[:space:]]*=[[:space:]]*"[0-9]+(\.[0-9]+)*"/) {
    v = substr($0, RSTART, RLENGTH)
    sub(/^[^"]*"/, "", v)
    sub(/"$/, "", v)
    print v
  }
' "${manifest}")" || {
  echo "error: failed to parse the manifest" >&2
  exit 2
}
if [ -z "${version}" ] || [ "$(printf '%s\n' "${version}" | wc -l)" -ne 1 ]; then
  echo "error: could not read a single rust-version from [workspace.package]" >&2
  exit 2
fi
printf '%s\n' "${version}"
