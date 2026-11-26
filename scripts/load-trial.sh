#!/usr/bin/env bash
# 計測・負荷のハーネス（crates/load-harness）の小さな試し実行だけを流す入口（TASK-12 10/10）。
# 呼び出し元: `make load-trial`。CI では流さない（同じ試験 trial_run は `make test` に入っている）。
# 負荷の記録は成功時に消えるため、--show-output で記録の本文を標準出力に出す（試験が秘密値・設定の目印・パスの無いことを確かめた本文だけ）。
# 本文の公開は、この入口が環境変数 FANDHE_AUTH_LOAD_TRIAL_PRINT=1 を試験へ渡したときだけ（通常の `make test` では出ない。SECR-23）。
# 終了コード: 0 = 主試験 1 件が流れて合格、1 = 失敗または主試験が 1 件でない、2 = 誤用（引数）。
# stub の結果を実物の結果に数えない（A-2026-10-28-9・ACPT-7）。この入口自身は環境変数・設定ファイルを読まない（DPOL-32）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd grep
cd "${ROOT_DIR}"

# 記録を生成する主試験を完全一致で選ぶ。一覧と実行に同じ選択条件を使い、実行される試験が 1 件であることを確かめる（AUDB-6 の向き）
main_test="small_trial_run_writes_record_without_requirement_values"
listed="$(cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --exact "${main_test}" --list)" || {
  echo "error: failed to list load trial tests" >&2
  exit 1
}
rc=0
count="$(printf '%s\n' "${listed}" | grep -cFx "${main_test}: test")" || rc=$?
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
if [ "${count}" -ne 1 ]; then
  echo "error: expected exactly one load trial main test, found ${count}" >&2
  exit 1
fi

# 試験の失敗は契約どおり 1 に揃える（cargo の 101 等を伝播しない）
FANDHE_AUTH_LOAD_TRIAL_PRINT=1 cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --exact "${main_test}" --show-output || exit 1
log "the load record body is printed above (the record file is removed on success; stub results are not real results)"
