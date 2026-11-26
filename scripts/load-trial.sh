#!/usr/bin/env bash
# 計測・負荷のハーネス（crates/load-harness）の小さな試し実行だけを流す入口（TASK-12 10/10）。
# 呼び出し元: `make load-trial`。CI では流さない（同じ試験 trial_run は `make test` に入っている）。
# 負荷の記録は成功時に消えるため、--show-output で記録の本文を標準出力に出す（試験が秘密値・設定の目印・パスの無いことを確かめた本文だけ）。
# 終了コード: 0 = 1 件以上が流れて全合格、1 = 失敗または一致 0 件、2 = 誤用（引数）。
# stub の結果を実物の結果に数えない（A-2026-10-28-9・ACPT-7）。環境変数・設定ファイルは読まない（DPOL-32）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd grep
cd "${ROOT_DIR}"

# 一致 0 件でも cargo test は 0 で終わるため、先に数えて 0 件なら失敗にする（AUDB-6 の向き）
listed="$(cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --list)" || {
  echo "error: failed to list load trial tests" >&2
  exit 1
}
rc=0
count="$(printf '%s\n' "${listed}" | grep -c ': test$')" || rc=$?
if [ "${rc}" -gt 1 ]; then
  echo "error: failed to count load trial tests (grep exit ${rc})" >&2
  exit 1
fi
case "${count}" in
  '' | *[!0-9]*)
    echo "error: load trial test count is not a number" >&2
    exit 1
    ;;
esac
if [ "${count}" -lt 1 ]; then
  echo "error: no load trial test found" >&2
  exit 1
fi

# 試験の失敗は契約どおり 1 に揃える（cargo の 101 等を伝播しない）
cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --show-output || exit 1
log "the load record body is printed above (the record file is removed on success; stub results are not real results)"
