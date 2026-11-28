#!/usr/bin/env bash
# test-hooks.sh の各 part が共有する最小の準備（TASK-6）。test-hooks-lib.sh と test-hooks-9.sh〜test-hooks-11.sh が source する。
#
# 単体では実行しない。使い捨ての作業領域（work_dir）・git の隔離・pass / fail の計数・finish だけを持つ。
# 秘密情報の検出器の試験の準備（git シム・例示値・検出器の複製・check_scan 等）は test-hooks-lib.sh にあり、
# それを使わない part（shellcheck-staged の試験）はこのファイルだけを source する。
# 理由: shellcheck は `external-sources` で source 先を辿って解析し直すため、source 先が大きいと、
# それを source する part の数だけ検査の時間が増える（pre-commit の時間予算。.claude/rules/git-hooks.md）。
# 各 part は別の bash として起動され、状態（変数・関数・cwd）は part の間で引き継がない。
# 計数は finish が TEST_HOOKS_COUNTS_FILE へ書き、入口（test-hooks.sh）が合算する。
#
# 変数・関数は source した側の part が参照するため、単独で未使用に見える定義には、宣言ごとに理由付きの disable を置く。
# scripts/lib.sh は source しない（part・helper は使わない。source すると shellcheck が辿って part ごとに解析し直す）。
set -euo pipefail
# 相対パスの cd が CDPATH に左右されない（CDPATH=. で cd が移動先を標準出力へ出し DIR が 2 行になる）ように、DIR の解決より前に外す（#6173）。
unset CDPATH
# shellcheck disable=SC2034 # part・test-hooks-lib.sh から参照する（本ファイルだけを検査すると未使用に見える）
HOOKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck disable=SC2034 # part・test-hooks-lib.sh から参照する（本ファイルだけを検査すると未使用に見える）
HOOK_BASH="${BASH}"

# テストハーネス自身の文字列比較はバイト単位で行う。フックは test-hooks-lib.sh の UTF-8 ロケールで起動する。
export LC_ALL=C

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/test-hooks.XXXXXX")"
# 権限を落とした試験用ディレクトリ（chmod 000）がシグナルで止まった後も残らないよう、消す前に権限を戻す。
trap 'chmod -R u+rwx "${work_dir}" 2> /dev/null; rm -rf "${work_dir}"' EXIT

# 開発者・CI ランナーの git 設定（署名・色・フック・テンプレート）から隔離する。
# lefthook 経由で起動された場合に備え、親リポジトリを指す環境変数も外す。
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
real_home="${HOME}"
export HOME="${work_dir}/home"
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=hook-test GIT_AUTHOR_EMAIL=hook-test@example.invalid
export GIT_COMMITTER_NAME=hook-test GIT_COMMITTER_EMAIL=hook-test@example.invalid
mkdir -p "${HOME}"

passed=0
failed=0

pass() {
  passed=$((passed + 1))
  echo "ok - $1"
}

fail() {
  failed=$((failed + 1))
  echo "not ok - $1"
  shift
  local detail
  for detail in ${@+"$@"}; do
    echo "    ${detail}"
  done
}

# finish <part の名前>: part の計数を出し、入口へ渡す。失敗があれば 1 で終わる。
# 入口は TEST_HOOKS_COUNTS_FILE に計数が書かれていない part を失敗として扱う（途中で止まった part を成功にしない）。
finish() {
  echo "# $1: passed ${passed}, failed ${failed}"
  if [ -n "${TEST_HOOKS_COUNTS_FILE:-}" ]; then
    echo "${passed} ${failed}" >> "${TEST_HOOKS_COUNTS_FILE}"
  fi
  if [ "${failed}" -ne 0 ]; then
    exit 1
  fi
}
