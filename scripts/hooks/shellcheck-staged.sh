#!/usr/bin/env bash
# staged のシェルスクリプトを、バッチに分けて並列の shellcheck で検査する（lefthook の pre-commit から呼ぶ）。
#
# 使い方: shellcheck-staged.sh <file>...
#   - 引数が 0 件なら何もせず 0 で終わる（lefthook は glob に一致するファイルが無ければジョブを起動しない）。
#   - 終了コード: 0 = 全件成功、1 = 指摘だけ（shellcheck が 1 を返したものだけ）、
#     2 = 検査不能（shellcheck が PATH に無い・バッチの結果が無い・出力先を開けない・0 と 1 以外・名前が空・道具の失敗）。2 は 1 より優先する（未検査で成功にしない）。
#   - 同じ名前の引数は 1 回だけ検査する（lefthook は glob を 2 つ書くと、両方に一致するファイルを 2 回渡す）。
#   - 並列数は getconf _NPROCESSORS_ONLN から決める。取れない・数でない値は固定値 4 にする（上限 16）。
#     nproc と `xargs -P 0` は使わない（macOS で当てにならない）。
#   - 対象を最大 max_jobs 個のバッチに分け、バッチごとに shellcheck を 1 回だけ起動する（起動の回数をファイル数に比例させない。
#     git-hooks.md）。大きさは wc -c を 1 回だけ呼んで取り、大きいファイルから順に、合計の小さいバッチへ入れる（貪欲法）。
#     大きさが取れなければ（読めないファイル・改行を含む名前）、大きさを揃えて割り振るだけで、検査の範囲は変わらない。
#   - 出力はバッチごとに一時ファイルへ受け、終わってからバッチの番号順に出す（並列の出力が混ざらない。順は引数の順ではない）。
#   - 設定は .shellcheckrc（source-path・external-sources）に従う。ファイル名は引数のまま `--` の後ろで渡す。
#
# 目的は pre-commit の時間を縮めること（時間予算は .claude/rules/git-hooks.md。--all-files では予算に未達で、残りは #6056。TASK-6）。macOS 標準の bash 3.2 でも動くように、
# mapfile・wait -n・空の配列の展開・`cmd || rc=$?`・名前付きの記述子（`{fd}>`）を使わない。
# 回帰テストは scripts/hooks/test-hooks-1.sh（偽の shellcheck を PATH に置いて確かめる）。
set -euo pipefail

if [ "$#" -eq 0 ]; then
  exit 0
fi

if ! command -v shellcheck > /dev/null 2>&1; then
  echo "shellcheck-staged: shellcheck not found on PATH" >&2
  exit 2
fi

max_jobs=""
if max_jobs="$(getconf _NPROCESSORS_ONLN 2> /dev/null)"; then
  :
else
  max_jobs=""
fi
case "${max_jobs}" in
  '' | *[!0-9]*) max_jobs=4 ;;
esac
# 桁数を絞ってから数として読む（巨大な数字の列は比較・算術を誤らせる。先頭の 0 は 10# で 10 進として読む）
if [ "${#max_jobs}" -gt 3 ]; then
  max_jobs=4
else
  max_jobs=$((10#${max_jobs}))
fi
if [ "${max_jobs}" -lt 1 ]; then
  max_jobs=4
elif [ "${max_jobs}" -gt 16 ]; then
  max_jobs=16
fi

# 道具の失敗（一時領域・並べ替え・結果の読み出し）は検査不能として 2 で終わる。set -e の既定の 1 は「指摘あり」と区別できない。
if ! tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/shellcheck-staged.XXXXXX")"; then
  echo "shellcheck-staged: cannot create a temporary directory" >&2
  exit 2
fi
trap 'rm -rf "${tmp_dir}"' EXIT
# 中断（INT・TERM・HUP）では、起動済みのバッチに停止を伝え、終わりを待ってから 2 で終わる（EXIT の trap が一時領域を消す）。
# バッチの子は自分の shellcheck を止めてから終わる（run_batch の trap）。起動前なら pids は空で、何もしない。
# 回収した（wait 済みの）pid は pids から外し、再利用された pid に kill を送らない。外すまでの wait 直後の単純なコマンド 1 つ分の窓だけは、bash では閉じられない（#6063・TASK-6）。
pids=()
launched=0
pending_signal=0
# job_state <pid>: bash のジョブ表での子の状態を job_state_result に入れる（owned・none・unknown）。bash は明示の wait の前にも終了した子を回収するため、
# 保持している pid が回収済み（pid が再利用されうる）のことがある。信号を送る前に、生きた自分の子だと確かめる（#6063・TASK-6）。
# ジョブ表は 1 回の `jobs -l` で取る（実行中と停止中を別々の一覧で見ると、その間に状態が変わった子をどちらにも見つけられない）。
# 状態名は bash が翻訳するため、`jobs` は LC_ALL=C で実行して ASCII の名前で照合する（git-hooks.md の (b)）。
# owned = Running か Stopped のジョブとして載っている、none = 載っていない・終了済み（Running・Stopped 以外の空でない状態語。信号で死んだ子の Hangup・Interrupt 等を含む）、
# unknown = 一覧を取れない・読めない・一致した pid の状態語が空（所有を確かめられない）。
# jobs は現在のシェルで実行する（command substitution やパイプラインのサブシェルは、bash 3.2 等で空のジョブ表を見る）。出力はファイルへのリダイレクトで受け、
# >| で開くため継承した noclobber に左右されない。確認と kill の間の窓は残るが、回収済みの pid を無条件に狙う経路は無くなる。
job_state_result=none
job_state() {
  # 関数の局所の LC_ALL: 戻ると元のロケールに戻る
  local LC_ALL=C
  local f="${tmp_dir}/jobs.$1" _jn jp js _jrest
  job_state_result=none
  if ! jobs -l >| "${f}" 2> /dev/null; then
    job_state_result=unknown
    return 0
  fi
  while read -r _jn jp js _jrest; do
    if [ "${jp}" = "$1" ]; then
      case "${js}" in
        Running | Stopped*) job_state_result=owned ;;
        # 状態語が空（行が崩れている）なら解釈できない。それ以外は、信号で死んだ子の状態語（Hangup・Interrupt・Segmentation fault 等）も含め終了済みとして扱う
        '') job_state_result=unknown ;;
        *) job_state_result=none ;;
      esac
    fi
  done < "${f}" || job_state_result=unknown
  # 一覧の削除の失敗で set -e が中断の後始末を飛ばさない
  rm -f "${f}" || true
}
# stop_job <pid>: ジョブ表で自分の子と確かめられたものだけを止める。CONT を先に、TERM を最後に 1 回だけ送る。
# 一時停止中（SIGSTOP 等）の子は CONT が無いと TERM を受け取れず wait で止まる（非対話の bash は停止を検知しないため、状態では分けず必ず CONT を送る）。
# 信号を送るたびに、直前に job_state で生きた自分の子だと確かめ直す。CONT で子が終わって回収されると pid が再利用されうるため、TERM の前にも確かめる。
# 確認と送信の間の窓は bash では閉じられないが、回収済みの pid を無条件に狙う経路は無い。TERM の後には何も送らない。
# 所有を確かめられない（unknown）ときは、無関係のプロセスを止めないよう何も送らず、診断を出して 1 を返す。
# 呼び出し側は、1 のとき wait へ進まない（停止していない子を無期限に待たない）。
stop_job() {
  job_state "$1"
  case "${job_state_result}" in
    owned) ;;
    unknown)
      echo "shellcheck-staged: cannot confirm ownership of child ${1}; no signal sent and not waiting" >&2
      return 1
      ;;
    *) return 0 ;;
  esac
  kill -CONT "$1" 2> /dev/null || true
  job_state "$1"
  case "${job_state_result}" in
    owned) kill "$1" 2> /dev/null || true ;;
    unknown)
      echo "shellcheck-staged: cannot confirm ownership of child ${1}; no signal sent and not waiting" >&2
      return 1
      ;;
  esac
  return 0
}
# shellcheck disable=SC2329 # trap から呼ばれる
on_signal() {
  # 後始末の途中の信号で入れ子に呼ばれると、回収済みの pid へ二重に kill を送り得る
  trap '' INT TERM HUP
  # どの後始末の失敗でも途中で終わらず、最後の exit 2 に必ず届く（終了コード 1 は shellcheck の指摘専用）
  set +e
  local k=0
  while [ "${k}" -lt "${launched}" ]; do
    if [ -n "${pids[k]}" ]; then
      # 所有を確かめられない子には送らず、待ちからも外す（無期限の wait を避ける）
      stop_job "${pids[k]}" || pids[k]=""
    fi
    k=$((k + 1))
  done
  k=0
  while [ "${k}" -lt "${launched}" ]; do
    if [ -n "${pids[k]}" ]; then
      wait "${pids[k]}" 2> /dev/null || true
      pids[k]=""
    fi
    k=$((k + 1))
  done
  exit 2
}
trap on_signal INT TERM HUP

# 重複を除いた引数を files に集める（名前の完全一致。引数は高々数百件で、二重の走査で足りる）。
# 空の名前は検査に回さず、検査不能として扱う（名前の無いまま 0 で終わる経路を作らない）。
files=()
total=0
unchecked=0
empty_seen=0
for file in "$@"; do
  if [ -z "${file}" ]; then
    empty_seen=1
    continue
  fi
  duplicate=0
  for prev in ${files[@]+"${files[@]}"}; do
    if [ "${prev}" = "${file}" ]; then
      duplicate=1
      break
    fi
  done
  if [ "${duplicate}" -eq 0 ]; then
    files[total]="${file}"
    total=$((total + 1))
  fi
done
if [ "${empty_seen}" -eq 1 ]; then
  unchecked=1
  echo "shellcheck-staged: no valid result for an empty file name (nothing was checked)" >&2
fi

# 大きさ（バイト）を wc -c の 1 回の起動で取る。出力は「大きさ 名前」の行がファイルの順に並び、2 件以上なら最後に合計の行が付く。
# 名前は読まず先頭の数だけを使い、行数が合わない（読めないファイル・改行を含む名前）ときは全件の大きさを 0 として扱う。
# 大きさは割り振りの偏りにしか効かず、どのファイルも必ず 1 回検査される。
sizes=()
i=0
while [ "${i}" -lt "${total}" ]; do
  sizes[i]=0
  i=$((i + 1))
done
if [ "${total}" -gt 0 ]; then
  if ! wc_out="$(wc -c -- "${files[@]}" 2> /dev/null)"; then
    wc_out=""
  fi
  want_lines="${total}"
  if [ "${total}" -gt 1 ]; then
    want_lines=$((total + 1))
  fi
  got_lines=0
  numeric=1
  parsed=()
  while read -r wc_size _wc_name; do
    case "${wc_size}" in
      '' | *[!0-9]*) numeric=0 ;;
    esac
    # 15 桁を超える数は大きさとして使わない（算術の桁あふれを避ける）
    if [ "${#wc_size}" -gt 15 ]; then
      numeric=0
    fi
    if [ "${got_lines}" -lt "${total}" ]; then
      parsed[got_lines]="${wc_size}"
    fi
    got_lines=$((got_lines + 1))
  done <<< "${wc_out}"
  if [ "${numeric}" -eq 1 ] && [ "${got_lines}" -eq "${want_lines}" ]; then
    i=0
    while [ "${i}" -lt "${total}" ]; do
      sizes[i]=$((10#${parsed[i]}))
      i=$((i + 1))
    done
  fi
fi

# バッチへの割り振り: 大きいファイルから順に、合計の小さいバッチへ入れる。重みは大きさ + 1（大きさが全部 0 でも偏らない）。
# 並べ替えの入力は番号だけにし、ファイル名を使わない。バッチ数は min(max_jobs, 件数)。
batches=0
batch_sum=()
batch_members=()
if [ "${total}" -gt 0 ]; then
  batches="${max_jobs}"
  if [ "${total}" -lt "${batches}" ]; then
    batches="${total}"
  fi
  b=0
  while [ "${b}" -lt "${batches}" ]; do
    batch_sum[b]=0
    batch_members[b]=""
    b=$((b + 1))
  done
  i=0
  while [ "${i}" -lt "${total}" ]; do
    printf '%s %s\n' "${sizes[i]}" "${i}"
    i=$((i + 1))
  done | sort -rn -k1,1 -k2,2n > "${tmp_dir}/order" || {
    echo "shellcheck-staged: cannot order the files" >&2
    exit 2
  }
  while read -r size idx; do
    best=0
    b=1
    while [ "${b}" -lt "${batches}" ]; do
      if [ "${batch_sum[b]}" -lt "${batch_sum[best]}" ]; then
        best="${b}"
      fi
      b=$((b + 1))
    done
    batch_sum[best]=$((batch_sum[best] + size + 1))
    batch_members[best]="${batch_members[best]} ${idx}"
  done < "${tmp_dir}/order"
fi

# 割り振りの確認: バッチに入れた件数の合計が total と一致し、バッチ数が 1 以上 16 以下であること。
# 違えば、一部のファイルが検査されないまま 0 で終わり得るため、検査不能（2）で止める。
assigned=0
b=0
while [ "${b}" -lt "${batches}" ]; do
  for idx in ${batch_members[b]}; do
    assigned=$((assigned + 1))
  done
  b=$((b + 1))
done
if [ "${assigned}" -ne "${total}" ] || [ "${batches}" -gt 16 ] || { [ "${total}" -gt 0 ] && [ "${batches}" -lt 1 ]; }; then
  echo "shellcheck-staged: internal error: ${assigned} of ${total} file(s) assigned to ${batches} batch(es)" >&2
  exit 2
fi

# run_batch の停止の処理。shellcheck の pid（run_batch の局所変数 sc_pid。動的スコープで見える）が残っていれば止めて回収し、2 で終わる。
# 止められた場合は .rc を書かず、親が「結果が無い」として扱う。
# shellcheck disable=SC2329 # trap から呼ばれる
batch_stop() {
  # 停止処理中の別の信号で再入すると、回収直後の pid へ kill を送り得る。冒頭で対象の信号を無視する。
  trap '' TERM INT HUP
  # どの後始末の失敗でも停止と回収を続け、最後の exit 2 に必ず届く
  set +e
  if [ -n "${sc_pid}" ]; then
    if stop_job "${sc_pid}"; then
      wait "${sc_pid}" 2> /dev/null || true
    fi
    sc_pid=""
  fi
  exit 2
}

# run_batch <バッチの番号>: バッチの全ファイルを 1 回の shellcheck に渡し、出力と終了コードを番号付きの一時ファイルへ書く。
# 番号は親が持ち、ファイル名は引数のまま `--` の後ろへ置く（名前を sh のコードに埋めない）。
run_batch() {
  local rc idx n=0 args=() sc_pid="" sc_pending=0 opened=0
  for idx in ${batch_members[$1]}; do
    args[n]="${files[idx]}"
    n=$((n + 1))
  done
  # 起動の前後は信号を記録するだけにし、pid を記録してから停止の trap に差し替えて旗を見る（記録前の信号で孫を残さない。#6063）。
  trap 'sc_pending=1' TERM INT HUP
  # 出力先は brace group のリダイレクトで 1 回だけ開き、背景の shellcheck へ受け継ぐ（先に作ってから開き直すと、noclobber 等で
  # 2 回目に開けないとき bash は shellcheck を起動せずに 1 を返し、「指摘あり」と取り違える）。開けなければ group の中身は実行されない。
  # 2> /dev/null は group の外のリダイレクト失敗の診断を抑えるだけで、shellcheck の stderr は中の 2>&1 で出力先へ行く。
  if { shellcheck -- "${args[@]}" 2>&1 & sc_pid=$!; } 2> /dev/null > "${tmp_dir}/$1.out"; then
    opened=1
  fi
  trap batch_stop TERM INT HUP
  if [ "${sc_pending}" -eq 1 ]; then
    batch_stop
  fi
  if [ "${opened}" -eq 0 ]; then
    # 検査器は起動していない。診断を分けるため数字でない印を書く（.rc の書き込みまで失敗すれば、親が「結果が無い」として 2 にする）。
    echo noout > "${tmp_dir}/$1.rc"
    return 0
  fi
  if [ -z "${sc_pid}" ]; then
    # 起動に失敗した（pid が無い）。.rc を書かず、親が「結果が無い」として 2 にする。
    return 0
  fi
  if wait "${sc_pid}"; then
    rc=0
  else
    rc=$?
  fi
  # 回収済みの pid に停止の trap が kill を送らないよう外す
  sc_pid=""
  echo "${rc}" > "${tmp_dir}/$1.rc"
}

# バッチは全部いっぺんに起動する（数は max_jobs 以下）。結果は wait の戻り値ではなく .rc から読む。
# 起動のループの間は信号を記録するだけにし、pid の記録の直後に旗を見る（記録前の信号で起動済みのバッチを残さない）。
trap 'pending_signal=1' INT TERM HUP
b=0
while [ "${b}" -lt "${batches}" ]; do
  run_batch "${b}" &
  pids[b]=$!
  launched=$((b + 1))
  b=$((b + 1))
  if [ "${pending_signal}" -eq 1 ]; then
    on_signal
  fi
done
trap on_signal INT TERM HUP
if [ "${pending_signal}" -eq 1 ]; then
  on_signal
fi
b=0
while [ "${b}" -lt "${batches}" ]; do
  wait "${pids[b]}" || true
  pids[b]=""
  b=$((b + 1))
done

# 終了コード（git-hooks.md の (a)。検査不能を「指摘あり」と同じにしない）: 0 = 全バッチが 0、1 = 指摘だけ（1 を返したバッチがあり検査不能は無い）、
# 2 = 検査不能が 1 件でもある（.rc が無い・数字でない・0 と 1 以外・名前が空・道具の失敗）。2 は 1 より優先する。
# 結果の無いバッチは、そのバッチの全ファイルを名前つきで診断する。
findings=0
b=0
while [ "${b}" -lt "${batches}" ]; do
  if [ -s "${tmp_dir}/${b}.out" ]; then
    cat "${tmp_dir}/${b}.out" || {
      echo "shellcheck-staged: cannot read the output of batch ${b}" >&2
      exit 2
    }
  fi
  rc_text=""
  if [ -s "${tmp_dir}/${b}.rc" ]; then
    if ! rc_text="$(cat "${tmp_dir}/${b}.rc")"; then
      echo "shellcheck-staged: cannot read the result of batch ${b}" >&2
      exit 2
    fi
  fi
  case "${rc_text}" in
    0) ;;
    1) findings=1 ;;
    noout)
      unchecked=1
      for idx in ${batch_members[b]}; do
        echo "shellcheck-staged: cannot open the output file of batch ${b}; ${files[idx]} was not checked" >&2
      done
      ;;
    '' | *[!0-9]*)
      unchecked=1
      for idx in ${batch_members[b]}; do
        echo "shellcheck-staged: no valid result for ${files[idx]} (the check did not finish)" >&2
      done
      ;;
    *)
      unchecked=1
      for idx in ${batch_members[b]}; do
        echo "shellcheck-staged: shellcheck could not check ${files[idx]} (exit ${rc_text})" >&2
      done
      ;;
  esac
  b=$((b + 1))
done
if [ "${unchecked}" -ne 0 ]; then
  exit 2
fi
exit "${findings}"
