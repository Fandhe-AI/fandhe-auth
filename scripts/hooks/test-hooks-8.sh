#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 8（TASK-6）。shellcheck-staged.sh の回帰テスト。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
# part 1 から分けた（検査時間は行数にほぼ比例するため、part を 300〜440 行程度に保つ。.claude/rules/git-hooks.md）。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

cd "${work_dir}"

# ---------------------------------------------------------------- shellcheck-staged.sh（pre-commit の shellcheck の並列実行）
# 本物の shellcheck には頼らず（macOS の CI にあるとは限らない）、PATH 先頭に置いた偽物で呼び方と集約を確かめる。
# 偽物は呼び出しを 1 行ずつ、呼び出しごとの別ファイル（${SC_LOG}.d/ の下）へ記録する。呼び出しは call と arg:<値>（1 引数 1 行）で記録する。呼ばれた引数の全件を順に見て、名前に bad を含むファイルは 3 行を出し（終了コード 1）、err を含むものは 1 行を出して（終了コード 2。shellcheck 自身の失敗。1 より優先）、slow を含むものは遅れて終わり、die を含むものは呼び出し元（結果を書く子の bash）を落とし、hang を含むものは自分の pid を SC_PIDFILE へ書いて 30 秒眠る。
sc_shim_dir="${work_dir}/sc-shim"
mkdir -p "${sc_shim_dir}"
cat > "${sc_shim_dir}/shellcheck" <<'EOF'
#!/bin/sh
# 1 回の呼び出しを「call」の行と、引数 1 つにつき 1 行の「arg:<値>」で記録する（空白を含む引数の境界を見分けるため）。
# 並列の呼び出しの記録が混ざらないよう（共有の 1 ファイルへの追記は macOS で行の途中で混ざった。#6076）、呼び出しごとに別のファイルへ書く。
# 名前は pid と連番で、noclobber の排他的な作成で確保する（pid が再利用されても既存の記録を上書きしない）。
# 確保できない・書けないときは、記録の欠落を成功と取り違えないよう 99 で落とす。
# 記録先が渡っていないときは、記録せずに通る経路を作らないよう 99 で落とす。
[ -n "${SC_LOG:-}" ] || exit 99
# 同期モード（SC_SYNC_DIR と SC_BARRIER の両方が空でないときだけ。並列に進むことと同時実行の上限を確かめる）。片方だけなら 99 で落とす。
# 開始の印（start.*。消さない）と実行中の印（run.*）を作り、その時点の run.* の件数を数える。
# start.* が SC_BARRIER 件になるまで最大約 10 秒待ち（待ちきれなければ timeout.<pid> を作って先へ進む）、少し眠ってからもう一度 run.* を数える。
# 2 回の大きい方を seen.* に書いて run.* を消す。バッチ数と同じ障壁は、全バッチが同時に起動したときだけ待たずに抜けられる。
# 記録は ${SC_LOG}.d/ の call / arg の行とは別のファイルに置く（記録の読み取りを乱さない）。
if [ -n "${SC_SYNC_DIR:-}" ] || [ -n "${SC_BARRIER:-}" ]; then
  if [ -z "${SC_SYNC_DIR:-}" ] || [ -z "${SC_BARRIER:-}" ]; then
    exit 99
  fi
  sy_n=0
  while :; do
    sy_id="$$.${sy_n}"
    if (set -C; : > "${SC_SYNC_DIR}/start.${sy_id}") 2> /dev/null; then
      break
    fi
    sy_n=$((sy_n + 1))
    if [ "${sy_n}" -gt 1000 ]; then
      exit 99
    fi
  done
  : > "${SC_SYNC_DIR}/run.${sy_id}" || exit 99
  sy_seen=0
  for sy_f in "${SC_SYNC_DIR}"/run.*; do
    [ -e "${sy_f}" ] && sy_seen=$((sy_seen + 1))
  done
  sy_tries=0
  while :; do
    sy_started=0
    for sy_f in "${SC_SYNC_DIR}"/start.*; do
      [ -e "${sy_f}" ] && sy_started=$((sy_started + 1))
    done
    if [ "${sy_started}" -ge "${SC_BARRIER}" ]; then
      break
    fi
    sy_tries=$((sy_tries + 1))
    if [ "${sy_tries}" -gt 200 ]; then
      : > "${SC_SYNC_DIR}/timeout.$$" || exit 99
      break
    fi
    sleep 0.05
  done
  sleep 0.2
  sy_now=0
  for sy_f in "${SC_SYNC_DIR}"/run.*; do
    [ -e "${sy_f}" ] && sy_now=$((sy_now + 1))
  done
  if [ "${sy_now}" -gt "${sy_seen}" ]; then
    sy_seen="${sy_now}"
  fi
  printf '%s\n' "${sy_seen}" > "${SC_SYNC_DIR}/seen.${sy_id}" || exit 99
  rm -f "${SC_SYNC_DIR}/run.${sy_id}"
fi
sc_n=0
while :; do
  sc_f="${SC_LOG}.d/$$.${sc_n}"
  if (set -C; : > "${sc_f}") 2> /dev/null; then
    break
  fi
  sc_n=$((sc_n + 1))
  if [ "${sc_n}" -gt 1000 ]; then
    exit 99
  fi
done
{ printf 'call\n' && printf 'arg:%s\n' "$@"; } > "${sc_f}" || exit 99
rc=0
for arg in "$@"; do
  case "${arg}" in
    *slow*) sleep 0.3 ;;
    *die*) kill -9 "${PPID}" ;;
    *hang*)
      echo "$$" > "${SC_PIDFILE}"
      exec sleep 30
      ;;
    *err*)
      echo "${arg}: could not be read"
      rc=2
      ;;
  esac
  case "${arg}" in
    *bad*)
      echo "${arg}: begin"
      echo "${arg}: middle"
      echo "${arg}: end"
      if [ "${rc}" -eq 0 ]; then
        rc=1
      fi
      ;;
  esac
done
exit "${rc}"
EOF
# 並列数（バッチ数の上限）を固定するため、getconf も偽物にする（SC_JOBS を返す）
# 呼ばれ方も確かめる: 引数が _NPROCESSORS_ONLN の 1 つだけでなければ 99 で落とす（決め方が別の変数に変わったら気づく）。
# SC_GETCONF_RC が空でなければ、SC_JOBS を出したうえでその終了コードで終わる（失敗した getconf の出力を使わないことを試す）。
# SC_JOBS が未設定なら 8、空なら空のまま返す（空の出力が固定値に落ちることを試す）。
cat > "${sc_shim_dir}/getconf" <<'EOF'
#!/bin/sh
if [ "$#" -ne 1 ] || [ "$1" != "_NPROCESSORS_ONLN" ]; then
  exit 99
fi
printf '%s\n' "${SC_JOBS-8}"
if [ -n "${SC_GETCONF_RC:-}" ]; then
  exit "${SC_GETCONF_RC}"
fi
EOF
chmod +x "${sc_shim_dir}/getconf"
chmod +x "${sc_shim_dir}/shellcheck"
SC_STAGED="${HOOKS_DIR}/shellcheck-staged.sh"
sc_log="${work_dir}/sc.log"
sc_jobs=8
sc_getconf_rc=""
sc_sync_dir=""
sc_barrier=""
# 記録は呼び出しごとのファイル（${sc_log}.d/）に書かれる。sc_reset で空に作り直し、入口の終了後に sc_collect で sc_log へ連結する
# （1 呼び出し = 1 ファイルなので、連結しても各呼び出しの記録は途中で混ざらない）。読むのは入口が終わった後だけ。
# 連結後の sc_log が空なら呼び出しは 0 件。連結の失敗は 0 件と取り違えないよう止める。
sc_reset() {
  rm -rf "${sc_log}.d"
  mkdir "${sc_log}.d"
  : > "${sc_log}"
}
sc_collect() {
  # 記録の置き場が読めない・辿れないときは、0 件と取り違えず 2 で止める。予約だけで中身の無い記録（偽物の書き込み途中の失敗）も同じ
  if [ ! -d "${sc_log}.d" ] || [ ! -r "${sc_log}.d" ] || [ ! -x "${sc_log}.d" ]; then
    echo "test-hooks: cannot read the shellcheck call records" >&2
    exit 2
  fi
  : > "${sc_log}"
  for sc_rec in "${sc_log}.d"/*; do
    if [ -e "${sc_rec}" ]; then
      if [ ! -s "${sc_rec}" ]; then
        echo "test-hooks: empty shellcheck call record" >&2
        exit 2
      fi
      cat "${sc_rec}" >> "${sc_log}" || exit 2
    fi
  done
}
# sc_run <引数...>: 偽の shellcheck を先頭に置いて入口を起動する。終了コードは sc_rc、出力は ${work_dir}/out、呼び出しの記録は sc_log。
sc_run() {
  sc_reset
  if SC_JOBS="${sc_jobs}" SC_GETCONF_RC="${sc_getconf_rc}" SC_SYNC_DIR="${sc_sync_dir}" SC_BARRIER="${sc_barrier}" SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" "$@" > "${work_dir}/out" 2>&1; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_collect
}

# 記録の読み方: sc_ncalls = 呼び出しの回数、sc_files = 渡された引数（-- を除く）を 1 行 1 つで並べ替えたもの、
# sc_groups = 呼び出しごとの引数を | でつないだ行を並べ替えたもの（引数の境界が行の境界に対応する）。
sc_ncalls() { grep -c '^call$' "${sc_log}" || true; }
sc_dashes() { grep -c '^arg:--$' "${sc_log}" || true; }
sc_files() { grep '^arg:' "${sc_log}" | sed 's/^arg://' | grep -vx -e '--' | sort || true; }
sc_groups() {
  awk '/^call$/ { if (g != "") print g; g = ""; next } { sub(/^arg:/, ""); if ($0 == "--") next; g = (g == "" ? $0 : g "|" $0) } END { if (g != "") print g }' "${sc_log}" | sort
}

# 同期モードの読み方: sc_sync_reset = 同期の置き場を空に作り直す、sc_max_seen = 観測した同時実行数の最大（記録が無ければ空）、sc_timeouts = 障壁の待ちきれなかった件数。
sc_sync_reset() {
  rm -rf "${sc_sync_dir}"
  mkdir "${sc_sync_dir}"
}
sc_max_seen() {
  sc_ms=""
  for sc_sf in "${sc_sync_dir}"/seen.*; do
    if [ -e "${sc_sf}" ]; then
      sc_sv="$(cat "${sc_sf}")"
      case "${sc_sv}" in
        '' | *[!0-9]*) sc_sv="" ;;
      esac
      if [ -z "${sc_sv}" ]; then
        echo "bad"
        return 0
      fi
      if [ -z "${sc_ms}" ] || [ "${sc_sv}" -gt "${sc_ms}" ]; then
        sc_ms="${sc_sv}"
      fi
    fi
  done
  echo "${sc_ms}"
}
sc_timeouts() {
  sc_tn=0
  for sc_tf in "${sc_sync_dir}"/timeout.*; do
    if [ -e "${sc_tf}" ]; then
      sc_tn=$((sc_tn + 1))
    fi
  done
  echo "${sc_tn}"
}
# sc_expect_batches <getconf の出力> <期待の起動回数> <説明>: 20 件を渡し、起動の回数が期待と完全に一致し、全件がちょうど 1 回ずつ渡って 0 で終わることを確かめる。
# 並列数（バッチ数）の決め方の確認で、getconf を固定する。SC_GETCONF_RC を使うときは呼ぶ前に sc_getconf_rc を設定する。
sc_twenty=()
for sc_i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  sc_twenty+=("f${sc_i}.sh")
done
sc_twenty_want="$(printf '%s\n' "${sc_twenty[@]}" | sort)"
sc_expect_batches() {
  sc_jobs="$1"
  sc_run "${sc_twenty[@]}"
  sc_calls="$(sc_ncalls)"
  if [ "${sc_rc}" -eq 0 ] && [ "${sc_calls}" = "$2" ] && [ "$(sc_files)" = "${sc_twenty_want}" ]; then
    pass "$3"
  else
    fail "$3" "exit ${sc_rc}, calls: ${sc_calls} (want $2)" "output: $(cat "${work_dir}/out")"
  fi
  sc_jobs=8
  sc_getconf_rc=""
}

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

finish "part 8"
