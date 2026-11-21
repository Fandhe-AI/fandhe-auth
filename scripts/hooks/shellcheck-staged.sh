#!/usr/bin/env bash
# staged のシェルスクリプトを、ファイルごとに並列の shellcheck で検査する（lefthook の pre-commit から呼ぶ）。
#
# 使い方: shellcheck-staged.sh <file>...
#   - 引数が 0 件なら何もせず 0 で終わる（lefthook は glob に一致するファイルが無ければジョブを起動しない）。
#   - 終了コード: 0 = 全件成功、1 = 指摘だけ（shellcheck が 1 を返したものだけ）、
#     2 = 検査不能（shellcheck が PATH に無い・結果が無い・0 と 1 以外・名前が空）。2 は 1 より優先する（未検査で成功にしない）。
#   - 同じ名前の引数は 1 回だけ検査する（lefthook は glob を 2 つ書くと、両方に一致するファイルを 2 回渡す）。
#   - 並列数は getconf _NPROCESSORS_ONLN から決める。取れない・数でない値は固定値 4 にする（上限 16）。
#     nproc と `xargs -P 0` は使わない（macOS で当てにならない）。
#   - 大きいファイルから先に起動する。出力はファイルごとに一時ファイルへ受け、終わってから引数の順に出す（並列の出力が混ざらない）。
#   - 設定は .shellcheckrc（source-path・external-sources）に従う。ファイル名は引数のまま `--` の後ろで渡す。
#
# 目的は pre-commit の時間予算（.claude/rules/git-hooks.md。TASK-6）。macOS 標準の bash 3.2 でも動くように、
# mapfile・wait -n・空の配列の展開・`cmd || rc=$?` を使わない。
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
trap 'exit 2' INT TERM

# 重複を除いた引数を files に集める（名前の完全一致。引数は高々数百件で、二重の走査で足りる）
files=()
total=0
for file in "$@"; do
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

# 大きいファイルから先に起動する（検査時間はほぼ大きさに比例し、最後に起動した大きなファイルが全体の時間を決めるため）。
# 並べ替えの入力は番号だけにし、ファイル名を使わない。出力は引数の順（番号順）のまま。
i=0
while [ "${i}" -lt "${total}" ]; do
  if size="$({ wc -c < "${files[i]}"; } 2> /dev/null)"; then
    size="${size//[!0-9]/}"
  else
    size=0
  fi
  printf '%s %s\n' "${size:-0}" "${i}"
  i=$((i + 1))
done | sort -rn -k1,1 -k2,2n > "${tmp_dir}/order" || {
  echo "shellcheck-staged: cannot order the files" >&2
  exit 2
}

# check_one <番号> <ファイル>: 出力と終了コードを番号付きの一時ファイルへ書く。番号は親が持ち、子には引数で渡す。
# ファイル名が空なら何も書かずに戻る（結果が無いものは下で失敗になる。名前の無いまま検査が 0 で終わる経路を作らない）。
check_one() {
  local rc
  if [ -z "$2" ]; then
    return 0
  fi
  if shellcheck -- "$2" > "${tmp_dir}/$1.out" 2>&1; then
    rc=0
  else
    rc=$?
  fi
  echo "${rc}" > "${tmp_dir}/$1.rc"
}

# bash だけでジョブを回す（xargs は BSD と GNU で引数の扱いが違い、macOS で子にファイル名が渡らなかった）。
# 上限まで起動したら最も古いものを待つ（wait -n は bash 3.2 に無い）。結果は wait の戻り値ではなく .rc から読む。
pids=()
launched=0
reaped=0
while read -r _size idx; do
  check_one "${idx}" "${files[idx]}" &
  pids[launched]=$!
  launched=$((launched + 1))
  if [ $((launched - reaped)) -ge "${max_jobs}" ]; then
    wait "${pids[reaped]}" || true
    reaped=$((reaped + 1))
  fi
done < "${tmp_dir}/order"
while [ "${reaped}" -lt "${launched}" ]; do
  wait "${pids[reaped]}" || true
  reaped=$((reaped + 1))
done

# 終了コード（git-hooks.md の (a)。検査不能を「指摘あり」と同じにしない）: 0 = 全件成功、1 = 指摘だけ（shellcheck が 1）、
# 2 = 検査不能が 1 件でもある（.rc が無い・数字でない・0 と 1 以外・名前が空）。2 は 1 より優先する。
findings=0
unchecked=0
i=0
while [ "${i}" -lt "${total}" ]; do
  if [ -s "${tmp_dir}/${i}.out" ]; then
    cat "${tmp_dir}/${i}.out" || {
      echo "shellcheck-staged: cannot read the output for ${files[i]}" >&2
      exit 2
    }
  fi
  rc_text=""
  if [ -s "${tmp_dir}/${i}.rc" ]; then
    rc_text="$(cat "${tmp_dir}/${i}.rc")"
  fi
  case "${rc_text}" in
    0) ;;
    1) findings=1 ;;
    '' | *[!0-9]*)
      unchecked=1
      echo "shellcheck-staged: no valid result for ${files[i]} (the check did not finish)" >&2
      ;;
    *)
      unchecked=1
      echo "shellcheck-staged: shellcheck could not check ${files[i]} (exit ${rc_text})" >&2
      ;;
  esac
  i=$((i + 1))
done
if [ "${unchecked}" -ne 0 ]; then
  exit 2
fi
exit "${findings}"
