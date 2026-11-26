#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 8（TASK-6）。shellcheck-staged.sh の回帰テスト。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
# part 1 から分けた（検査時間は行数にほぼ比例するため、part を 300〜440 行程度に保つ。.claude/rules/git-hooks.md）。
# 道具（偽の shellcheck・helper）は test-hooks-sc-lib.sh に分けた（part 9 と共有。part を 300〜440 行程度に保つ。.claude/rules/git-hooks.md）。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

cd "${work_dir}"

# shellcheck source=./test-hooks-sc-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-sc-lib.sh"

sc_run
if [ "${sc_rc}" -eq 0 ] && [ ! -s "${sc_log}" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: no arguments exits 0 without starting shellcheck"
else
  fail "shellcheck-staged: no arguments exits 0 without starting shellcheck" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

sc_run a.sh b.sh c.sh
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_files)" = "$(printf '%s\n' a.sh b.sh c.sh)" ] && [ "$(sc_ncalls)" = "$(sc_dashes)" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: every file passing exits 0 and each file is checked once"
else
  fail "shellcheck-staged: every file passing exits 0 and each file is checked once" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 失敗したファイルがあれば非 0。ほかのファイルも検査され、出力はファイル単位でまとまり、引数の順に並ぶ
sc_run a.sh slow-bad1.sh c.sh bad2.sh
sc_want="$(printf '%s\n' 'slow-bad1.sh: begin' 'slow-bad1.sh: middle' 'slow-bad1.sh: end' 'bad2.sh: begin' 'bad2.sh: middle' 'bad2.sh: end')"
if [ "${sc_rc}" -eq 1 ] && [ "$(cat "${work_dir}/out")" = "${sc_want}" ] && [ "$(sc_ncalls)" = "4" ]; then
  pass "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per batch in batch order"
else
  fail "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per batch in batch order" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 1 件だけの検査で失敗しても非 0（失敗するファイルが 1 件だけの場合）
sc_run a.sh only-bad.sh
if [ "${sc_rc}" -eq 1 ]; then
  pass "shellcheck-staged: exactly one failing file among passing ones exits 1"
else
  fail "shellcheck-staged: exactly one failing file among passing ones exits 1" "exit ${sc_rc}"
fi

# 空白を含むファイル名は 1 つの引数のまま渡る（空の引数で起動しない）
sc_run "dir with space/my file.sh" "-dash.sh"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "2" ] && [ "$(sc_dashes)" = "2" ] && [ "$(sc_files)" = "$(printf '%s\n' '-dash.sh' 'dir with space/my file.sh')" ]; then
  pass "shellcheck-staged: file names with spaces or a leading dash are passed intact after --"
else
  fail "shellcheck-staged: file names with spaces or a leading dash are passed intact after --" "exit ${sc_rc}, calls: $(cat "${sc_log}")"
fi

# 同じ名前の引数（lefthook は glob が 2 つあると両方に一致するファイルを 2 回渡す）は 1 回だけ検査する
sc_run a.sh b.sh a.sh "x y.sh" "x y.sh" b.sh
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "3" ] && [ "$(sc_files)" = "$(printf '%s\n' a.sh b.sh 'x y.sh')" ]; then
  pass "shellcheck-staged: duplicate arguments are checked once"
else
  fail "shellcheck-staged: duplicate arguments are checked once" "exit ${sc_rc}, calls: $(cat "${sc_log}")"
fi

# 検査が結果を書く前に落ちたとき（子の bash が kill される）は、成功にせず 2（検査不能）で止まり、ファイル名つきの診断を出す
sc_run a.sh die.sh c.sh
if [ "${sc_rc}" -eq 2 ] && grep -qF "no valid result for die.sh" "${work_dir}/out"; then
  pass "shellcheck-staged: a check that dies before writing its result exits 2 and names the file"
else
  fail "shellcheck-staged: a check that dies before writing its result exits 2 and names the file" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 空のファイル名（名前が子に渡らない形）は検査を始めず、成功にもしない
sc_run ""
if [ "${sc_rc}" -eq 2 ] && [ ! -s "${sc_log}" ] && grep -qF "no valid result for" "${work_dir}/out"; then
  pass "shellcheck-staged: an empty file name is never reported as a pass"
else
  fail "shellcheck-staged: an empty file name is never reported as a pass" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 検査の道具自身の失敗（shellcheck が 2）は検査不能で、指摘（1）より優先して 2 で終わる。指摘だけのときは 1 のまま
sc_run bad1.sh err1.sh bad2.sh a.sh
if [ "${sc_rc}" -eq 2 ] && grep -qF "could not check err1.sh (exit 2)" "${work_dir}/out" && grep -qF "bad1.sh: begin" "${work_dir}/out"; then
  pass "shellcheck-staged: one unchecked file among findings exits 2 (unchecked wins over findings)"
else
  fail "shellcheck-staged: one unchecked file among findings exits 2 (unchecked wins over findings)" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 一時領域を作れない（TMPDIR が存在しない）ときは、指摘ありの 1 ではなく検査不能の 2 で止まる
sc_reset
if SC_LOG="${sc_log}" TMPDIR="${work_dir}/no-such-dir" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
if [ "${sc_rc}" -eq 2 ] && grep -qF "cannot create a temporary directory" "${work_dir}/out"; then
  pass "shellcheck-staged: a missing TMPDIR exits 2"
else
  fail "shellcheck-staged: a missing TMPDIR exits 2" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 件数が並列数の上限を超えても、shellcheck の起動はバッチ数（上限）以下で、全件がちょうど 1 回ずつ渡る（起動の回数をファイル数に比例させない）
sc_many=()
for sc_i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  sc_many+=("f${sc_i}.sh")
done
sc_jobs=4
sc_run "${sc_many[@]}"
sc_calls="$(sc_ncalls)"
sc_args="$(sc_files)"
sc_want_args="$(printf '%s\n' "${sc_many[@]}" | sort)"
if [ "${sc_rc}" -eq 0 ] && [ "${sc_calls}" = "4" ] && [ "${sc_args}" = "${sc_want_args}" ]; then
  pass "shellcheck-staged: 30 files with 4 jobs start shellcheck exactly 4 times and pass every file exactly once"
else
  fail "shellcheck-staged: 30 files with 4 jobs start shellcheck exactly 4 times and pass every file exactly once" "exit ${sc_rc}, calls: ${sc_calls}" "args: $(printf '%s' "${sc_args}" | tr '\n' ' ')"
fi
sc_jobs=8

# 大きいファイルから順に、合計の小さいバッチへ入れる（貪欲法）。大きさは wc -c の 1 回の起動で取り、空白を含む名前も対応が崩れない。
# 大きさ 1000・600・500・400 を 2 バッチに分けると、{1000, 400} と {600, 500} になる。
sc_dir="${work_dir}/sc-sizes"
mkdir -p "${sc_dir}"
cd "${sc_dir}"
head -c 1000 /dev/zero > big.sh
head -c 600 /dev/zero > "mid one.sh"
head -c 500 /dev/zero > mid2.sh
head -c 400 /dev/zero > small.sh
sc_jobs=2
sc_run small.sh "mid one.sh" mid2.sh big.sh
sc_want="$(printf '%s\n' 'big.sh|small.sh' 'mid one.sh|mid2.sh' | sort)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_groups)" = "${sc_want}" ]; then
  pass "shellcheck-staged: files are split into batches of similar total size (largest first)"
else
  fail "shellcheck-staged: files are split into batches of similar total size (largest first)" "exit ${sc_rc}" "calls: $(cat "${sc_log}")"
fi
sc_jobs=8
cd "${work_dir}"

# getconf が桁あふれする数字の列を返しても、比較が誤って 0 のまま素通りしない。固定値（4）に落ちて全件が検査され、0 で終わる
# （期待は「正常に検査されて 0」。2 で止まるのではなく、検査の範囲が変わらないことを確かめる）
sc_jobs=99999999999999999999
sc_run a.sh b.sh c.sh d.sh e.sh
sc_args="$(sc_files)"
sc_calls="$(sc_ncalls)"
if [ "${sc_rc}" -eq 0 ] && [ "${sc_args}" = "$(printf '%s\n' a.sh b.sh c.sh d.sh e.sh)" ] && [ "${sc_calls}" = "4" ]; then
  pass "shellcheck-staged: an overflowing getconf value falls back to a fixed parallel limit and every file is checked"
else
  fail "shellcheck-staged: an overflowing getconf value falls back to a fixed parallel limit and every file is checked" "exit ${sc_rc}, calls: ${sc_calls}" "output: $(cat "${work_dir}/out")"
fi
sc_jobs=8

# 出力先（番号付きの .out）を開けないときは、bash が shellcheck を起動せずに返す 1 を「指摘あり」にせず、検査不能の 2 にする。
# 偽の mktemp が、1 番目のバッチの出力先を一時領域の中の存在しない相対パスへのリンクにした一時領域を返す（開けず、親の読み出しにも引っかからない）
sc_mk_dir="${work_dir}/sc-mk-shim"
mkdir -p "${sc_mk_dir}"
cat > "${sc_mk_dir}/mktemp" <<'EOF'
#!/bin/sh
d="$("${SC_REAL_MKTEMP}" "$@")" || exit 1
ln -s no-such-dir/out "${d}/0.out"
printf '%s\n' "${d}"
EOF
chmod +x "${sc_mk_dir}/mktemp"
sc_real_mktemp="$(command -v mktemp)"
sc_reset
if SC_REAL_MKTEMP="${sc_real_mktemp}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_mk_dir}:${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
sc_collect
if [ "${sc_rc}" -eq 2 ] && [ ! -s "${sc_log}" ] && grep -qF "shellcheck could not check a.sh (exit 2)" "${work_dir}/out"; then
  pass "shellcheck-staged: an unopenable output file exits 2 (not treated as a finding)"
else
  fail "shellcheck-staged: an unopenable output file exits 2 (not treated as a finding)" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 中断（TERM）では、起動済みのバッチの shellcheck まで止め、2 で終わる（止めずに孫の shellcheck を残さない）。
# 偽の shellcheck が pid を書いて眠っている間に親へ TERM を送り、終わった後にその pid のプロセスが残っていないことを確かめる
sc_pidfile="${work_dir}/sc.pid"
rm -f "${sc_pidfile}"
sc_reset
SC_PIDFILE="${sc_pidfile}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh a.sh > "${work_dir}/out" 2>&1 &
sc_script_pid=$!
sc_waited=0
while [ ! -s "${sc_pidfile}" ] && [ "${sc_waited}" -lt 100 ]; do
  sleep 0.1
  sc_waited=$((sc_waited + 1))
done
sc_t0="${SECONDS}"
kill -TERM "${sc_script_pid}" 2> /dev/null || true
if wait "${sc_script_pid}"; then
  sc_rc=0
else
  sc_rc=$?
fi
sc_stop_secs=$((SECONDS - sc_t0))
sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
sc_alive=1
if [ -n "${sc_fake_pid}" ]; then
  sc_waited=0
  while kill -0 "${sc_fake_pid}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_fake_pid}" 2> /dev/null; then
    kill -9 "${sc_fake_pid}" 2> /dev/null || true
  else
    sc_alive=0
  fi
fi
# 偽物は 30 秒眠るため、止まるまでに 10 秒以上かかったなら、停止が伝わらず眠りの終わりを待っただけ
if [ "${sc_rc}" -eq 2 ] && [ -n "${sc_fake_pid}" ] && [ "${sc_alive}" -eq 0 ] && [ "${sc_stop_secs}" -lt 10 ]; then
  pass "shellcheck-staged: a TERM stops the running shellcheck and exits 2"
else
  fail "shellcheck-staged: a TERM stops the running shellcheck and exits 2" "exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_alive}, seconds to stop: ${sc_stop_secs}" "output: $(cat "${work_dir}/out")"
fi

# PATH に shellcheck が無いときは、未検査で成功にせず 2 で止まる
sc_empty_path="${work_dir}/sc-empty-path"
mkdir -p "${sc_empty_path}"
if PATH="${sc_empty_path}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
if [ "${sc_rc}" -eq 2 ] && grep -qF "shellcheck not found" "${work_dir}/out"; then
  pass "shellcheck-staged: a missing shellcheck exits 2"
else
  fail "shellcheck-staged: a missing shellcheck exits 2" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

finish "part 8"
