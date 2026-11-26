#!/usr/bin/env bash
# 秘密情報検査の CI 向け入口（TASK-6。CI の secret-scan ジョブから呼ぶ〔#51〕。ローカルからも同じ形で呼べる）。
#   scripts/secret-scan-ci.sh [<走査対象のリポジトリのパス>]   既定は本スクリプトのあるリポジトリ
# 検出器・陰性対照は本スクリプトと同じリポジトリ（CI では PR が書き換えられない版。PR では保護された main の先端）のものを使い、
# 走査対象はデータとして渡す。公開ログには規則 ID・件数・固定の診断文だけを出す。
# 検査不能（2）のときは、lib.sh の許可リストと行全体が完全一致する検出器の診断文も通す（#6036）。
# 走査するのは HEAD を sha に固定して作った専用の一時 index（作業ツリーの index は読まない）。
# 検出器の出力は一度ファイルへ受け、規則 ID・件数の行（--summary の出力形）だけを通す。ファイル名・値・
# git のエラー文は出さない（パスに値が含まれ得る。SECR-23）。
# 終了コード: 0 = 検出なし・陰性対照も期待どおり / 1 = 検出または陰性対照の失敗 / 2 = 検査を完了できない。
# 範囲外: コミット履歴・submodule の中。検出時は .claude/rules/security.md の重大度「高」に従う。
set -euo pipefail
# 相対パスの解決（cd）が CDPATH に左右されないようにする（以降の cd・lib.sh の解決より前に外す）。
unset CDPATH
# 想定外の失敗（set -e 相当）は検出（1）と取り違えないよう、すべて検査不能（2）として止める（fail-closed）。
# lib.sh の読み込み・DIR の解決の失敗も対象にするため、source より前に置く。
set -E
trap 'echo "secret-scan-ci: internal error; aborting" >&2; exit 2' ERR
# 起動時のパス解決は bash の組み込みだけで行う（依存の確認より前に外部コマンド dirname を呼ばない）。
_src="${BASH_SOURCE[0]}"
case "${_src}" in
  */*) _src_dir="${_src%/*}" ;;
  *) _src_dir="." ;;
esac
[ -n "${_src_dir}" ] || _src_dir="/"
DIR="$(cd -- "${_src_dir}" && pwd)"
unset _src _src_dir
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

# 外部コマンドは起動前に確かめる。欠けていれば検査不能（2）で止める（makefile.md の契約。fail-closed）。
for _c in git mktemp grep rm; do
  require_cmd "${_c}" || exit 2
done
unset _c

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
# 相対の TMPDIR でも、git は `-C` や cd で作業ディレクトリが変わる。一時領域は作った直後に絶対パスへ解決してから使う。
# work_dir は解決と検証が成功するまで mktemp の生のパスのままにする（EXIT トラップが作ったディレクトリを確実に消せるように）。
if ! resolved_dir="$(cd "${work_dir}" 2> /dev/null && pwd -P && printf x)"; then
  echo "secret-scan-ci: cannot resolve the temporary directory" >&2
  exit 2
fi
resolved_dir="${resolved_dir%x}"
resolved_dir="${resolved_dir%$'\n'}"
case "${resolved_dir}" in
  /*) ;;
  *)
    echo "secret-scan-ci: cannot resolve the temporary directory" >&2
    exit 2
    ;;
esac
work_dir="${resolved_dir}"
# 検出器の mktemp なども絶対パスの一時領域を使うよう、TMPDIR も解決済みの値に置き換える。
export TMPDIR="${work_dir}"
out="${work_dir}/out"
tmp_index="${work_dir}/index"
if ! GIT_INDEX_FILE="${tmp_index}" g -C "${target}" read-tree "${head_sha}" > /dev/null 2>&1; then
  echo "secret-scan-ci: failed to build the temporary index from HEAD" >&2
  exit 2
fi

# 再走査の案内を出す。案内する検出器・除外一覧は、この実行が使った信頼済みの版（本スクリプトのあるリポジトリの HEAD）に揃える。
# 保護された main の先端ではない（trust-root-secret-scan.yml は組織の ruleset が固定した SHA の版を使うため）。
# secret-scan.sh --tracked はコミットを引数に取らず index を走査するので、走査したコミットから専用の一時 index を作って渡す。
# ただし検出器・除外一覧が作業ツリーでコミットと違うときは、sha では再現できないので、その旨の案内に切り替える。
# 詳細付きの再走査はファイル名・git の生の診断を出すため、非公開の端末で行い出力を公開の場へ出さないよう明記する（SECR-23）。
# 出す値は公開してよい 2 つの sha だけ（秘密値・パスを含まない）。
rescan_hint() {
  local trusted_sha dirty detector_src tail_text private_text
  trusted_sha="$(g -C "${ROOT_DIR}" rev-parse --verify "HEAD^{commit}" 2> /dev/null)" || trusted_sha="unknown"
  # 検出器（scripts/hooks/secret-scan.sh）と除外一覧（skills-lock.json）は作業ツリーのファイルを使う。どちらかが
  # コミットと違う（未コミットの変更・未追跡）か、比較できないときは、sha では再現できないので、版の案内を切り替える（fail-closed）。
  # 限界: 判定は git status --porcelain によるため、skip-worktree・assume-unchanged の変更と、ignore された未追跡のファイルは
  # 見落とす（その場合は sha の案内になり、実際に使った版とずれ得る）。走査そのものと終了コードには影響しない（案内の文だけの違い）。
  # 内容を直接比べる方式は、git の設定・フラグ次第の挙動を増やすため採らない。
  dirty="$(g -C "${ROOT_DIR}" status --porcelain --untracked-files=all -- scripts/hooks/secret-scan.sh skills-lock.json 2> /dev/null && printf x)" || dirty="unknown"
  if [ "${trusted_sha}" = "unknown" ] || [ "${dirty}" != "x" ]; then
    detector_src="use the detector and exclusion list exactly as this run used them: the working-tree copies of scripts/hooks/secret-scan.sh and skills-lock.json in the repository of this entry differ from, or cannot be compared with, the commit (${trusted_sha}), so that commit does not reproduce them; keep copies of both files from that working tree (do not rely on the commit or the main tip)"
    tail_text="run <detector copy>/scripts/hooks/secret-scan.sh --tracked (without --summary) with the same GIT_INDEX_FILE, with skills-lock.json beside it as in the original layout"
  else
    detector_src="check out the trusted detector and exclusion list at the commit this job used (${trusted_sha}), not the main tip"
    tail_text="run <trusted checkout>/scripts/hooks/secret-scan.sh --tracked (without --summary) with the same GIT_INDEX_FILE"
  fi
  private_text="Run this rescan only in a private local terminal: without --summary it prints file names and raw git diagnostics, which can contain secret values (SECR-23); never run it in a public CI job and never paste or upload its output to a public log, PR or issue."
  echo "secret-scan-ci: to see file names and diagnostics, ${detector_src}; then, with the scanned repository as the working directory and every existing GIT_* variable unset for both commands below, as this entry does (e.g. env -u GIT_DIR -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES -u GIT_WORK_TREE -u GIT_INDEX_FILE -u GIT_COMMON_DIR -u GIT_NAMESPACE; list others with env | grep ^GIT_), build a dedicated index from the scanned commit (${head_sha}) with GIT_NO_REPLACE_OBJECTS=1 GIT_INDEX_FILE=<new file> git -c core.hooksPath=/dev/null -c core.fsmonitor=false read-tree ${head_sha}, and ${tail_text}; do not run make or any script from the scanned commit. ${private_text}" >&2
}

# 検出器の終了状態は 0 = 検出なし / 1 = 検出 / 2 = 検査不能。それ以外（起動失敗・シグナル等）も検査不能に畳む。
rc=0
# cd の失敗（終了状態 1 になり「検出」と取り違えられる）は、サブシェルの中で検査不能（2）にする。
(
  # サブシェルは set -E と ERR トラップを受け継ぐ。検出器の終了状態 1（検出）がトラップで 2 に変わらないよう、
  # ここでは外し、終了状態は下の明示の `exit 2` と検出器の終了状態だけで決める（bash の版による差も避ける）。
  trap - ERR
  set +E
  cd "${target}" 2> /dev/null || exit 2
  GIT_INDEX_FILE="${tmp_index}" "${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked --summary
) > "${out}" 2>&1 || rc=$?
# 規則 ID は許可した一覧との完全一致だけを通す（任意の文字列・git の診断を公開ログへ通さない）。
# grep の終了状態は 0 = 一致 / 1 = 該当なし / 2 以上 = エラー。1 だけを「該当なし」として許す。
filtered="${work_dir}/filtered"
grep_rc=0
grep -aE '^secret-scan: (env-file|vendor-token|private-key|jwt|jwk-private|credential-url|assignment): [0-9]+ (line|file)\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) in [0-9]+ file\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) \([0-9]+ tracked file\(s\) checked\)$|^secret-scan: findings in tracked files$' "${out}" > "${filtered}" 2> /dev/null || grep_rc=$?
if [ "${grep_rc}" -gt 1 ]; then
  echo "secret-scan-ci: failed to filter the scanner output (grep exit ${grep_rc})" >&2
  exit 2
fi
# 許可した行だけを公開ログへ出し、走査完了の集計行（検出 0 件）の数と、それ以外の行の数を数える。
clean_re='^secret-scan: total: 0 finding\(s\) \([0-9]+ tracked file\(s\) checked\)$'
clean_lines=0
other_lines=0
while IFS= read -r line || [ -n "${line}" ]; do
  printf '%s\n' "${line}"
  if [[ ${line} =~ ${clean_re} ]]; then
    clean_lines=$((clean_lines + 1))
  else
    other_lines=$((other_lines + 1))
  fi
done < "${filtered}"
case "${rc}" in
  0)
    # 検出器が 0 を返しても、走査完了の集計行（検出 0 件）がちょうど 1 行で、ほかの許可行（検出の行）が無いことを
    # 合格の条件にする。欠けていたり矛盾していたりしたら、走査が完了した証拠が無いため検査不能（2）。
    if [ "${clean_lines}" -ne 1 ] || [ "${other_lines}" -ne 0 ]; then
      echo "secret-scan-ci: the scanner exited 0 without exactly one completion summary line; treating as incomplete" >&2
      exit 2
    fi
    ;;
  1)
    echo "secret-scan-ci: findings reported; details are withheld on the public log." >&2
    rescan_hint
    exit 1
    ;;
  *)
    # 検査不能のときだけ、検出器の固定の診断文を許可リスト（lib.sh。陰性対照と共有）との行全体の完全一致で通す。
    # 値・パス・git の生の診断は通さない（SECR-23）。失敗は無視する（既に検査不能（2）で終える）。
    secret_scan_fixed_diagnostics > "${work_dir}/allowed-diag" 2> /dev/null || true
    grep -aFx -f "${work_dir}/allowed-diag" "${out}" >&2 2> /dev/null || true
    echo "secret-scan-ci: scan could not be completed (exit ${rc}); details are withheld on the public log." >&2
    rescan_hint
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
