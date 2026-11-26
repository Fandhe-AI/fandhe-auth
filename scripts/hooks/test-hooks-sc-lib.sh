#!/usr/bin/env bash
# test-hooks-8.sh・test-hooks-9.sh が共有する shellcheck-staged.sh の試験の道具（TASK-6）。単体では実行せず、part から source する。
# part が test-hooks-lib.sh を source し、cd "${work_dir}" した後に source する。part の間で状態は引き継がない。
# 名前は test-hooks-<数字>.sh に当たらない（test-hooks.sh の parts の突き合わせの対象外）。

# ---------------------------------------------------------------- shellcheck-staged.sh（pre-commit の shellcheck の並列実行）
# 本物の shellcheck には頼らず（macOS の CI にあるとは限らない）、PATH 先頭に置いた偽物で呼び方と集約を確かめる。
# 偽物は呼び出しを 1 行ずつ、呼び出しごとの別ファイル（${SC_LOG}.d/ の下）へ記録する。呼び出しは call と arg:<値>（1 引数 1 行）で記録する。呼ばれた引数の全件を順に見て、名前に bad を含むファイルは 3 行を出し（終了コード 1）、err を含むものは 1 行を出して（終了コード 2。shellcheck 自身の失敗。1 より優先）、slow を含むものは遅れて終わり、die を含むものは呼び出し元（結果を書く子の bash）を落とし、hang を含むものは自分の pid を SC_PIDFILE へ書いて 30 秒眠る。
# shellcheck disable=SC2154  # work_dir は test-hooks-lib.sh が定義する（part が先に source する）。受け取るのはここだけで、以降は sc_work_dir を参照する
sc_work_dir="${work_dir}"
sc_shim_dir="${sc_work_dir}/sc-shim"
mkdir -p "${sc_shim_dir}"
cat > "${sc_shim_dir}/shellcheck" <<'EOF'
#!/bin/sh
# 1 回の呼び出しを「call」の行と、引数 1 つにつき 1 行の「arg:<値>」で記録する（空白を含む引数の境界を見分けるため）。
# 並列の呼び出しの記録が混ざらないよう、共有の 1 ファイルへ追記せず呼び出しごとに別のファイルへ書く。
# #6076 では、共有の 1 ファイルへ追記した並列の呼び出しの記録が行の境界で入り混じった（macOS の /bin/sh は bash。観測した条件に限る）。printf が出力の各行を個別の write(2) で書くとは保証されず、1 回の printf が複数の書き込みに分かれ得るため、O_APPEND は 1 回の write(2) の中しか保護せず、呼び出し単位の記録をまとめては保護できない。
# 名前は pid と連番で、noclobber の排他的な作成で確保する（pid が再利用されても既存の記録を上書きしない）。
# 確保できない・書けないときは、記録の欠落を成功と取り違えないよう 99 で落とす。
# 記録先が渡っていないときは、記録せずに通る経路を作らないよう 99 で落とす。
[ -n "${SC_LOG:-}" ] || exit 99
# 同期モード（SC_SYNC_DIR と SC_BARRIER の両方が空でないときだけ。並列に進むことと同時実行の上限を確かめる）。片方だけなら 99 で落とす。
# 開始の印（start.*。id の確保用で消さない）と実行中の印（run.*）を作り、run.* を作り終えた印（ready.*。消さない）を置く。
# 2 段の同期をとる。(1) ready.* が SC_BARRIER 件になるまで待つ（全員が run.* を作ってから観測を始める）。
# 少し眠ってから run.* を数えて seen.* に書き、観測を終えた印（done.*。消さない）を置く。
# (2) done.* が SC_BARRIER 件になるまで待ち（全員が観測を終えるまで run.* を保持する）、run.* を消す。
# どちらも最大約 10 秒待ち、待ちきれなければ timeout.<pid> を作って先へ進む。timeout.* が 1 つでもあれば、両方の待ちを（後続の呼び出しも）すぐ打ち切る。
# 記録は ${SC_LOG}.d/ の call / arg の行とは別のファイルに置く（記録の読み取りを乱さない）。
if [ -n "${SC_SYNC_DIR:-}" ] || [ -n "${SC_BARRIER:-}" ]; then
  if [ -z "${SC_SYNC_DIR:-}" ] || [ -z "${SC_BARRIER:-}" ]; then
    exit 99
  fi
  sy_timed_out() {
    for sy_t in "${SC_SYNC_DIR}"/timeout.*; do
      [ -e "${sy_t}" ] && return 0
    done
    return 1
  }
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
  : > "${SC_SYNC_DIR}/ready.${sy_id}" || exit 99
  sy_tries=0
  while :; do
    sy_started=0
    for sy_f in "${SC_SYNC_DIR}"/ready.*; do
      [ -e "${sy_f}" ] && sy_started=$((sy_started + 1))
    done
    if [ "${sy_started}" -ge "${SC_BARRIER}" ]; then
      break
    fi
    # 誰かが待ちきれていたら、後続の待ちも打ち切る（直列化の回帰で後続バッチが各約 10 秒待たないように）
    if sy_timed_out; then
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
  sy_seen=0
  for sy_f in "${SC_SYNC_DIR}"/run.*; do
    [ -e "${sy_f}" ] && sy_seen=$((sy_seen + 1))
  done
  printf '%s\n' "${sy_seen}" > "${SC_SYNC_DIR}/seen.${sy_id}" || exit 99
  : > "${SC_SYNC_DIR}/done.${sy_id}" || exit 99
  sy_tries=0
  while :; do
    sy_finished=0
    for sy_f in "${SC_SYNC_DIR}"/done.*; do
      [ -e "${sy_f}" ] && sy_finished=$((sy_finished + 1))
    done
    if [ "${sy_finished}" -ge "${SC_BARRIER}" ]; then
      break
    fi
    if sy_timed_out; then
      break
    fi
    sy_tries=$((sy_tries + 1))
    if [ "${sy_tries}" -gt 200 ]; then
      : > "${SC_SYNC_DIR}/timeout.$$" || exit 99
      break
    fi
    sleep 0.05
  done
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
sc_log="${sc_work_dir}/sc.log"
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
# sc_run <引数...>: 偽の shellcheck を先頭に置いて入口を起動する。終了コードは sc_rc、出力は ${sc_work_dir}/out、呼び出しの記録は sc_log。
sc_run() {
  sc_reset
  if SC_JOBS="${sc_jobs}" SC_GETCONF_RC="${sc_getconf_rc}" SC_SYNC_DIR="${sc_sync_dir}" SC_BARRIER="${sc_barrier}" SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" "$@" > "${sc_work_dir}/out" 2>&1; then
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
    fail "$3" "exit ${sc_rc}, calls: ${sc_calls} (want $2)" "output: $(cat "${sc_work_dir}/out")"
  fi
  sc_jobs=8
  sc_getconf_rc=""
}

