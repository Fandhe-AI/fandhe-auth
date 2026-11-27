#!/usr/bin/env bash
# 計測・負荷のハーネス（crates/load-harness）の小さな試し実行だけを流す入口（TASK-12 10/10）。
# 呼び出し元: `make load-trial`。CI では流さない（同じ試験 trial_run は `make test` に入っている）。
# 負荷の記録は成功時に消えるため、--show-output で記録の本文を標準出力に出す（試験が設定の目印 2 種と出力先のパスの無いことを確かめ、出す本文そのものを秘密値の各表現の照合に通した本文だけ。一致があれば本文を出さず失敗する）。
# 本文の公開は、#[ignore] の専用の試験（print_checked_load_record_body）をこの入口が `--ignored --exact` で選んだときだけ（主試験は環境変数に依らず本文を出さない。通常の `make test` では出ない。SECR-23・DPOL-32）。
# 子の標準エラーは /dev/null へ捨てて出さない（一時ファイルを作らない。SECR-23）。本文は試験が照合後に固定の開始行・終了行で挟んで出す。この入口は、集計行が合格 1 件（無視 0 件）かつ区画がちょうど 1 件で、開始行が終了行より前にあり、区画が空でなく記録の形式の行を含むときだけ区画の中身を出す（満たさなければ固定の文言で 1。本文は出さない）。
# 終了コード: 0 = 主試験 1 件が流れて合格、1 = 失敗または主試験の合格が 1 件（無視 0 件）でない、2 = 誤用（引数）。
# stub の結果を実物の結果に数えない（A-2026-10-28-9・ACPT-7）。この入口自身は環境変数・設定ファイルを読まない（DPOL-32。子の RUST_TEST_NOCAPTURE の解除だけ。一時ファイルを作らないため TMPDIR にも依らない）。
set -euo pipefail
# 相対パスの cd が CDPATH に左右されない（CDPATH=. で cd が移動先を標準出力へ出し DIR が 2 行になる）ように、DIR の解決より前に外す。
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd grep
require_cmd cut
require_cmd awk
require_cmd env
cd "${ROOT_DIR}"

# 本文を出す専用の試験（#[ignore]。主試験と同じ試行・照合を使う）を `--ignored --exact` で選んで流し、実行結果で合格がちょうど 1 件であることを確かめてから成功を返す（AUDB-6 の向き）。
# 集計行の `0 ignored` は、選んだ試験が無視されず流れたことを表す（`--ignored` では選んだ #[ignore] の試験が実行される）
main_test="print_checked_load_record_body"
rc=0
# 子の標準エラーは /dev/null へ捨てる。コマンド置換が捕まえるのは標準出力だけで、試験・依存が標準エラーへ直接書いた診断は libtest の捕捉を通らずに端末へ出る。
# 一時ファイルへ集めると走査されない出力先ができ、強制終了では残るため、ファイルを作らない（SECR-23。標準エラーは本文を出してよい条件を満たさない）
# RUST_TEST_NOCAPTURE が有効だと試験の標準出力・標準エラーが捕捉されず、照合を通っていない出力が直接流れる。
# 継承を外して libtest の捕捉（--show-output の対象は標準出力だけでなく標準エラーも含む）を保つ（SECR-23）
out="$(env -u RUST_TEST_NOCAPTURE cargo test -p fandhe-auth-load-harness --test trial_run --locked -- --ignored --exact "${main_test}" --show-output 2>/dev/null)" || rc=$?
# 試験の失敗は契約どおり 1 に揃える（cargo の 101 等を伝播しない）。
# 失敗時は捕捉した標準出力・標準エラー（libtest の診断・コンパイルの診断・記録の本文を含みうる）を再掲しない。照合を通っていない本文を出さない（fail-closed。SECR-23）。
# 診断が要るときは `env -u RUST_TEST_NOCAPTURE cargo test -p fandhe-auth-load-harness --test trial_run --locked` を直接流す（本文を出す試験は #[ignore] で、`--ignored` を付けない限り流れない）
if [ "${rc}" -ne 0 ]; then
  echo "error: load trial test failed (the output is not printed; run the test directly to inspect)" >&2
  exit 1
fi
if ! grep -Eq '^test result: ok\. 1 passed; 0 failed; 0 ignored;' <<<"${out}"; then
  # 捕捉した出力は、同じ接頭辞で始まる本文の行を含みうるため一切再掲しない（固定の文言だけ。SECR-23）
  echo "error: expected exactly one passed load trial main test (0 ignored)" >&2
  exit 1
fi
# 公開するのは、試験が照合（printable_body）を通した後に固定の行で挟んで出した本文の区画だけ。区画がちょうど 1 件でなければ何も出さず失敗する
# （本文を出す試験が選ばれなかった場合に、記録が無いまま 0 を返さない）。区画の外の出力は公開しない。
# 文字列は crates/load-harness/tests/trial_run.rs の PRINT_BEGIN・PRINT_END と一致させる
begin_line="-----BEGIN LOAD RECORD BODY-----"
end_line="-----END LOAD RECORD BODY-----"
n_begin="$(grep -cFx -- "${begin_line}" <<<"${out}" || true)"
n_end="$(grep -cFx -- "${end_line}" <<<"${out}" || true)"
if [ "${n_begin}" != "1" ] || [ "${n_end}" != "1" ]; then
  echo "error: expected exactly one checked load record body block (begin=${n_begin} end=${n_end}); nothing is printed" >&2
  exit 1
fi
# 開始行が終了行より前にあること（順序が逆なら区画として扱わない）
begin_no="$(grep -nFx -- "${begin_line}" <<<"${out}" | cut -d: -f1)" || {
  echo "error: could not locate the load record body block; nothing is printed" >&2
  exit 1
}
end_no="$(grep -nFx -- "${end_line}" <<<"${out}" | cut -d: -f1)" || {
  echo "error: could not locate the load record body block; nothing is printed" >&2
  exit 1
}
# 通ってよい側（開始 < 終了）から書き、比較そのものが誤ったときも失敗の側に落とす
if ! [ "${begin_no}" -lt "${end_no}" ]; then
  echo "error: the load record body block is out of order (end before begin); nothing is printed" >&2
  exit 1
fi
block="$(awk -v b="${begin_line}" -v e="${end_line}" '$0 == e {on = 0} on {print} $0 == b {on = 1}' <<<"${out}")"
if [ -z "${block}" ] || ! grep -q '^format: fandhe-auth-load-record/' <<<"${block}"; then
  echo "error: the load record body block is empty or malformed; nothing is printed" >&2
  exit 1
fi
printf '%s\n' "${block}"
log "the load record body is printed above (the record file is removed on success; stub results are not real results)"
