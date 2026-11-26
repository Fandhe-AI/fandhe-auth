#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テスト（CI の test-hooks ジョブ・`make test-hooks`）。
#
# 一時ディレクトリに使い捨ての git リポジトリを作り、secret-scan.sh の検出・許可ケースと
# commit-msg-check.sh の OK / NG ケースを実際のスクリプトで検証する。
# 基本の外部依存は bash と git。msrv・install-msrv の節は cargo・jq・tomllib を持つ python3（3.11 以上）も要し、
# 前提が足りなければ CI（CI か GITHUB_ACTIONS が設定されている）では失敗、ローカルでは skip する。
# crate-deps の節は cargo が無ければ skip する。フックではなく CI で実行する（.claude/rules/git-hooks.md）。
#
# 回帰ケースは過去に見つかった fail-open の不具合クラスに対応する:
#   (a) 検査エラーを「一致なし」と同一視する
#   (b) 非 UTF-8 の内容でロケール依存の照合が一致しなくなる
#   (c) hunk 内の `++ b/...` で始まる追加行を `+++ b/` ヘッダと誤認する
#   (d) 利用者の git 設定（色・外部 diff・textconv・prefix）で diff の出力形式が変わる
#   (e) git の失敗がパイプ・here-string に隠れる
#
# フックは本スクリプトを実行している bash（${BASH}）で起動する。macOS 標準の bash 3.2 での
# 互換性は `/bin/bash scripts/hooks/test-hooks.sh` で確認できる（本スクリプトも 3.2 互換で書く）。
#
#
# 本ファイルは入口で、試験の本体は part（test-hooks-1.sh 〜 test-hooks-N.sh）と共有部（test-hooks-lib.sh）に分けてある。
# 検査（shellcheck）の時間を縮めるため、バッチに分けて並列に流せる大きさに分けてある（lefthook の pre-commit の時間予算。
# .claude/rules/git-hooks.md。TASK-6）。part は別の bash として順に起動し、計数を合算する。
# part の間で状態（変数・関数・cwd）は引き継がない。part を足したら下の parts にも足す。
#
# 検出させる例示値はすべて実行時に組み立て、本ファイル自体のコミットが secret-scan に
# 検知されないようにする（実体は test-hooks-lib.sh）。
set -euo pipefail
HOOKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib.sh
source "${HOOKS_DIR}/../lib.sh"

reject_args "$(basename "$0")" "$@"

parts="1 2 3 4 5 6 7 8"

# parts が実在する part のファイル（test-hooks-<数字>.sh）と一致しなければ失敗にする（足した part の起動漏れ・消した part の素通りを防ぐ）
part_files=0
for part_file in "${HOOKS_DIR}"/test-hooks-[0-9]*.sh; do
  if [ -e "${part_file}" ]; then
    part_files=$((part_files + 1))
  fi
done
parts_listed=0
seen_parts=" "
for part in ${parts}; do
  # 同じ番号を 2 回書いても件数が合ってしまうため、重複は拒否する
  case "${seen_parts}" in
    *" ${part} "*)
      echo "test-hooks: part ${part} is listed more than once in parts" >&2
      exit 1
      ;;
  esac
  seen_parts="${seen_parts}${part} "
  parts_listed=$((parts_listed + 1))
  if [ ! -e "${HOOKS_DIR}/test-hooks-${part}.sh" ]; then
    echo "test-hooks: part ${part} listed in parts does not exist" >&2
    exit 1
  fi
done
if [ "${parts_listed}" -ne "${part_files}" ]; then
  echo "test-hooks: parts lists ${parts_listed} part(s) but ${part_files} test-hooks-<N>.sh file(s) exist" >&2
  exit 1
fi

counts_dir="$(mktemp -d "${TMPDIR:-/tmp}/test-hooks-counts.XXXXXX")"
trap 'rm -rf "${counts_dir}"' EXIT
# part が計数を追記する先。計数を書かずに終わった part（途中で止まった・早期に 0 で抜けた）は失敗として扱う。
export TEST_HOOKS_COUNTS_FILE="${counts_dir}/counts"
: > "${TEST_HOOKS_COUNTS_FILE}"

crashed=0
exit_mismatch=0
for part in ${parts}; do
  before="$(wc -l < "${TEST_HOOKS_COUNTS_FILE}")"
  part_rc=0
  if "${BASH}" "${HOOKS_DIR}/test-hooks-${part}.sh"; then
    part_rc=0
  else
    part_rc=$?
  fi
  after="$(wc -l < "${TEST_HOOKS_COUNTS_FILE}")"
  if [ "${after}" -ne $((before + 1)) ]; then
    echo "not ok - test-hooks: part ${part} ended without reporting its counts (exit ${part_rc})"
    crashed=$((crashed + 1))
  elif [ "${part_rc}" -ne 0 ]; then
    # 計数のあとに非 0 で終わった part。失敗を報告した part（finish が 1 で終わる）は計数に入っているので足さず、
    # 失敗 0 を報告したのに非 0 で終わったときだけ、計数と終了コードの不一致を 1 件の失敗として足す。
    last_counts="$(tail -n 1 "${TEST_HOOKS_COUNTS_FILE}")"
    if [ "${last_counts##* }" = "0" ]; then
      echo "not ok - test-hooks: part ${part} exited ${part_rc} after reporting no failures"
      exit_mismatch=$((exit_mismatch + 1))
    fi
  fi
done

passed=0
failed=0
while read -r part_passed part_failed; do
  passed=$((passed + part_passed))
  failed=$((failed + part_failed))
done < "${TEST_HOOKS_COUNTS_FILE}"

# 計数を報告せずに終わった part・計数のあとに非 0 で終わった part は、それぞれ 1 件の失敗として足す（not ok の行は上で出した）
failed=$((failed + crashed + exit_mismatch))

echo "# passed: ${passed}, failed: ${failed}"
if [ "${failed}" -ne 0 ]; then
  exit 1
fi
