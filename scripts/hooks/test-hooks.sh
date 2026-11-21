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
# 検査（shellcheck）の時間を、ファイルごとに並列に流せる大きさに収めるため（lefthook の pre-commit の時間予算。
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

parts="1 2 3 4 5 6 7"

counts_dir="$(mktemp -d "${TMPDIR:-/tmp}/test-hooks-counts.XXXXXX")"
trap 'rm -rf "${counts_dir}"' EXIT
# part が計数を追記する先。計数を書かずに終わった part（途中で止まった・早期に 0 で抜けた）は失敗として扱う。
export TEST_HOOKS_COUNTS_FILE="${counts_dir}/counts"
: > "${TEST_HOOKS_COUNTS_FILE}"

crashed=0
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
  fi
done

passed=0
failed=0
while read -r part_passed part_failed; do
  passed=$((passed + part_passed))
  failed=$((failed + part_failed))
done < "${TEST_HOOKS_COUNTS_FILE}"

# 計数を報告せずに終わった part は 1 件の失敗として足す（not ok の行は上で出した）
failed=$((failed + crashed))

echo "# passed: ${passed}, failed: ${failed}"
if [ "${failed}" -ne 0 ]; then
  exit 1
fi
