#!/usr/bin/env bash
# staged のシェルスクリプトを、ファイルごとに並列の shellcheck で検査する（lefthook の pre-commit から呼ぶ）。
#
# 使い方: shellcheck-staged.sh <file>...
#   - 引数が 0 件なら何もせず 0 で終わる（lefthook は glob に一致するファイルが無ければジョブを起動しない）。
#   - 1 ファイルでも指摘があれば 1、shellcheck が PATH に無ければ 2 で終わる（未検査で成功にしない）。
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

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/shellcheck-staged.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT
trap 'exit 1' INT TERM

# 重複を除いた引数を files に集める（名前の完全一致。引数は高々数百件で、二重の走査で足りる）
files=()
for file in "$@"; do
  duplicate=0
  for prev in ${files[@]+"${files[@]}"}; do
    if [ "${prev}" = "${file}" ]; then
      duplicate=1
      break
    fi
  done
  if [ "${duplicate}" -eq 0 ]; then
    files[${#files[@]}]="${file}"
  fi
done
total=${#files[@]}

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
done | sort -rn -k1,1 -k2,2n > "${tmp_dir}/order"

# 起動は xargs に任せる（-0 -n 2 -P は GNU・BSD とも使える。空いた枠からすぐ次を起動し、先頭の遅いファイルが列を止めない）。
# 入力は「番号 NUL ファイル名 NUL」の列で、子は番号ごとの .out と .rc を tmp_dir に書く（$0 = tmp_dir、$1 = 番号、$2 = ファイル名）。
# 子は検査の結果にかかわらず 0 で終わり、xargs の終了コードには頼らない。結果は下で .rc から読む。
# shellcheck disable=SC2016 # 子の bash に渡す固定のコード（親では展開させない）
while read -r _size idx; do
  printf '%s\0%s\0' "${idx}" "${files[idx]}"
done < "${tmp_dir}/order" \
  | xargs -0 -n 2 -P "${max_jobs}" bash -c '
    if shellcheck -- "$2" > "$0/$1.out" 2>&1; then
      rc=0
    else
      rc=$?
    fi
    echo "${rc}" > "$0/$1.rc"
  ' "${tmp_dir}" || true

status=0
i=0
while [ "${i}" -lt "${total}" ]; do
  if [ -s "${tmp_dir}/${i}.out" ]; then
    cat "${tmp_dir}/${i}.out"
  fi
  # rc ファイルが無い・0 でないものは失敗にする（途中で落ちた検査を成功にしない）
  if [ ! -s "${tmp_dir}/${i}.rc" ] || [ "$(cat "${tmp_dir}/${i}.rc")" != "0" ]; then
    status=1
  fi
  i=$((i + 1))
done
exit "${status}"
