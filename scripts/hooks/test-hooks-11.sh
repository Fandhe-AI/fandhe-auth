#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 11（TASK-6）。shellcheck-staged.sh の中断（TERM・HUP）の扱いの回帰テスト。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 最小の準備は test-hooks-core.sh が、偽の shellcheck と道具は test-hooks-sc-lib.sh が持つ。part の間で状態は引き継がない。
# part 9 から分けた（検査時間は行数にほぼ比例するため、part の行数の目安は .claude/rules/git-hooks.md）。
# 中断の試験の補助関数（sc_kill_job・sc_reap 等）はこの part の中だけで使うため、test-hooks-sc-lib.sh には置かず、この part に持つ（lib の解析を part ごとに重ねないため）。
set -euo pipefail
# shellcheck source=./test-hooks-core.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-core.sh"

cd "${work_dir}"

# shellcheck source=./test-hooks-sc-lib.sh
source "${HOOKS_DIR}/test-hooks-sc-lib.sh"

# pid が偽物の shellcheck（exec した sleep）になったかを見る（待ち合わせ専用。kill には使わない）。
sc_is_fake_sleep() {
  sc_comm="$(ps -p "$1" -o comm= 2> /dev/null)" || return 1
  sc_comm="${sc_comm#"${sc_comm%%[![:space:]]*}"}"
  sc_comm="${sc_comm%"${sc_comm##*[![:space:]]}"}"
  case "${sc_comm}" in
    sleep | */sleep) return 0 ;;
    *) return 1 ;;
  esac
}

# pid が試験の起動したスクリプト（$2）の子孫の偽物（sleep）であるときだけ、信号 $1 を $3 へ送る（pid の再利用で無関係のプロセスへ送らない。深さ 4 まで辿る）。
sc_signal_owned() {
  sc_so_pid="$3"
  sc_so_depth=0
  sc_is_fake_sleep "${sc_so_pid}" || return 1
  while [ "${sc_so_depth}" -lt 4 ]; do
    sc_so_pid="$(ps -p "${sc_so_pid}" -o ppid= 2> /dev/null | tr -d '[:space:]')" || return 1
    [ -n "${sc_so_pid}" ] || return 1
    if [ "${sc_so_pid}" = "$2" ]; then
      kill -"$1" "$3" 2> /dev/null
      return $?
    fi
    sc_so_depth=$((sc_so_depth + 1))
  done
  return 1
}

# sc_kill_job <信号> <pid>: この試験のシェルのジョブ表に、生きた（Running・Stopped）ジョブとして残る pid にだけ信号を送る。
# ジョブ表は 1 回の `jobs -l` で取る（bash が回収済みの pid は再利用されうるため、起動時に控えた pid へ無条件に送らない）。確かめられなければ送らず 1 を返す。
sc_kill_job() {
  # 状態名は bash が翻訳するため、関数の局所の LC_ALL=C で ASCII の名前にして照合する（git-hooks.md の (b)）
  local LC_ALL=C
  sc_kj_file="${work_dir}/kj-jobs"
  sc_kj_ok=1
  jobs -l >| "${sc_kj_file}" 2> /dev/null || return 1
  while read -r _kj_n sc_kj_pid sc_kj_state _kj_rest; do
    if [ "${sc_kj_pid}" = "$2" ]; then
      case "${sc_kj_state}" in
        Running | Stopped*) sc_kj_ok=0 ;;
      esac
    fi
  done < "${sc_kj_file}"
  rm -f "${sc_kj_file}"
  [ "${sc_kj_ok}" -eq 0 ] || return 1
  kill -"$1" "$2" 2> /dev/null
}

# sc_reap <pid>: 試験が起動したスクリプトを強制停止して回収する。消滅を確かめられたときだけ wait し（0）、
# 確かめられなければ wait せず 1 を返す（残り続ける対象を無期限に待たない）。既に回収済みで一覧に無い場合も、消滅していれば 0。
sc_reap() {
  sc_kj_note=""
  sc_kill_job KILL "$1" || true
  sc_rp_waited=0
  while kill -0 "$1" 2> /dev/null && [ "${sc_rp_waited}" -lt 50 ]; do
    sleep 0.1
    sc_rp_waited=$((sc_rp_waited + 1))
  done
  if kill -0 "$1" 2> /dev/null; then
    sc_kj_note="; could not confirm the forced stop of pid ${1} (not waiting)"
    return 1
  fi
  wait "$1" 2> /dev/null || true
  return 0
}

# sc_orphan_signal <信号> <pid> <起動時の開始時刻>: 孤児の偽物へ、送るたびに開始時刻の一致と sleep であることを確かめてから送る
# （CONT の後に終わって pid が再利用されても、TERM が別のプロセスへ届かない）。
sc_orphan_signal() {
  [ -n "$3" ] || return 1
  [ "$(ps -p "$2" -o lstart= 2> /dev/null || true)" = "$3" ] || return 1
  sc_is_fake_sleep "$2" || return 1
  kill -"$1" "$2" 2> /dev/null
}

# 中断（TERM・HUP）では、起動済みのバッチの shellcheck まで止め、2 で終わる（止めずに孫の shellcheck を残さない）。
# 偽の shellcheck が pid を書いて眠っている間に親へ信号を送り、終わった後にその pid のプロセスが残っていないことを確かめる
# INT は試験しない（背景で起動したシェルでは INT が無視された状態で始まり、trap できない）。
# HUP を trap できないシェルでは、30 秒待たない。CI（CI か GITHUB_ACTIONS が設定）では前提の欠けとして失敗にし、
# ローカルでは TAP の `# SKIP`（合格に数えない）で飛ばす（試験が黙って消えないようにする。AUDB-6 の趣旨）。
for sc_sig in TERM HUP TERM-STOPPED; do
  sc_stop_child=0
  if [ "${sc_sig}" = "TERM-STOPPED" ]; then
    sc_sig=TERM
    sc_stop_child=1
    sc_label="a TERM with a stopped shellcheck"
  else
    sc_label="a ${sc_sig}"
  fi
  if [ "${sc_sig}" = "HUP" ] && [ "$("${HOOK_BASH}" -c 'trap "exit 7" HUP; kill -HUP $$; exit 0' > /dev/null 2>&1; echo $?)" != "7" ]; then
    if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
      fail "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2" "missing prerequisite: HUP cannot be trapped in this shell (required in CI)"
    else
      echo "ok - shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2 # SKIP HUP cannot be trapped in this shell (not counted as a pass)"
    fi
    continue
  fi
  sc_pidfile="${work_dir}/sc.pid"
  rm -f "${sc_pidfile}"
  sc_reset
  SC_PIDFILE="${sc_pidfile}" SC_SYNC_DIR="" SC_BARRIER="" SC_GETCONF_RC="" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh a.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_waited=0
  while [ ! -s "${sc_pidfile}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  # 偽物が exec して sleep になるまで待ってから信号を送る（exec 前に送ると、停止の対象が sleep でなくシェルになり、確認が曖昧になる）
  # 起動・sleep への exec・SIGSTOP の送信・停止状態のどれかを確認できなければ、信号を送らず試験の失敗として扱う（退行を見逃さない）。
  sc_setup_err=""
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  if [ -z "${sc_fake_pid}" ]; then
    sc_setup_err="the fake shellcheck did not start"
  else
    sc_waited=0
    while ! sc_is_fake_sleep "${sc_fake_pid}" && [ "${sc_waited}" -lt 50 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if ! sc_is_fake_sleep "${sc_fake_pid}"; then
      sc_setup_err="the fake shellcheck did not become sleep"
    fi
  fi
  sc_fake_start=""
  if [ -z "${sc_setup_err}" ]; then
    sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
  fi
  if [ -z "${sc_setup_err}" ] && [ "${sc_stop_child}" -eq 1 ]; then
    # 一時停止中の子を持つ中断（停止中の子は CONT が無いと TERM を受け取れない）
    if ! sc_signal_owned STOP "${sc_script_pid}" "${sc_fake_pid}"; then
      sc_setup_err="SIGSTOP could not be sent to the fake shellcheck"
    else
      sc_waited=0
      sc_state=""
      while [ "${sc_waited}" -lt 30 ]; do
        sc_state="$(ps -p "${sc_fake_pid}" -o stat= 2> /dev/null || true)"
        sc_state="${sc_state#"${sc_state%%[![:space:]]*}"}"
        case "${sc_state}" in
          T*) break ;;
        esac
        sleep 0.1
        sc_waited=$((sc_waited + 1))
      done
      case "${sc_state}" in
        T*) ;;
        *) sc_setup_err="the fake shellcheck did not reach the stopped state" ;;
      esac
    fi
  fi
  if [ -n "${sc_setup_err}" ]; then
    # 管理下のプロセスだけを後始末する（偽物は自分で起動した子孫。眠りは 30 秒で終わる）
    if [ -n "${sc_fake_pid}" ]; then
      sc_signal_owned CONT "${sc_script_pid}" "${sc_fake_pid}" || true
    fi
    sc_reap "${sc_script_pid}" || true
    fail "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2" "setup failed: ${sc_setup_err}${sc_kj_note}" "output: $(cat "${work_dir}/out")"
    continue
  fi
  sc_t0="${SECONDS}"
  sc_kill_job "${sc_sig}" "${sc_script_pid}" || true
  # 期限（20 秒）付きで終了を待つ。無期限の wait にすると、停止が伝わらない退行で試験が止まる。
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sc_timed_out=0
  sc_kj_note=""
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_timed_out=1
    # 期限切れ: 試験が管理するプロセスだけを後始末して失敗にする
    sc_signal_owned CONT "${sc_script_pid}" "${sc_fake_pid}" || true
    sc_reap "${sc_script_pid}" || true
  fi
  if [ "${sc_timed_out}" -eq 1 ]; then
    # 回収は sc_reap が済ませた（確かめられなければ wait していない）
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_stop_secs=$((SECONDS - sc_t0))
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  # 停止の確認: 消滅（kill -0 の失敗）を確かめられたときだけ停止済みとする。ps の失敗・別名は停止の証拠にしない。
  # 強制終了は送らない（pid の再利用で無関係のプロセスを落とさない）。
  # 消滅の判定は kill -0 だけで行う（ps の失敗・名前の不一致を消滅と取り違えない。ps は起動時の待ち合わせにだけ使う）。
  sc_alive=1
  if [ -n "${sc_fake_pid}" ]; then
    sc_waited=0
    while kill -0 "${sc_fake_pid}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_fake_pid}" 2> /dev/null; then
      # 親が先に終わった孤児（SIGSTOP のまま残る等）は、起動時に控えた開始時刻と一致し、まだ sleep であるときだけ回収する
      # （pid の再利用で無関係のプロセスへ送らない）。ps で確かめられなければ送らず、偽物は 30 秒で自然に終わる。
      sc_now_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
      if [ -n "${sc_fake_start}" ] && [ "${sc_now_start}" = "${sc_fake_start}" ] && sc_is_fake_sleep "${sc_fake_pid}"; then
        sc_orphan_signal CONT "${sc_fake_pid}" "${sc_fake_start}" || true
        sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
      fi
    else
      sc_alive=0
    fi
  fi
  # 偽物は 30 秒眠るため、止まるまでに 10 秒以上かかったなら、停止が伝わらず眠りの終わりを待っただけ
  if [ "${sc_rc}" -eq 2 ] && [ -n "${sc_fake_pid}" ] && [ "${sc_alive}" -eq 0 ] && [ "${sc_stop_secs}" -lt 10 ]; then
    pass "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2"
  else
    fail "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2" "exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_alive}, seconds to stop: ${sc_stop_secs}${sc_kj_note}" "output: $(cat "${work_dir}/out")"
  fi
done

# job_state の状態語の解釈（ジョブ表を偽物に差し替えて確かめる。本体の関数をそのまま取り出す）。
# 信号で死んだ子の状態語（Hangup・Interrupt・Segmentation fault 等）は終了済み（none）で、誤った診断・wait の省略を起こさない。
# 一致した pid の状態語が空・ジョブ表を取れないときだけ unknown（信号を送らない）。
sc_js_fn="$(awk '/^job_state\(\) \{/ {p=1} p {print} p && /^\}/ {exit}' "${SC_STAGED}")"
# sc_js_fake_read <read の引数...>: sc_js_readfail が first なら最初の読み取りから、second なら 2 回目の読み取りから失敗し、それ以外は本物の read -r へ渡す。
# コマンド置換の中に case を置かない（bash 3.2 の解析の不具合を避ける）ため、if で書く。
sc_js_fake_read() {
  sc_js_reads=$((sc_js_reads + 1))
  if [ "${sc_js_readfail}" = first ]; then
    return 1
  fi
  if [ "${sc_js_readfail}" = second ] && [ "${sc_js_reads}" -ge 2 ]; then
    return 1
  fi
  builtin read -r "${@:2}"
}
sc_js_check() {
  # sc_js_check <期待> <説明> <jobs の出力の 1 行。"FAIL" なら jobs が失敗する> [読み取りの失敗: first=最初の read から失敗する / second=2 回目の read から失敗する]
  # コマンド置換の中には case・ヒアドキュメント・括弧を含むコメントを置かない（bash 3.2 は、コマンド置換の中の `)` を誤って解析する）。差し替えの本体は外の関数に置く。
  # 読み取りの失敗は、read を差し替えて起こす（読み取りエラーは EOF と同じ非 0 で終わるため、一覧を最後まで読めたかを終端の印で確かめる）。
  sc_js_got="$(
    # shellcheck disable=SC2034 # eval で取り出した job_state が使う
    tmp_dir="${work_dir}"
    SC_FAKE_LINE="$3"
    sc_js_reads=0
    sc_js_readfail="${4:-}"
    # shellcheck disable=SC2329 # eval で取り出した job_state が呼ぶ
    read() {
      sc_js_fake_read "$@"
    }
    # shellcheck disable=SC2329 # eval で取り出した job_state が呼ぶ
    jobs() {
      [ "${SC_FAKE_LINE}" != "FAIL" ] || return 1
      printf '%s\n' "${SC_FAKE_LINE}"
    }
    eval "${sc_js_fn}"
    job_state 4242
    # shellcheck disable=SC2154 # eval で取り出した job_state が設定する
    echo "${job_state_result}"
  )"
  if [ "${sc_js_got}" = "$1" ]; then
    pass "shellcheck-staged: job_state treats ${2} as ${1}"
  else
    fail "shellcheck-staged: job_state treats ${2} as ${1}" "got: ${sc_js_got:-none}" "line: $3"
  fi
}
sc_js_check owned "Running" "[1]+ 4242 Running                    sleep 30 &"
sc_js_check owned "Stopped" "[1]+ 4242 Stopped                    sleep 30"
sc_js_check owned "Stopped(SIGTSTP)" "[1]+ 4242 Stopped(SIGTSTP)           sleep 30"
sc_js_check owned "Stopped (signal)" "[1]+ 4242 Stopped (signal)           sleep 30"
sc_js_check owned "Suspended" "[1]+ 4242 Suspended                  sleep 30"
sc_js_check owned "Suspended (signal)" "[1]+ 4242 Suspended (signal)         sleep 30"
sc_js_check none "Done" "[1]+ 4242 Done                       sleep 30"
sc_js_check none "Exit 3" "[1]+ 4242 Exit 3                     sleep 30"
sc_js_check none "Terminated" "[1]+ 4242 Terminated                 sleep 30"
sc_js_check none "Interrupt (a child killed by SIGINT)" "[1]+ 4242 Interrupt                  sleep 30"
sc_js_check none "Hangup (a child killed by SIGHUP)" "[1]+ 4242 Hangup                     sleep 30"
sc_js_check none "Segmentation fault" "[1]+ 4242 Segmentation fault        sleep 30"
sc_js_check none "a pid not in the table" "[1]+ 9999 Running                    sleep 30 &"
sc_js_check unknown "an empty state word" "[1]+ 4242"
sc_js_check unknown "a failing jobs" "FAIL"
# 退行を捕まえる（#6136）: 読み取りエラーを EOF と同じにすると、載っているジョブの一覧を読めないまま none（停止済み）になる
sc_js_check unknown "a read error before the listed child (an incomplete table)" "[1]+ 4242 Running                    sleep 30 &" first
sc_js_check unknown "a read error after the listed child (the end mark is not reached)" "[1]+ 4242 Running                    sleep 30 &" second
sc_js_check unknown "a read error on a table without the child" "[1]+ 9999 Running                    sleep 30 &" first

# sc_klog_read <記録>: 差し込んだ kill の記録（1 行 = 信号と宛先の pid）から、信号の並び（sc_kr_sigs）と宛先の pid の並び（sc_kr_pids）を空白区切りで取り出す。
sc_klog_read() {
  sc_kr_sigs="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $1 }' "$1" 2> /dev/null || true)"
  sc_kr_pids="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $2 }' "$1" 2> /dev/null || true)"
  # 3 列目: 送った時点の宛先の親の pid（取れなければ空）
  sc_kr_ppids="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $3 }' "$1" 2> /dev/null || true)"
}

# 子への信号の送り先を固定する補助（pid の再利用・短く終わる別の子へ信号を送らない。sc_kill_job・sc_orphan_signal と同じ規律）。
# sc_same_args_children <親>: 親の直接の子のうち、引数が親と同じ bash（`&` で fork した subshell）の pid を出す。
# 本体はバッチの前に、引数の違う短命の子（getconf・mktemp・wc・sort の偽物や本物）も作るため、引数で区別する。
sc_same_args_children() {
  sc_sa_args="$(ps -p "$1" -o args= 2> /dev/null | sed 's/^ *//; s/ *$//')"
  [ -n "${sc_sa_args}" ] || return 1
  ps -A -o pid= -o ppid= -o args= 2> /dev/null | awk -v p="$1" -v a="${sc_sa_args}" '$2 == p { r = $0; sub(/^ *[0-9]+ +[0-9]+ +/, "", r); sub(/ *$/, "", r); if (r == a) print $1 }'
}
# sc_find_batch <親>: 親の直接の子が 1 つだけで、それが引数が親と同じ子のときだけ、その pid を sc_batch_pid に入れる（短命の別の子が残っている間・0 件・2 件以上なら空）。
sc_find_batch() {
  sc_batch_pid=""
  sc_fb_list="$(sc_same_args_children "$1" || true)"
  sc_fb_all="$(ps -A -o pid= -o ppid= 2> /dev/null | awk -v p="$1" '$2 == p { n++ } END { print n + 0 }')"
  sc_fb_n="$(printf '%s\n' "${sc_fb_list}" | grep -c . || true)"
  if [ "${sc_fb_n}" = "1" ] && [ "${sc_fb_all}" = "1" ]; then
    sc_batch_pid="${sc_fb_list}"
  fi
}
# sc_wait_batch <親>: 間を置いた 2 回続けて同じ pid が見つかるまで待つ（最大 10 秒）。見つからなければ sc_batch_pid は空。時間の経過だけには頼らない。
sc_wait_batch() {
  sc_wb_prev=""
  sc_wb_n=0
  while [ "${sc_wb_n}" -lt 50 ]; do
    sc_find_batch "$1"
    if [ -n "${sc_batch_pid}" ] && [ "${sc_batch_pid}" = "${sc_wb_prev}" ]; then
      return 0
    fi
    sc_wb_prev="${sc_batch_pid}"
    sleep 0.2
    sc_wb_n=$((sc_wb_n + 1))
  done
  sc_batch_pid=""
  return 1
}
# sc_write_kill_env <ファイル> <差し込む jobs の定義（空なら差し込まない）> <kill を失敗させる条件（空なら失敗させない）>:
# BASH_ENV で読ませるファイルを書く。差し込む kill は、本体が渡した最初の引数（信号）を SC_KLOG へ記録してから、条件に当たれば失敗し、そうでなければ本物の kill を呼ぶ。
sc_write_kill_env() {
  {
    printf '%s\n' "$2"
    printf '%s\n' "kill() { printf '%s %s %s\\n' \"\$1\" \"\${2:-}\" \"\$(ps -p \"\${2:-0}\" -o ppid= 2> /dev/null | tr -d ' ')\" >> \"\${SC_KLOG}\"; ${3:-true}; builtin kill \"\$@\"; }"
  } > "$1"
}
# sc_signal_child <信号> <pid> <親>: 送る直前に、pid が今も親の直接の子で、引数が親と同じことを確かめ直してから送る。
sc_signal_child() {
  sc_same_args_children "$3" | grep -qx "$2" || return 1
  kill -"$1" "$2" 2> /dev/null
}

# 信号を送れない・所有を確かめられない中断では、信号を送らず（または送れず）、固定の診断を出し、wait せずに 2 で終わる。
# BASH_ENV で差し込む関数で `jobs` か `kill` を失敗させる。偽の shellcheck は CONT・TERM を受けても生き続け、受けた信号を SC_SIGLOG へ記録する
# （sleep のままでは CONT を誤って送っても生きているため、合格してしまう）。wait へ進めば偽物の眠りが終わるまで戻らないため、10 秒未満で終わることで wait の省略を確かめる。
# 親が送る CONT・TERM の宛先はバッチの子（偽の shellcheck の 1 つ上）で、偽物には直接届かない。そのため、本体が送ろうとした信号は、差し込む kill が SC_KLOG へ記録して確かめる
# （偽物の記録では、バッチの子へ誤って送られた CONT・TERM を見落とす。バッチの子が TERM を受ければ batch_stop が偽物を止めるため、偽物が生きていることは TERM の不達の証拠になる）。
# sc_unk_check <名前> <差し込む jobs の定義（空なら差し込まない）> <差し込む kill の失敗の条件（空なら失敗させない）> <診断の断片> <信号の送り先: script か batch> <本体が送ろうとした信号の記録>
#   script: スクリプト本体（親）へ TERM を送る。batch: バッチの子（引数が親と同じ子がちょうど 1 つ）へ TERM を直接送る（batch_stop の経路）。
sc_unk_check() {
  sc_unk_env="${work_dir}/sc-unknown-env.sh"
  sc_write_kill_env "${sc_unk_env}" "$2" "$3"
  sc_klog="${work_dir}/sc.klog"
  rm -f "${sc_klog}"
  : > "${sc_klog}"
  sc_unk_dest=""
  sc_pidfile="${work_dir}/sc.pid"
  sc_siglog="${work_dir}/sc.siglog"
  rm -f "${sc_pidfile}" "${sc_siglog}"
  : > "${sc_siglog}"
  # 出力先は起動の前に作る（読み出しが set -e で試験を終わらせず、無いことも失敗として数える）
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_unk_env}" SC_KLOG="${sc_klog}" SC_SIGLOG="${sc_siglog}" SC_PIDFILE="${sc_pidfile}" SC_SYNC_DIR="" SC_BARRIER="" SC_GETCONF_RC="" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" sigtrap.sh a.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_waited=0
  while [ ! -s "${sc_pidfile}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  sc_fake_start=""
  if [ -n "${sc_fake_pid}" ]; then
    sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
  fi
  sc_kj_note=""
  sc_unk_err=""
  if [ -z "${sc_fake_start}" ]; then
    sc_unk_err="the fake shellcheck did not start"
  fi
  if ! "${HOOK_BASH}" -n "${sc_unk_env}" 2> /dev/null; then
    sc_unk_err="the injected functions have a syntax error"
  fi
  sc_t0="${SECONDS}"
  if [ -z "${sc_unk_err}" ]; then
    if [ "$5" = "batch" ]; then
      # 他の直接の子が消え、バッチの子が 2 回続けて同じ pid で見つかるのを待ってから送る
      sc_wait_batch "${sc_script_pid}" || true
      if [ -z "${sc_batch_pid}" ]; then
        sc_unk_err="the batch child could not be identified"
      elif ! sc_signal_child TERM "${sc_batch_pid}" "${sc_script_pid}"; then
        sc_unk_err="the TERM could not be sent to the batch child"
      fi
      sc_unk_dest="${sc_fake_pid}"
    else
      # 親が信号を送る宛先はバッチの子。送る前に、他の直接の子が消えて一意に定まるのを待って控える
      if sc_wait_batch "${sc_script_pid}"; then
        sc_unk_dest="${sc_batch_pid}"
      else
        sc_unk_err="the batch child could not be identified"
      fi
      sc_kill_job TERM "${sc_script_pid}" || sc_unk_err="the TERM could not be sent to the script"
    fi
  fi
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_unk_secs=$((SECONDS - sc_t0))
  sc_unk_alive=0
  if [ -n "${sc_fake_pid}" ] && kill -0 "${sc_fake_pid}" 2> /dev/null; then
    sc_unk_alive=1
  fi
  # 偽物の trap は sleep の終わりまで遅れて走るため、少し待ってから記録を読む（誤って送られた信号を取りこぼさない）
  sleep 0.4
  sc_unk_sigs="$(tr '\n' ' ' < "${sc_siglog}" 2> /dev/null | sed 's/ *$//' || true)"
  sc_klog_read "${sc_klog}"
  sc_unk_sent="${sc_kr_sigs}"
  # 宛先: 記録された pid が全て、期待した pid（script 対象はバッチの子、batch 対象は偽物）と一致する。記録が無ければ照合しない。
  sc_unk_dest_ok=1
  for sc_kd in ${sc_kr_pids}; do
    [ "${sc_kd}" = "${sc_unk_dest}" ] || sc_unk_dest_ok=0
  done
  # 後始末: 偽物は止まらず生き続けるので、起動時の開始時刻が一致し、引数が sigtrap.sh を含むときだけ TERM を送る（pid の再利用で別のプロセスへ送らない）
  if [ -n "${sc_fake_start}" ] && [ "$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)" = "${sc_fake_start}" ] && [[ "$(ps -p "${sc_fake_pid}" -o args= 2> /dev/null || true)" == *sigtrap.sh* ]]; then
    kill -TERM "${sc_fake_pid}" 2> /dev/null || true
    # 偽物の trap の記録が次の確認へ混ざらないよう、終わるのを待つ
    sc_waited=0
    while kill -0 "${sc_fake_pid}" 2> /dev/null && [ "${sc_waited}" -lt 50 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
  fi
  sc_unk_out="$(cat "${work_dir}/out" 2> /dev/null || true)"
  if [ -z "${sc_unk_err}" ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_unk_alive}" -eq 1 ] && [ "${sc_unk_secs}" -lt 10 ] && [ -z "${sc_unk_sigs}" ] && [ "${sc_unk_sent}" = "$6" ] && [ "${sc_unk_dest_ok}" -eq 1 ] && [[ "${sc_unk_out}" == *"$4"* ]]; then
    pass "shellcheck-staged: ${1}"
  else
    fail "shellcheck-staged: ${1}" "${sc_unk_err:+setup failed: ${sc_unk_err}; }exit ${sc_rc}, fake still running: ${sc_unk_alive}, seconds: ${sc_unk_secs}, signals received by the fake: ${sc_unk_sigs:-none}, signals the script tried to send: ${sc_unk_sent:-none} to pids [${sc_kr_pids:-none}] (expected: ${6:-none} to ${sc_unk_dest:-no pid})${sc_kj_note}" "output: ${sc_unk_out}"
  fi
}
# 差し込む関数は bash 3.2 でも動く書き方にする（`builtin`・`${var:-}`・case のみ）。sc_pid は run_batch の局所変数で、親では未設定（バッチの子の中だけ設定される）。
# 記録される信号は、本体が kill に渡した最初の引数（-CONT・-TERM）。TERM の前に CONT が先に送られること、所有を確かめられなければどちらも送られないことを確かめる。
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM with an unreadable job table sends no signal and does not wait" 'jobs() { return 1; }' '' "cannot confirm ownership of child" script ""
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM with a job table that only the parent cannot read sends no signal and does not wait" 'jobs() { [ -n "${sc_pid:-}" ] || return 1; builtin jobs "$@"; }' '' "cannot confirm ownership of child" script ""
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM whose signals cannot be sent to a live child does not wait" '' 'return 1' "cannot stop child" script "-CONT"
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM that cannot be sent after a CONT does not wait" '' 'case "$1" in -TERM) return 1 ;; esac' "cannot stop child" script "-CONT -TERM"
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM to a batch child whose kill fails does not wait" '' '[ -z "${sc_pid:-}" ] || return 1' "cannot stop child" batch "-CONT"

# 起動より前に張る早い trap（sc_pending=1）と、停止の trap への差し替え（batch_stop）の間に届いた中断は、保留の旗（sc_pending）で扱う（孫の shellcheck を残さず、2 で終わる）。
# FIFO の open を止め所にする方式は、bash の版・OS によって中断された open をやり直さない（macOS の bash 3.2 は EINTR で失敗する）ため使わない。
# 代わりに、BASH_ENV で差し込む `trap` 関数が、本体の 2 か所の `trap` の呼び出しで、go の印のファイルが現れるまで待つ（外部の sleep の繰り返し。信号は早い trap が受けて旗を立て、待ちを続ける）。
#   1. 起動の前: 早い trap（`sc_pending=1`）を張った直後。shellcheck はまだ起動していない（バッチの子に shellcheck の子が無く、pidfile も無い）ことを確かめてから TERM を送る。
#      早い trap が起動より前に張られていることを見る（起動と pid の記録の後へ動かす変異は、この時点で偽物が起動済みになり落ちる）。
#   2. pid の記録の後・停止の trap への差し替えの前: 差し替え（`batch_stop`）の直前。shellcheck の pid は記録済みで、早い trap が張られたまま。
#      旗の確認が差し替えの後に在ること（確認を差し替えの前へ動かす変異は、旗が立つ前に確認が済み、落ちる）を見る。
# 確かめる範囲（どちらも）:
#   - TERM を送る前に、待ちに入った印（held）があること。TERM の後もバッチの子が生きていること（既定の動作で死んでいない）。
#   - 保留の旗の経路を通った正の証拠: 差し込んだ kill の記録が -CONT -TERM で、宛先が 2 回とも同じ pid で、送った時点の親がバッチの子であり、
#     その pid が残っていないこと（バッチが記録した $! の宛先と消滅）。pidfile が現れた場合は、それとも一致すること。
#   - 2 で終わること、偽物が残らないこと、10 秒未満で終わること。親の pending_signal の区間は対象外（同期できる口が無い）。
# 飛ばす分岐は持たない（どの環境でも同じ経路を試す）。
# sc_hold_case <試験名> <止める trap の第 1 引数> <1=その trap を張った後で止める / 0=張る前で止める>
sc_hold_case() {
  local sc_hold_name="$1" sc_hold_arg="$2" sc_hold_first="$3"
  sc_pidfile="${work_dir}/sc.pid"
  sc_klog="${work_dir}/sc.klog"
  sc_hold_env="${work_dir}/sc-hold-env.sh"
  sc_hold_held="${work_dir}/sc.held"
  sc_hold_go="${work_dir}/sc.go"
  # shellcheck disable=SC2016 # BASH_ENV のファイルへ、展開せずにそのまま書く
  sc_write_kill_env "${sc_hold_env}" 'trap() { if [ "${1:-}" = "${SC_HOLD_ARG}" ]; then [ "${SC_HOLD_INSTALL_FIRST}" != 1 ] || builtin trap "$@"; : > "${SC_HELD}"; _sc_hw=0; while [ ! -e "${SC_GO}" ] && [ "${_sc_hw}" -lt 150 ]; do sleep 0.1; _sc_hw=$((_sc_hw + 1)); done; [ "${SC_HOLD_INSTALL_FIRST}" = 1 ] || builtin trap "$@"; return 0; fi; builtin trap "$@"; }' ""
  rm -f "${sc_pidfile}" "${sc_klog}" "${sc_hold_held}" "${sc_hold_go}"
  : > "${sc_klog}"
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_hold_env}" SC_HOLD_ARG="${sc_hold_arg}" SC_HOLD_INSTALL_FIRST="${sc_hold_first}" SC_HELD="${sc_hold_held}" SC_GO="${sc_hold_go}" SC_KLOG="${sc_klog}" TMPDIR="${work_dir}" SC_PIDFILE="${sc_pidfile}" SC_SYNC_DIR="" SC_BARRIER="" SC_GETCONF_RC="" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_early_err=""
  sc_t0="${SECONDS}"
  sc_kj_note=""
  # 待ちに入った印（pid の記録の後）が現れるまで待ち、他の直接の子が消えてバッチの子が間を置いた 2 回続けて同じ pid で見つかるまで待つ
  sc_waited=0
  while [ ! -e "${sc_hold_held}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if [ ! -e "${sc_hold_held}" ]; then
    sc_early_err="the batch child never reached the point before the trap swap"
  elif ! sc_wait_batch "${sc_script_pid}"; then
    sc_early_err="the batch child could not be identified"
  elif [ "${sc_hold_first}" = 1 ] && { [ -s "${sc_pidfile}" ] || ps -A -o ppid= -o args= 2> /dev/null | awk -v p="${sc_batch_pid}" '$1 == p && $0 ~ /shellcheck/ { f = 1 } END { exit f ? 0 : 1 }'; }; then
    # 早い trap の直後に止めた場合は、この時点で shellcheck がまだ起動していないこと（起動より前に trap が張られている証拠）
    sc_early_err="the fake shellcheck had already started when the early trap was in place"
  elif ! sc_signal_child TERM "${sc_batch_pid}" "${sc_script_pid}"; then
    sc_early_err="the TERM could not be sent to the batch child"
  else
    # 信号の処理が済むのを待ち、バッチの子がまだ生きている（早い trap が旗を立て、待ちを続けている）ことを確かめる
    sleep 0.3
    if ! sc_same_args_children "${sc_script_pid}" | grep -qx "${sc_batch_pid}"; then
      sc_early_err="the batch child did not survive the TERM (it died by the default action)"
    fi
  fi
  # 失敗の経路でも、待ちを解いて回収する
  : > "${sc_hold_go}"
  # 偽物の開始時刻は、pidfile が現れた時点（待つ間の最初の観測）で控える。現れなければ控えず、残っても止めない（偽物の眠りは 30 秒で終わる）。
  sc_fake_pid=""
  sc_fake_start=""
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    if [ -z "${sc_fake_start}" ] && [ -s "${sc_pidfile}" ]; then
      sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
      if [ -n "${sc_fake_pid}" ]; then
        sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
      fi
    fi
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_pend_secs=$((SECONDS - sc_t0))
  if [ -z "${sc_fake_pid}" ]; then
    sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  fi
  sc_klog_read "${sc_klog}"
  sc_pend_sent="${sc_kr_sigs}"
  # 宛先: バッチが記録した shellcheck の pid（$!）は、差し込んだ kill の記録の宛先として試験側で取れる。偽物が書く pidfile には頼らない。
  # 記録の宛先が 2 回（CONT・TERM）とも同じ数字の pid で、送った時点の親が TERM の前に見つけたバッチの子であること。
  # pidfile が現れた場合は、それとも一致すること（偽物が書く pid と、バッチが記録した pid が別なら失敗）。
  sc_pend_dest_ok=0
  sc_pend_target=""
  case "${sc_kr_pids}" in
    [0-9]*" "[0-9]*)
      sc_pend_first="${sc_kr_pids%% *}"
      sc_pend_second="${sc_kr_pids#* }"
      if [ "${sc_pend_first}" = "${sc_pend_second}" ] && [ "${sc_kr_ppids}" = "${sc_batch_pid} ${sc_batch_pid}" ] \
        && { [ -z "${sc_fake_pid}" ] || [ "${sc_fake_pid}" = "${sc_pend_first}" ]; }; then
        sc_pend_dest_ok=1
        sc_pend_target="${sc_pend_first}"
      fi
      ;;
  esac
  # 子の消滅: 宛先（pidfile があればそれも）が残っていないこと
  sc_pend_alive=0
  for sc_pend_p in ${sc_pend_target} ${sc_fake_pid}; do
    sc_waited=0
    while kill -0 "${sc_pend_p}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_pend_p}" 2> /dev/null; then
      sc_pend_alive=1
      # 残った偽物は、pidfile が現れた時点で控えた開始時刻が一致し、まだ sleep であるときだけ止める
      if [ "${sc_pend_p}" = "${sc_fake_pid}" ]; then
        sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
      fi
    fi
  done
  sc_pend_out="$(cat "${work_dir}/out" 2> /dev/null || true)"
  if [ -z "${sc_early_err}" ] && [ "${sc_pend_sent}" = "-CONT -TERM" ] && [ "${sc_pend_dest_ok}" -eq 1 ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_pend_alive}" -eq 0 ] && [ "${sc_pend_secs}" -lt 10 ] \
    && [[ "${sc_pend_out}" == *"no valid result for hang.sh (the check did not finish)"* ]] && [[ "${sc_pend_out}" != *"cannot open the output file of batch 0"* ]]; then
    pass "${sc_hold_name}"
  else
    fail "${sc_hold_name}" "${sc_early_err:+setup failed: ${sc_early_err}; }exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_pend_alive}, seconds: ${sc_pend_secs}, kill calls of the batch child: ${sc_pend_sent:-none} to pids [${sc_kr_pids:-none}] with parents [${sc_kr_ppids:-none}] (expected: -CONT -TERM to one pid, whose parent is the batch child ${sc_batch_pid:-none}, which is gone afterwards)${sc_kj_note}" "output: ${sc_pend_out}"
  fi
}

sc_hold_case "shellcheck-staged: a TERM before the batch starts its shellcheck exits 2 and leaves no process" "sc_pending=1" 1
sc_hold_case "shellcheck-staged: a TERM after the batch records its shellcheck pid and before it swaps its trap exits 2 and leaves no process" batch_stop 0

# 親（本体の本流）の保留の旗 pending_signal。親が旗を見る箇所は 2 つある: 起動のループの中（バッチを 1 つ起動するたび）と、ループの後（2 回目の `trap on_signal` の後）。
# 片方が消えても、もう片方が代わりに旗を見て 2 で終わるため、場合を分けて 1 つずつ確かめる:
#   A. 起動の前（`trap 'pending_signal=1'` の直後。1 バッチ）で止める: 2 つの確認のどちらも使われうる。両方を消す変異で落ちる。
#   B. 起動の前で止め、2 バッチ（2 ファイル）にする: ループの中の確認が働けば、親が回収するのは起動済みの 1 バッチだけ（親が信号を送った子の pid は 1 つ）。
#      ループの中の確認だけを消すと、2 バッチとも起動され、ループの後の確認が 2 つとも止める（宛先が 2 つ）ため落ちる。
#   C. ループの後（2 回目の `trap on_signal` を張る前。バッチは全部起動済み）で止める: ループの後の確認だけが旗を見る。ループの後の確認だけを消すと落ちる。
# 止め所はバッチの起動の前か後で、「起動してから pid を記録するまでの窓」そのものは同期できない（bash で窓の内側に止め所が作れない）。
# 確かめる範囲:
#   - TERM を送る前に、親が待ちに入っていること（A・B はバッチも偽物も未起動）。TERM の後も親が生きていること。
#   - 親が子へ送った信号（差し込んだ kill の記録のうち、送った時点の親が親自身の行）が、1 つの pid への -CONT -TERM であること。
#     記録が無い場合（バッチの子が先に終わった）は、本体が信号を送らずに済む正常な経路だが、この試験では子が眠り続けるため起きない。起きたら失敗にする（記録なしを成功にしない）。
#   - 2 で終わること、10 秒未満であること、親が信号を送った子（バッチの子）と偽物が残らないこと。
#   - 記録（差し込んだ kill の全行）に、3 列目（宛先の親）が空の行が 1 件でもあれば失敗にする（#6147）。親の行だけを `$3 == 親` で選ぶので、
#     `ps` が失敗して親が取れなかった行は黙って外れ、ループの中の確認を消す退行（宛先の数による判別）がまれに素通りするため。
#   - 場合 A・B の「偽物が残らない」の確認は補助である。A・B ではバッチの子が shellcheck を起動する前に止められることが多く、偽物がまだ無くて空振りしやすい。
#     B（ループの中の確認）を判別しているのは、記録のうち親が信号を送った宛先の pid の数（1 つ）である（#6147）。
#   - 場合 C は、TERM を送る前に、親と同じ引数の子（バッチの子）がちょうど 1 つあることを確かめる。本体の `trap on_signal` の数が変わって止め所がずれたら、
#     別の点を黙って試さずに setup failed にするため（#6147）。1 回だけ数えない理由: 止めている間、親は差し込んだ trap の中の `sleep` を繰り返し起動し、
#     `sleep` は exec の前の一瞬、親と同じ引数の fork した bash に見える。このため 2 回続けて同じ pid が 1 つだけ見えたときに決める。
# 止めている親の直接の子のうち、親と同じ引数のもの（バッチの子）がちょうど 1 つで、2 回続けて同じ pid のとき、その pid を出して 0 を返す。決まらなければ 1。
# 止めている間の `sleep` の fork は毎回 pid が違うので、2 回続けて同じ pid にはならない。
sc_hold_one_child() {
  local sc_ho_prev="" sc_ho_cur="" sc_ho_i=0
  while [ "${sc_ho_i}" -lt 50 ]; do
    sc_ho_cur="$(sc_same_args_children "$1" 2> /dev/null || true)"
    if [ "$(printf '%s\n' "${sc_ho_cur}" | grep -c . || true)" = 1 ]; then
      case "${sc_ho_cur}" in
        *[!0-9]*) sc_ho_cur="" ;;
      esac
    else
      sc_ho_cur=""
    fi
    if [ -n "${sc_ho_cur}" ] && [ "${sc_ho_cur}" = "${sc_ho_prev}" ]; then
      printf '%s\n' "${sc_ho_cur}"
      return 0
    fi
    sc_ho_prev="${sc_ho_cur}"
    sleep 0.1
    sc_ho_i=$((sc_ho_i + 1))
  done
  return 1
}
# sc_parent_hold_case <試験名> <止める trap の第 1 引数> <その何回目の呼び出しで止めるか> <1=trap を張った後で止める / 0=張る前で止める> <止め所の確認: prestart=起動前（子も偽物も無い）/ launched=バッチの子がちょうど 1 つ> <引数のファイル名...>
sc_parent_hold_case() {
  local sc_hold_name="$1" sc_hold_arg="$2" sc_hold_nth="$3" sc_hold_first="$4" sc_hold_check="$5"
  shift 5
  sc_pidfile="${work_dir}/sc.pid"
  sc_klog="${work_dir}/sc.klog"
  sc_hold_env="${work_dir}/sc-hold-env.sh"
  sc_hold_held="${work_dir}/sc.held"
  sc_hold_go="${work_dir}/sc.go"
  # shellcheck disable=SC2016 # BASH_ENV のファイルへ、展開せずにそのまま書く
  sc_write_kill_env "${sc_hold_env}" 'trap() { if [ "${1:-}" = "${SC_HOLD_ARG}" ]; then _sc_hn=$((${_sc_hn:-0} + 1)); if [ "${_sc_hn}" = "${SC_HOLD_NTH}" ]; then [ "${SC_HOLD_INSTALL_FIRST}" != 1 ] || builtin trap "$@"; : > "${SC_HELD}"; _sc_hw=0; while [ ! -e "${SC_GO}" ] && [ "${_sc_hw}" -lt 150 ]; do sleep 0.1; _sc_hw=$((_sc_hw + 1)); done; [ "${SC_HOLD_INSTALL_FIRST}" = 1 ] || builtin trap "$@"; return 0; fi; fi; builtin trap "$@"; }' ""
  rm -f "${sc_pidfile}" "${sc_klog}" "${sc_hold_held}" "${sc_hold_go}"
  : > "${sc_klog}"
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_hold_env}" SC_HOLD_ARG="${sc_hold_arg}" SC_HOLD_NTH="${sc_hold_nth}" SC_HOLD_INSTALL_FIRST="${sc_hold_first}" SC_HELD="${sc_hold_held}" SC_GO="${sc_hold_go}" SC_KLOG="${sc_klog}" TMPDIR="${work_dir}" SC_PIDFILE="${sc_pidfile}" SC_SYNC_DIR="" SC_BARRIER="" SC_GETCONF_RC="" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" "$@" > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_early_err=""
  sc_t0="${SECONDS}"
  sc_kj_note=""
  sc_hold_batch=""
  sc_waited=0
  while [ ! -e "${sc_hold_held}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if [ ! -e "${sc_hold_held}" ]; then
    sc_early_err="the parent never reached the point where it was to be held"
  elif [ "${sc_hold_check}" != prestart ] && [ "${sc_hold_check}" != launched ]; then
    sc_early_err="unknown stop-point check: ${sc_hold_check}"
  elif [ "${sc_hold_check}" = launched ] && ! sc_hold_batch="$(sc_hold_one_child "${sc_script_pid}")"; then
    sc_early_err="the parent was not held with exactly one batch child (the stop point moved)"
  elif [ "${sc_hold_check}" = prestart ] && { [ -n "$(sc_same_args_children "${sc_script_pid}" || true)" ] || [ -s "${sc_pidfile}" ]; }; then
    sc_early_err="a batch or the fake shellcheck had already started"
  elif ! sc_kill_job TERM "${sc_script_pid}"; then
    sc_early_err="the TERM could not be sent to the parent"
  else
    sleep 0.3
    if ! kill -0 "${sc_script_pid}" 2> /dev/null; then
      sc_early_err="the parent did not survive the TERM (it died by the default action)"
    fi
  fi
  : > "${sc_hold_go}"
  sc_fake_pid=""
  sc_fake_start=""
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    if [ -z "${sc_fake_start}" ] && [ -s "${sc_pidfile}" ]; then
      sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
      if [ -n "${sc_fake_pid}" ]; then
        sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
      fi
    fi
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_pend_secs=$((SECONDS - sc_t0))
  if [ -z "${sc_fake_pid}" ]; then
    sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  fi
  # 親が子へ送った信号: 記録の 3 列目（送った時点の宛先の親）が親自身（sc_script_pid）の行だけを取り出す
  sc_pp_sigs="$(awk -v p="${sc_script_pid}" '$3 == p { printf "%s%s", (n++ ? " " : ""), $1 }' "${sc_klog}" 2> /dev/null || true)"
  sc_pp_pids="$(awk -v p="${sc_script_pid}" '$3 == p { printf "%s%s", (n++ ? " " : ""), $2 }' "${sc_klog}" 2> /dev/null || true)"
  # 記録の全行のうち 3 列目が空の行の数。読めない・awk の失敗は error（文字列で 0 と比べ、0 のときだけ合格）
  sc_kl_gaps=error
  sc_kl_all="(unreadable)"
  if [ -f "${sc_klog}" ]; then
    sc_kl_gaps="$(awk 'NF < 3 { n++ } END { print n + 0 }' "${sc_klog}" 2> /dev/null || echo error)"
    sc_kl_all="$(awk '{ printf "%s%s %s %s", (n++ ? "; " : ""), ($1 == "" ? "(no signal)" : $1), ($2 == "" ? "(no pid)" : $2), ($3 == "" ? "(no parent)" : $3) } END { if (!n) printf "(empty)" }' "${sc_klog}" 2> /dev/null || echo "(unreadable)")"
  fi
  sc_pp_ok=0
  sc_pp_target=""
  case "${sc_pp_pids}" in
    [0-9]*" "[0-9]*)
      if [ "${sc_pp_sigs}" = "-CONT -TERM" ] && [ "${sc_pp_pids%% *}" = "${sc_pp_pids#* }" ]; then
        sc_pp_ok=1
        sc_pp_target="${sc_pp_pids%% *}"
      fi
      ;;
  esac
  sc_pend_alive=0
  # 偽物の確認は A・B では補助（偽物が未起動で空振りしやすい。判別は宛先の数）。C では偽物も見る
  for sc_pend_p in ${sc_pp_target} ${sc_fake_pid}; do
    sc_waited=0
    while kill -0 "${sc_pend_p}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_pend_p}" 2> /dev/null; then
      sc_pend_alive=1
      if [ "${sc_pend_p}" = "${sc_fake_pid}" ]; then
        sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
      fi
    fi
  done
  sc_tgt_ok=1
  if [ "${sc_hold_check}" = launched ] && [ "${sc_pp_target}" != "${sc_hold_batch}" ]; then
    sc_tgt_ok=0
  fi
  if [ -z "${sc_early_err}" ] && [ "${sc_kl_gaps}" = 0 ] && [ "${sc_tgt_ok}" -eq 1 ] && [ "${sc_pp_ok}" -eq 1 ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_pend_alive}" -eq 0 ] && [ "${sc_pend_secs}" -lt 10 ]; then
    pass "${sc_hold_name}"
  else
    fail "${sc_hold_name}" "${sc_early_err:+setup failed: ${sc_early_err}; }exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_pend_alive}, seconds: ${sc_pend_secs}, signals the parent sent: ${sc_pp_sigs:-none} to pids [${sc_pp_pids:-none}] (expected: -CONT -TERM to one pid), kill log rows without a parent: ${sc_kl_gaps}, batch child at the stop point: ${sc_hold_batch:-none}, kill log: ${sc_kl_all}" "output: $(cat "${work_dir}/out" 2> /dev/null || true)"
  fi
}
sc_parent_hold_case "shellcheck-staged: a TERM to the parent before it starts the batches stops the batches and exits 2" "pending_signal=1" 1 1 prestart hang.sh
sc_parent_hold_case "shellcheck-staged: a TERM to the parent before it starts two batches stops only the batch started so far (the check inside the launch loop)" "pending_signal=1" 1 1 prestart hang.sh hang2.sh
sc_parent_hold_case "shellcheck-staged: a TERM to the parent after it launched every batch stops them and exits 2 (the check after the launch loop)" "on_signal" 2 0 launched hang.sh

# CONT が成功した直後に子が終わった（ジョブ表から消えた）場合は、TERM を送らない（pid が再利用されうるため。CONT と TERM の間の job_state）。
# 差し込む jobs が、kill の記録に -CONT が現れた後は空の表を返す。本物の子は生きているため、記録に -TERM が現れないことを確かめた後、試験が偽物を止めて本体の wait を解く。
sc_cont_gone_case() {
  local sc_hold_name="$1"
  sc_pidfile="${work_dir}/sc.pid"
  sc_klog="${work_dir}/sc.klog"
  sc_hold_env="${work_dir}/sc-hold-env.sh"
  sc_hold_held="${work_dir}/sc.held"
  sc_hold_go="${work_dir}/sc.go"
  # shellcheck disable=SC2016 # BASH_ENV のファイルへ、展開せずにそのまま書く
  sc_write_kill_env "${sc_hold_env}" 'trap() { if [ "${1:-}" = batch_stop ]; then : > "${SC_HELD}"; _sc_hw=0; while [ ! -e "${SC_GO}" ] && [ "${_sc_hw}" -lt 150 ]; do sleep 0.1; _sc_hw=$((_sc_hw + 1)); done; fi; builtin trap "$@"; }
jobs() { if grep -q "^-CONT" "${SC_KLOG}" 2> /dev/null; then return 0; fi; builtin jobs "$@"; }' ""
  rm -f "${sc_pidfile}" "${sc_klog}" "${sc_hold_held}" "${sc_hold_go}"
  : > "${sc_klog}"
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_hold_env}" SC_HELD="${sc_hold_held}" SC_GO="${sc_hold_go}" SC_KLOG="${sc_klog}" TMPDIR="${work_dir}" SC_PIDFILE="${sc_pidfile}" SC_SYNC_DIR="" SC_BARRIER="" SC_GETCONF_RC="" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_early_err=""
  sc_t0="${SECONDS}"
  sc_kj_note=""
  sc_waited=0
  while [ ! -e "${sc_hold_held}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if [ ! -e "${sc_hold_held}" ]; then
    sc_early_err="the batch child never reached the point before the trap swap"
  elif ! sc_wait_batch "${sc_script_pid}"; then
    sc_early_err="the batch child could not be identified"
  elif ! sc_signal_child TERM "${sc_batch_pid}" "${sc_script_pid}"; then
    sc_early_err="the TERM could not be sent to the batch child"
  fi
  : > "${sc_hold_go}"
  # 記録に -CONT が現れるのを待ち、TERM が（誤って）続く時間を置いてから、偽物を止めて本体の wait を解く
  sc_waited=0
  while ! grep -q '^-CONT' "${sc_klog}" 2> /dev/null && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sleep 0.5
  sc_klog_read "${sc_klog}"
  sc_pend_sent="${sc_kr_sigs}"
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  sc_fake_start=""
  if [ -n "${sc_fake_pid}" ]; then
    sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
  fi
  sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_pend_secs=$((SECONDS - sc_t0))
  # 本体が終わった後の最終の記録も読み直す（待っている間に TERM が遅れて送られていないこと）
  sc_klog_read "${sc_klog}"
  sc_pend_sent="${sc_kr_sigs}"
  sc_pend_alive=0
  if [ -n "${sc_fake_pid}" ] && kill -0 "${sc_fake_pid}" 2> /dev/null; then
    sc_pend_alive=1
  fi
  if [ -z "${sc_early_err}" ] && [ -n "${sc_fake_pid}" ] && [ "${sc_pend_sent}" = "-CONT" ] && [ "${sc_kr_pids}" = "${sc_fake_pid}" ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_pend_alive}" -eq 0 ]; then
    pass "${sc_hold_name}"
  else
    fail "${sc_hold_name}" "${sc_early_err:+setup failed: ${sc_early_err}; }exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_pend_alive}, kill calls of the batch child: ${sc_pend_sent:-none} to pids [${sc_kr_pids:-none}] (expected: -CONT only, to the fake pid)" "output: $(cat "${work_dir}/out" 2> /dev/null || true)"
  fi
}
sc_cont_gone_case "shellcheck-staged: a child that ends right after the CONT does not get a TERM"

finish "part 11"
