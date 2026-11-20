#!/usr/bin/env bash
# 秘密情報検査の CI 向け入口（TASK-6。2 段目で CI の secret-scan ジョブから呼ぶ予定〔#51〕。ローカルからも同じ形で呼べる）。
#   scripts/secret-scan-ci.sh [<走査対象のリポジトリのパス>]   既定は本スクリプトのあるリポジトリ
# 検出器・陰性対照は本スクリプトと同じリポジトリ（CI では PR が書き換えられない base の版）のものを使い、
# 走査対象はデータとして渡す。公開ログには規則 ID・件数・固定の診断文だけを出す。
# 検出器の出力は一度ファイルへ受け、規則 ID・件数の行（--summary の出力形）だけを通す。ファイル名・値・
# git のエラー文は出さない（パスに値が含まれ得る。SECR-23）。
# 終了コード: 0 = 検出なし・陰性対照も期待どおり / 1 = 検出または陰性対照の失敗 / 2 = 検査を完了できない。
# 範囲外: コミット履歴・submodule の中。検出時は .claude/rules/security.md の重大度「高」に従う。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

if [ "$#" -gt 1 ]; then
  echo "usage: secret-scan-ci.sh [<target-repo-path>]" >&2
  exit 2
fi
target="${1:-${ROOT_DIR}}"
if [ ! -d "${target}" ]; then
  echo "secret-scan-ci: target is not a directory" >&2
  exit 2
fi
target="$(cd "${target}" && pwd -P)"
# 親のリポジトリを誤って走査しないよう、対象がリポジトリのルートそのものであることを確かめる（fail-closed）。
top="$(git -C "${target}" rev-parse --show-toplevel 2>/dev/null)" || top=""
if [ -z "${top}" ] || [ "$(cd "${top}" && pwd -P)" != "${target}" ]; then
  echo "secret-scan-ci: target is not the root of a git repository" >&2
  exit 2
fi

out="$(mktemp "${TMPDIR:-/tmp}/secret-scan-ci.XXXXXX")" || {
  echo "secret-scan-ci: mktemp failed" >&2
  exit 2
}
trap 'rm -f "${out}"' EXIT

rc=0
(cd "${target}" && "${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked --summary) > "${out}" 2>&1 || rc=$?
# 規則 ID は許可した一覧との完全一致だけを通す（任意の文字列・git の診断を公開ログへ通さない）。
grep -aE '^secret-scan: (env-file|vendor-token|private-key|jwt|jwk-private|credential-url|assignment): [0-9]+ (line|file)\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) in [0-9]+ file\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) \([0-9]+ tracked file\(s\) checked\)$|^secret-scan: findings in tracked files$' "${out}" || true
if [ "${rc}" -ne 0 ]; then
  echo "secret-scan-ci: scan failed (exit ${rc}); details are withheld on the public log. Re-run locally: make secret-scan" >&2
  exit "${rc}"
fi

"${ROOT_DIR}/scripts/secret-scan-canary.sh"
