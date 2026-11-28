#!/usr/bin/env bash
# 非破壊の静的チェック。ソース・設定は変更しない。
# - editorconfig-checker: .editorconfig の宣言（改行・インデント・末尾空白等）への準拠
# - shellcheck: scripts/・dev/ 配下のシェルスクリプト
# - 暗号の基本処理の自作の走査（TASK-715・LIB-21。scripts/check-crypto-primitives.sh。外す引数・環境変数は無い）
# ツールが無い場合は「未検査で成功」にせず失敗させる。
# 言語固有のチェック（fmt --check・lint・test 等）は派生リポジトリで追記する。
set -euo pipefail
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

log "crypto-scan (scripts/check-crypto-primitives.sh)"
"${DIR}/check-crypto-primitives.sh"
