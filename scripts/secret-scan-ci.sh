#!/usr/bin/env bash
# 秘密情報検査の CI 向け入口（TASK-6。2 段目で CI の secret-scan ジョブから呼ぶ予定〔#51〕。ローカルからも同じ形で呼べる）。
#   scripts/secret-scan-ci.sh [<走査対象のリポジトリのパス>]   既定は本スクリプトのあるリポジトリ
# 検出器・陰性対照は本スクリプトと同じリポジトリ（CI では PR が書き換えられない base の版）のものを使い、
# 走査対象はデータとして渡す。公開ログには規則 ID・件数・固定の診断文だけを出す。
# 走査するのは HEAD を sha に固定して作った専用の一時 index（作業ツリーの index は読まない）。
# 検出器の出力は一度ファイルへ受け、規則 ID・件数の行（--summary の出力形）だけを通す。ファイル名・値・
# git のエラー文は出さない（パスに値が含まれ得る。SECR-23）。
# 終了コード: 0 = 検出なし・陰性対照も期待どおり / 1 = 検出または陰性対照の失敗 / 2 = 検査を完了できない。
# 範囲外: コミット履歴・submodule の中。検出時は .claude/rules/security.md の重大度「高」に従う。
set -euo pipefail
# 相対パスの解決（cd）が CDPATH に左右されないようにする（以降の cd・lib.sh の解決より前に外す）。
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

# 想定外の失敗（set -e 相当）は検出（1）と取り違えないよう、すべて検査不能（2）として止める（fail-closed）。
set -E
trap 'echo "secret-scan-ci: internal error; aborting" >&2; exit 2' ERR

# 呼び出し元の環境が git のリポジトリ・index・設定を差し替えないよう、GIT_* をすべて外す
# （GIT_INDEX_FILE・GIT_DIR・GIT_WORK_TREE 等。対象の確認と本走査を同じ環境で行う）。
# 列挙の失敗を「該当なし」と取り違えないよう、パイプを使わず bash の照合で絞り、失敗は検査不能にする。
if ! env_names="$(compgen -e 2> /dev/null)"; then
  echo "secret-scan-ci: failed to list environment variables" >&2
  exit 2
fi
for _v in ${env_names}; do
  case "${_v}" in
    GIT_*) unset "${_v}" ;;
  esac
done
unset _v env_names
# 置換参照（refs/replace/）でオブジェクトの中身が差し替わらないよう、対象の確定から検出器・陰性対照の実行まで
# 一貫して無効にする（子プロセスへ受け継ぐ）。
export GIT_NO_REPLACE_OBJECTS=1
# git の呼び出しは、設定で外部プログラムを起動し得る経路（hooks・fsmonitor）を無効にして通す。
g() {
  git -c core.hooksPath=/dev/null -c core.fsmonitor=false "$@"
}

if [ "$#" -gt 1 ]; then
  echo "usage: secret-scan-ci.sh [<target-repo-path>]" >&2
  exit 2
fi
# 既定の対象を使うのは引数を省略したときだけ。空文字が明示されたときは取り違えを避けて検査不能にする。
if [ "$#" -eq 0 ]; then
  target="${ROOT_DIR}"
else
  target="$1"
  if [ -z "${target}" ]; then
    echo "secret-scan-ci: target is empty" >&2
    exit 2
  fi
fi
# `-` や `-x` のような値を cd がオプション・OLDPWD として解釈しないよう、絶対パスでなければ `./` を前に付ける。
case "${target}" in
  /*) ;;
  *) target="./${target}" ;;
esac
if [ ! -d "${target}" ]; then
  echo "secret-scan-ci: target is not a directory" >&2
  exit 2
fi
# パスの取得は `$(...)` が末尾の改行を落とすため、番兵（x）を付けて受けてから外す（改行で終わるパスも保つ）。
if ! target="$(cd "${target}" 2> /dev/null && pwd -P && printf x)"; then
  echo "secret-scan-ci: cannot resolve the target directory" >&2
  exit 2
fi
target="${target%x}"
target="${target%$'\n'}"
if [ -z "${target}" ]; then
  echo "secret-scan-ci: cannot resolve the target directory" >&2
  exit 2
fi
# 親のリポジトリを誤って走査しないよう、対象がリポジトリのルートそのものであることを確かめる（fail-closed）。
top="$(g -C "${target}" rev-parse --show-toplevel 2> /dev/null && printf x)" || top=""
top="${top%x}"
top="${top%$'\n'}"
top_real=""
if [ -n "${top}" ]; then
  top_real="$(cd "${top}" 2> /dev/null && pwd -P && printf x)" || top_real=""
  top_real="${top_real%x}"
  top_real="${top_real%$'\n'}"
fi
if [ -z "${top_real}" ] || [ "${top_real}" != "${target}" ]; then
  echo "secret-scan-ci: target is not the root of a git repository" >&2
  exit 2
fi

# 走査対象は作業ツリーの index ではなく、HEAD を sha に固定してその tree から作る専用の一時 index にする。
# 照合（HEAD と index の比較）と走査が index を別々に開く隙間（TOCTOU）で index が書き換わっても、
# 取り違えて 0 を返さない。作業ツリーの index が HEAD と違っても、走査するのは HEAD の内容である。
# git の呼び出しは対象の hooks（post-index-change 等）を起動しない。
if ! head_sha="$(g -C "${target}" rev-parse --verify "HEAD^{commit}" 2> /dev/null)" || [ -z "${head_sha}" ]; then
  echo "secret-scan-ci: cannot resolve HEAD" >&2
  exit 2
fi
if ! work_dir="$(mktemp -d "${TMPDIR:-/tmp}/secret-scan-ci.XXXXXX" 2> /dev/null)" || [ -z "${work_dir}" ]; then
  echo "secret-scan-ci: mktemp failed" >&2
  exit 2
fi
trap 'rm -rf "${work_dir}"' EXIT
out="${work_dir}/out"
tmp_index="${work_dir}/index"
if ! GIT_INDEX_FILE="${tmp_index}" g -C "${target}" read-tree "${head_sha}" > /dev/null 2>&1; then
  echo "secret-scan-ci: failed to build the temporary index from HEAD" >&2
  exit 2
fi

# 検出器の終了状態は 0 = 検出なし / 1 = 検出 / 2 = 検査不能。それ以外（起動失敗・シグナル等）も検査不能に畳む。
rc=0
# cd の失敗（終了状態 1 になり「検出」と取り違えられる）は、サブシェルの中で検査不能（2）にする。
(
  cd "${target}" 2> /dev/null || exit 2
  GIT_INDEX_FILE="${tmp_index}" "${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked --summary
) > "${out}" 2>&1 || rc=$?
# 規則 ID は許可した一覧との完全一致だけを通す（任意の文字列・git の診断を公開ログへ通さない）。
# grep の終了状態は 0 = 一致 / 1 = 該当なし / 2 以上 = エラー。1 だけを「該当なし」として許す。
grep_rc=0
grep -aE '^secret-scan: (env-file|vendor-token|private-key|jwt|jwk-private|credential-url|assignment): [0-9]+ (line|file)\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) in [0-9]+ file\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) \([0-9]+ tracked file\(s\) checked\)$|^secret-scan: findings in tracked files$' "${out}" 2> /dev/null || grep_rc=$?
if [ "${grep_rc}" -gt 1 ]; then
  echo "secret-scan-ci: failed to filter the scanner output (grep exit ${grep_rc})" >&2
  exit 2
fi
case "${rc}" in
  0) ;;
  1)
    echo "secret-scan-ci: findings reported; details are withheld on the public log. Re-run locally: make secret-scan" >&2
    exit 1
    ;;
  *)
    echo "secret-scan-ci: scan could not be completed (exit ${rc}); details are withheld on the public log. Re-run locally: make secret-scan" >&2
    exit 2
    ;;
esac

# 陰性対照の終了状態: 0 = 期待どおり / 1 = 検出能力の異常 / 2 以上 = 検査不能。
canary_rc=0
"${ROOT_DIR}/scripts/secret-scan-canary.sh" || canary_rc=$?
case "${canary_rc}" in
  0) ;;
  1) exit 1 ;;
  *) exit 2 ;;
esac
