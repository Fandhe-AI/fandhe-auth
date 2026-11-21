#!/usr/bin/env bash
# staged のシェルスクリプトを、バッチに分けて並列の shellcheck で検査する（lefthook の pre-commit から呼ぶ）。
#
# 使い方: shellcheck-staged.sh <file>...
#   - 引数が 0 件なら何もせず 0 で終わる（lefthook は glob に一致するファイルが無ければジョブを起動しない）。
#   - 終了コード: 0 = 全件成功、1 = 指摘だけ（shellcheck が 1 を返したものだけ）、
#     2 = 検査不能（shellcheck が PATH に無い・バッチの結果が無い・0 と 1 以外・名前が空・道具の失敗）。2 は 1 より優先する（未検査で成功にしない）。
#   - 同じ名前の引数は 1 回だけ検査する（lefthook は glob を 2 つ書くと、両方に一致するファイルを 2 回渡す）。
#   - 並列数は getconf _NPROCESSORS_ONLN から決める。取れない・数でない値は固定値 4 にする（上限 16）。
#     nproc と `xargs -P 0` は使わない（macOS で当てにならない）。
#   - 対象を最大 max_jobs 個のバッチに分け、バッチごとに shellcheck を 1 回だけ起動する（起動の回数をファイル数に比例させない。
#     git-hooks.md）。大きさは wc -c を 1 回だけ呼んで取り、大きいファイルから順に、合計の小さいバッチへ入れる（貪欲法）。
#     大きさが取れなければ（読めないファイル・改行を含む名前）、大きさを揃えて割り振るだけで、検査の範囲は変わらない。
#   - 出力はバッチごとに一時ファイルへ受け、終わってからバッチの番号順に出す（並列の出力が混ざらない。順は引数の順ではない）。
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
    if [ "${got_lines}" -lt "${total}" ]; then
      parsed[got_lines]="${wc_size}"
    fi
    got_lines=$((got_lines + 1))
  done <<< "${wc_out}"
  if [ "${numeric}" -eq 1 ] && [ "${got_lines}" -eq "${want_lines}" ]; then
    i=0
    while [ "${i}" -lt "${total}" ]; do
      sizes[i]="${parsed[i]}"
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

# run_batch <バッチの番号>: バッチの全ファイルを 1 回の shellcheck に渡し、出力と終了コードを番号付きの一時ファイルへ書く。
# 番号は親が持ち、ファイル名は引数のまま `--` の後ろへ置く（名前を sh のコードに埋めない）。
run_batch() {
  local rc idx n=0 args=()
  for idx in ${batch_members[$1]}; do
    args[n]="${files[idx]}"
    n=$((n + 1))
  done
  if shellcheck -- "${args[@]}" > "${tmp_dir}/$1.out" 2>&1; then
    rc=0
  else
    rc=$?
  fi
  echo "${rc}" > "${tmp_dir}/$1.rc"
}

# バッチは全部いっぺんに起動する（数は max_jobs 以下）。結果は wait の戻り値ではなく .rc から読む。
pids=()
b=0
while [ "${b}" -lt "${batches}" ]; do
  run_batch "${b}" &
  pids[b]=$!
  b=$((b + 1))
done
b=0
while [ "${b}" -lt "${batches}" ]; do
  wait "${pids[b]}" || true
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
