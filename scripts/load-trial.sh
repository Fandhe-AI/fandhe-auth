#!/usr/bin/env bash
# 計測・負荷のハーネス（crates/load-harness）の小さな試し実行だけを流す入口（TASK-12 10/10）。
# 呼び出し元: `make load-trial`。CI では流さない（同じ試験 trial_run は `make test` に入っている）。
# 負荷の記録は成功時に消えるため、--show-output で記録の本文を標準出力に出す（試験が設定の目印 2 種と出力先のパスの無いことを確かめ、出す本文そのものを秘密値の各表現の照合に通した本文だけ。一致があれば本文を出さず失敗する）。
# 本文の公開は、この入口が環境変数 FANDHE_AUTH_LOAD_TRIAL_PRINT=1 を試験へ渡したときだけ（通常の `make test` では出ない。SECR-23）。
# 終了コード: 0 = 主試験 1 件が流れて合格、1 = 失敗または主試験の合格が 1 件（無視 0 件）でない、2 = 誤用（引数）。
# stub の結果を実物の結果に数えない（A-2026-10-28-9・ACPT-7）。この入口自身は環境変数・設定ファイルを読まない（DPOL-32）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd grep
cd "${ROOT_DIR}"

# 記録を生成する主試験を完全一致で選んで流し、実行結果で合格がちょうど 1 件（無視 0 件）であることを確かめてから成功を返す（AUDB-6 の向き）。
# `--list` は #[ignore] の試験も列挙するため、件数の確認には使わない
main_test="small_trial_run_writes_record_without_requirement_values"
rc=0
out="$(FANDHE_AUTH_LOAD_TRIAL_PRINT=1 cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --exact "${main_test}" --show-output)" || rc=$?
# 試験の失敗は契約どおり 1 に揃える（cargo の 101 等を伝播しない）。
# 失敗時は捕捉した出力（libtest の診断・記録の本文を含みうる）を再掲しない。照合を通っていない本文を出さない（fail-closed。SECR-23）。
# コンパイルの診断などは cargo が標準エラーへ直接出す
if [ "${rc}" -ne 0 ]; then
  echo "error: load trial test failed (the captured test output is not printed; run the test directly to inspect)" >&2
  exit 1
fi
if ! printf '%s\n' "${out}" | grep -Eq '^test result: ok\. 1 passed; 0 failed; 0 ignored;'; then
  # 件数の行（libtest の固定の形）だけを出し、本文は出さない
  printf '%s\n' "${out}" | grep -E '^test result:' >&2 || true
  echo "error: expected exactly one passed load trial main test (0 ignored)" >&2
  exit 1
fi
printf '%s\n' "${out}"
log "the load record body is printed above (the record file is removed on success; stub results are not real results)"
