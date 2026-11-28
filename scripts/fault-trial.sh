#!/usr/bin/env bash
# 障害注入のハーネス（crates/it-harness の fault）の試し実行だけを流す入口（TASK-13 8/8）。
# 呼び出し元: `make fault-trial`。CI では流さない（同じ試験 fault_trial の主試験は `make test` に入っている）。
# 障害注入の記録は成功時に消えるため、--show-output で基準線と注入ありの 2 つの本文を標準出力に出す（試験が行ごとの許可リストで本文を照合し、通った本文だけを出す。通らなければ本文を出さず失敗する）。
# 本文の公開は、#[ignore] の専用の試験（print_checked_fault_record_bodies）をこの入口が `--ignored --exact` で選んだときだけ（主試験は環境変数に依らず本文を出さない。通常の `make test` では出ない。SECR-23・DPOL-32）。
# 子の標準エラーは /dev/null へ捨てて出さない（一時ファイルを作らない。SECR-23）。本文は試験が照合後に固定の開始行・終了行で挟んで出す。この入口は、集計行が合格 1 件（無視 0 件）かつ区画がちょうど 1 件で、開始行が終了行より前にあり、区画が空でなく記録の形式の行 2 件と baseline・fault の行が 1 件ずつ（baseline が先）あるときだけ区画の中身を出す（満たさなければ固定の文言で 1。本文は出さない）。
# 終了コード: 0 = 本文を出す試験 1 件が流れて合格し、2 つの本文（baseline、fault の順）が区画に揃う、1 = 失敗または合格が 1 件（無視 0 件）でない・区画が不正、2 = 誤用（引数）。
# stub の結果を実物の結果に数えない（A-2026-10-28-9・ACPT-7）。この入口自身は環境変数・設定ファイルを読まない（DPOL-32。子の RUST_TEST_NOCAPTURE の解除だけ。一時ファイルを作らないため TMPDIR にも依らない）。
set -euo pipefail
# 相対パスの cd が CDPATH に左右されない（CDPATH=. で cd が移動先を標準出力へ出し DIR が 2 行になる）ように、DIR の解決より前に外す。
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd awk
require_cmd env
cd "${ROOT_DIR}"

# 本文を出す専用の試験（#[ignore]。主試験と同じ試行・照合を使う）を `--ignored --exact` で選んで流し、実行結果で合格がちょうど 1 件であることを確かめてから成功を返す（AUDB-6 の向き）。
# 集計行の `0 ignored` は、選んだ試験が無視されず流れたことを表す（`--ignored` では選んだ #[ignore] の試験が実行される）
main_test="print_checked_fault_record_bodies"
rc=0
# 子の標準エラーは /dev/null へ捨てる。コマンド置換が捕まえるのは標準出力だけで、試験・依存が標準エラーへ直接書いた診断は libtest の捕捉を通らずに端末へ出る。
# 一時ファイルへ集めると走査されない出力先ができ、強制終了では残るため、ファイルを作らない（SECR-23。標準エラーは本文を出してよい条件を満たさない）
# RUST_TEST_NOCAPTURE が有効だと試験の標準出力・標準エラーが捕捉されず、照合を通っていない出力が直接流れる。
# 継承を外して libtest の捕捉（--show-output の対象は標準出力だけでなく標準エラーも含む）を保つ（SECR-23）
out="$(env -u RUST_TEST_NOCAPTURE cargo test -p fandhe-auth-it-harness --test fault_trial --locked -- --ignored --exact "${main_test}" --show-output 2>/dev/null)" || rc=$?
# 試験の失敗は契約どおり 1 に揃える（cargo の 101 等を伝播しない）。
# 失敗時は捕捉した標準出力・標準エラー（libtest の診断・コンパイルの診断・記録の本文を含みうる）を再掲しない。照合を通っていない本文を出さない（fail-closed。SECR-23）。
# 診断が要るときは `env -u RUST_TEST_NOCAPTURE cargo test -p fandhe-auth-it-harness --test fault_trial --locked` を直接流す（本文を出す試験は #[ignore] で、`--ignored` を付けない限り流れない）
if [ "${rc}" -ne 0 ]; then
  echo "error: fault trial test failed (the output is not printed; run the test directly to inspect)" >&2
  exit 1
fi
# 捕捉した出力 ${out} は、here-string（一時ファイルへ書き出す実装がある。macOS 標準の bash 3.2 等。TMPDIR に依る）に渡さず、printf（組み込み）からパイプで awk へ渡す（SECR-23）。
# 抽出・集計の awk は入力を最後まで読む（途中で終わる grep -q・head をパイプの途中に置かない。pipefail の下で SIGPIPE を失敗と取り違えない）。
if ! printf '%s\n' "${out}" | awk '/^test result: ok\. 1 passed; 0 failed; 0 ignored;/ {found = 1} END {exit !found}'; then
  # 捕捉した出力は、同じ接頭辞で始まる本文の行を含みうるため一切再掲しない（固定の文言だけ。SECR-23）
  echo "error: expected exactly one passed fault trial print test (0 ignored)" >&2
  exit 1
fi
# 公開するのは、試験が照合（printable_body）を通した後に固定の行で挟んで出した本文の区画だけ。区画がちょうど 1 件でなければ何も出さず失敗する
# （本文を出す試験が選ばれなかった場合に、記録が無いまま 0 を返さない）。区画の外の出力は公開しない。
# 文字列は crates/it-harness/tests/fault_trial.rs の PRINT_BEGIN・PRINT_END と一致させる
begin_line="-----BEGIN FAULT RECORD BODIES-----"
end_line="-----END FAULT RECORD BODIES-----"
# 行全体の一致（固定文字列）の件数。入力が空でも 0 を出す
count_line() {
  printf '%s\n' "${out}" | awk -v l="$1" '$0 == l {n++} END {print n + 0}'
}
# 行全体の一致の最初の行番号。無ければ空
first_line_no() {
  printf '%s\n' "${out}" | awk -v l="$1" '$0 == l && !seen {print NR; seen = 1}'
}
n_begin="$(count_line "${begin_line}")"
n_end="$(count_line "${end_line}")"
if [ "${n_begin}" != "1" ] || [ "${n_end}" != "1" ]; then
  echo "error: expected exactly one checked fault record bodies block (begin=${n_begin} end=${n_end}); nothing is printed" >&2
  exit 1
fi
# 開始行が終了行より前にあること（順序が逆なら区画として扱わない）
begin_no="$(first_line_no "${begin_line}")"
end_no="$(first_line_no "${end_line}")"
if [ -z "${begin_no}" ] || [ -z "${end_no}" ]; then
  echo "error: could not locate the fault record bodies block; nothing is printed" >&2
  exit 1
fi
# 通ってよい側（開始 < 終了）から書き、比較そのものが誤ったときも失敗の側に落とす
if ! [ "${begin_no}" -lt "${end_no}" ]; then
  echo "error: the fault record bodies block is out of order (end before begin); nothing is printed" >&2
  exit 1
fi
block="$(printf '%s\n' "${out}" | awk -v b="${begin_line}" -v e="${end_line}" '$0 == e {on = 0} on {print} $0 == b {on = 1}')"
# 区画の中に、記録の形式の行がちょうど 2 件、`run_mode: baseline` と `run_mode: fault` の行がそれぞれちょうど 1 件で、baseline が先であること
# （基準線と注入ありの片方が欠けたまま 0 を返さない）。awk は入力を最後まで読む。
if [ -z "${block}" ] || ! printf '%s\n' "${block}" | awk '
  /^format: fandhe-auth-it-fault-record\// {f++}
  /^run_mode: baseline$/ {b++; if (!bn) bn = NR}
  /^run_mode: fault$/ {m++; if (!mn) mn = NR}
  END {exit !(f == 2 && b == 1 && m == 1 && bn < mn)}'; then
  echo "error: the fault record bodies block is empty or malformed; nothing is printed" >&2
  exit 1
fi
printf '%s\n' "${block}"
log "the fault record bodies are printed above (baseline, then fault; the record files are removed on success; stub results are not real results)"
