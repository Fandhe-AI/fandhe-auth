#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 9（TASK-6）。shellcheck-staged.sh の並列数・同時実行の上限の回帰テスト。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が、偽の shellcheck と道具は test-hooks-sc-lib.sh が持つ。part の間で状態は引き継がない。
# part 8 から分けた（検査時間は行数にほぼ比例するため、part を 300〜440 行程度に保つ。.claude/rules/git-hooks.md）。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

cd "${work_dir}"

# shellcheck source=./test-hooks-sc-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-sc-lib.sh"

# 並列数の決め方: getconf の出力からバッチ数（= shellcheck の起動の回数）が決まる。20 件なので期待は min(上限, 20)
sc_expect_batches 3 3 "shellcheck-staged: getconf 3 gives 3 batches (the value is used, not the fixed 4)"
sc_expect_batches 1 1 "shellcheck-staged: getconf 1 gives a single start with every file"
sc_expect_batches 16 16 "shellcheck-staged: getconf 16 gives 16 batches (the cap itself)"
sc_expect_batches 17 16 "shellcheck-staged: getconf 17 is capped at 16 batches"
sc_expect_batches 40 16 "shellcheck-staged: getconf 40 is capped at 16 batches"
sc_expect_batches 08 8 "shellcheck-staged: getconf 08 is read as decimal 8"
sc_expect_batches 016 16 "shellcheck-staged: getconf 016 (3 digits) is read as a number"
sc_expect_batches 0016 4 "shellcheck-staged: getconf 0016 (4 digits) falls back to 4"
sc_expect_batches 0 4 "shellcheck-staged: getconf 0 falls back to 4"
sc_expect_batches "" 4 "shellcheck-staged: an empty getconf output falls back to 4"
sc_expect_batches abc 4 "shellcheck-staged: a non-numeric getconf output (abc) falls back to 4"
sc_expect_batches -3 4 "shellcheck-staged: a negative getconf output falls back to 4"
sc_expect_batches 2x 4 "shellcheck-staged: a getconf output with trailing characters falls back to 4"
sc_getconf_rc=1
sc_expect_batches 12 4 "shellcheck-staged: the output of a failing getconf is not used (falls back to 4)"

# 並列に進むこと: 障壁（バッチ数と同じ）で全バッチの開始を揃える。直列に起動する実装では、最初の 1 件が誰も来ない障壁で待ち、timeout が残る
sc_sync_dir="${work_dir}/sc-sync"
sc_jobs=4
sc_barrier=4
sc_sync_reset
sc_run p1.sh p2.sh p3.sh p4.sh
sc_seen="$(sc_max_seen)"
sc_tmo="$(sc_timeouts)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "4" ] && [ "${sc_tmo}" = "0" ] && [ -n "${sc_seen}" ] && [ "${sc_seen}" != "bad" ] && [ "${sc_seen}" -ge 2 ]; then
  pass "shellcheck-staged: 4 batches run at the same time (not one after another)"
else
  fail "shellcheck-staged: 4 batches run at the same time (not one after another)" "exit ${sc_rc}, calls: $(sc_ncalls), max seen: ${sc_seen:-none}, timeouts: ${sc_tmo}" "output: $(cat "${work_dir}/out")"
fi

# 件数が上限より少ないときは、件数と同じ数のバッチが同時に進む
sc_jobs=8
sc_barrier=2
sc_sync_reset
sc_run q1.sh q2.sh
sc_tmo="$(sc_timeouts)"
sc_seen="$(sc_max_seen)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "2" ] && [ "${sc_tmo}" = "0" ] && [ "${sc_seen}" = "2" ]; then
  pass "shellcheck-staged: 2 files with 8 jobs start 2 batches at the same time"
else
  fail "shellcheck-staged: 2 files with 8 jobs start 2 batches at the same time" "exit ${sc_rc}, calls: $(sc_ncalls), max seen: ${sc_seen:-none}, timeouts: ${sc_tmo}" "output: $(cat "${work_dir}/out")"
fi

# 同時実行の上限: 12 件・上限 3 なら、同時に 3 つ進み、3 を超えない（上限なしでファイルごとに起動する実装では 12 になる）
sc_jobs=3
sc_barrier=3
sc_many=()
for sc_i in 1 2 3 4 5 6 7 8 9 10 11 12; do
  sc_many+=("g${sc_i}.sh")
done
sc_sync_reset
sc_run "${sc_many[@]}"
sc_seen="$(sc_max_seen)"
sc_tmo="$(sc_timeouts)"
sc_want_args="$(printf '%s\n' "${sc_many[@]}" | sort)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "3" ] && [ "$(sc_files)" = "${sc_want_args}" ] && [ "${sc_tmo}" = "0" ] && [ "${sc_seen}" = "3" ]; then
  pass "shellcheck-staged: 12 files with 3 jobs run exactly 3 at a time"
else
  fail "shellcheck-staged: 12 files with 3 jobs run exactly 3 at a time" "exit ${sc_rc}, calls: $(sc_ncalls), max seen: ${sc_seen:-none}, timeouts: ${sc_tmo}" "output: $(cat "${work_dir}/out")"
fi

# 上限 16 は同時実行の数でも守られる（getconf 40・20 件でも 16 を超えない）
sc_jobs=40
sc_barrier=16
sc_sync_reset
sc_run "${sc_twenty[@]}"
sc_seen="$(sc_max_seen)"
sc_tmo="$(sc_timeouts)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "16" ] && [ "${sc_tmo}" = "0" ] && [ -n "${sc_seen}" ] && [ "${sc_seen}" != "bad" ] && [ "${sc_seen}" -le 16 ] && [ "${sc_seen}" -ge 2 ]; then
  pass "shellcheck-staged: getconf 40 with 20 files runs at most 16 at a time"
else
  fail "shellcheck-staged: getconf 40 with 20 files runs at most 16 at a time" "exit ${sc_rc}, calls: $(sc_ncalls), max seen: ${sc_seen:-none}, timeouts: ${sc_tmo}" "output: $(cat "${work_dir}/out")"
fi
sc_jobs=8
sc_barrier=""
sc_sync_dir=""

finish "part 9"
