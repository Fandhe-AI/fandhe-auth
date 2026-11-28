#!/usr/bin/env bash
# 非破壊の静的チェック。ソース・設定は変更しない。
# - editorconfig-checker: .editorconfig の宣言（改行・インデント・末尾空白等）への準拠
# - shellcheck: scripts/・dev/ 配下のシェルスクリプト
# ツールが無い場合は「未検査で成功」にせず失敗させる。
# 言語固有のチェック（fmt --check・lint・test 等）は派生リポジトリで追記する。
set -euo pipefail
# 相対パスの cd が CDPATH に左右されない（CDPATH=. で cd が移動先を標準出力へ出し DIR が 2 行になる）ように、DIR の解決より前に外す（#6173）。
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd editorconfig-checker
require_cmd shellcheck

cd "${ROOT_DIR}"

log "editorconfig-checker (exclusions: .editorconfig-checker.json)"
editorconfig-checker

log "shellcheck scripts/**/*.sh dev/**/*.sh"
find scripts dev -type f -name '*.sh' -print0 | xargs -0 shellcheck
