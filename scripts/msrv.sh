#!/usr/bin/env bash
# MSRV（workspace の rust-version）を標準出力へ 1 行で出す。
# 使い方: scripts/msrv.sh [MANIFEST]（MANIFEST の既定はルートの Cargo.toml。試験用に差し替えられる）。
# 版の文字列を Cargo.toml の [workspace.package] の 1 か所に保つ（D-2026-11-20-12）。呼び出し元: scripts/install-msrv.sh。
# 版の正は 2 か所を照合する。(1) ルートの [workspace.package].rust-version（python3 標準の tomllib で TOML の
# 構造どおりに読む。新しい依存は足さない。複数行の文字列・コメント・表記の違いに左右されない）、
# (2) 全 workspace メンバーの rust_version（cargo metadata --no-deps の解決結果）。(2) が 1 種類に揃い、
# かつ (1) と文字列で一致するときだけ出す。ルートに値が無い・メンバーに rust-version が無い・版が食い違う
# （全メンバーが同じ別の版を書いた場合を含む）・読めない場合は終了コード 2（検査不能。環境・設定の誤り。
# fail-closed）で止める。cargo・jq・python3 が無いときも 2。
# 出力は Cargo.toml の表記のまま（`1.100` のように 2 部分のことがある）。パッチ版の補いは install-msrv.sh が行う。
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
require_cmd cargo || exit 2
require_cmd jq || exit 2
require_cmd python3 || exit 2

# ルートの [workspace.package].rust-version を tomllib で読む。-I は環境変数・カレントのモジュールを読まない。
# 読めない・文字列でない場合は出力せず非 0 で終わる（stderr は捨て、値を出さない）。
if ! root_version="$(python3 -I -c '
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    v = tomllib.load(f)["workspace"]["package"]["rust-version"]
if not isinstance(v, str):
    sys.exit(1)
print(v)
' "${manifest}" 2> /dev/null)"; then
  echo "error: cannot read [workspace.package].rust-version from: ${manifest}" >&2
  exit 2
fi

# 失敗（cargo の誤り・JSON の不正）を空の結果と取り違えないよう、代入の終了状態を確かめる。
if ! metadata="$(cargo metadata --no-deps --format-version 1 --manifest-path "${manifest}" 2> /dev/null)"; then
  echo "error: cargo metadata failed for: ${manifest}" >&2
  exit 2
fi
# 全メンバーの rust_version を重複を除いて集める。null（rust-version 無し）が混ざれば 2 種類以上になり失敗する。
if ! versions="$(printf '%s' "${metadata}" | jq -r '[.packages[].rust_version] | unique | .[] | if . == null then "null" else . end')"; then
  echo "error: could not read cargo metadata" >&2
  exit 2
fi
version="${versions}"
if [ -z "${version}" ] || [ "$(printf '%s\n' "${versions}" | wc -l)" -ne 1 ] || [ "${version}" = "null" ] \
  || ! [[ "${version}" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]]; then
  echo "error: workspace members do not share a single rust-version" >&2
  exit 2
fi
if [ "${version}" != "${root_version}" ]; then
  echo "error: members' rust-version differs from [workspace.package].rust-version" >&2
  exit 2
fi
printf '%s\n' "${version}"
