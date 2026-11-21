#!/usr/bin/env bash
# staged のシェルスクリプトを、ファイルごとに並列の shellcheck で検査する（lefthook の pre-commit から呼ぶ）。
#
# 使い方: shellcheck-staged.sh <file>...
#   - 引数が 0 件なら何もせず 0 で終わる（lefthook は glob に一致するファイルが無ければジョブを起動しない）。
#   - 1 ファイルでも指摘があれば 1、shellcheck が PATH に無ければ 2 で終わる（未検査で成功にしない）。
#   - 並列数は getconf _NPROCESSORS_ONLN から決める。取れない・数でない値は固定値 4 にする（上限 16）。
#     nproc と `xargs -P 0` は使わない（macOS で当てにならない）。
#   - 出力はファイルごとに一時ファイルへ受け、終わってから引数の順に出す（並列の出力が混ざらない）。
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

# check_one <番号> <ファイル>: 出力と終了コードを番号付きの一時ファイルへ書く
check_one() {
  local rc
  if shellcheck -- "$2" > "${tmp_dir}/$1.out" 2>&1; then
    rc=0
  else
    rc=$?
  fi
  echo "${rc}" > "${tmp_dir}/$1.rc"
}

pids=()
total=0
reaped=0
for file in "$@"; do
  check_one "${total}" "${file}" &
  pids[total]=$!
  total=$((total + 1))
  # 同時に走らせる数を上限に収める。最も古いものを待つ（wait -n は bash 3.2 に無い）。終了コードは rc ファイルから読む。
  if [ $((total - reaped)) -ge "${max_jobs}" ]; then
    wait "${pids[reaped]}" || true
    reaped=$((reaped + 1))
  fi
done
while [ "${reaped}" -lt "${total}" ]; do
  wait "${pids[reaped]}" || true
  reaped=$((reaped + 1))
done

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
