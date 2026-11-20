#!/usr/bin/env bash
# 秘密情報の検査（scripts/hooks/secret-scan.sh --tracked --summary。CI と同じ引数）の陰性対照（TASK-6。CI の secret-scan ジョブが
# `scripts/secret-scan-ci.sh` 経由で呼ぶ）。検出器が壊れて常に合格する状態（fail-open）を CI で見逃さないため、
# 実行時に組み立てた既知のダミーだけを入れた一時 index（空から開始）を検出器に走査させ、
# 終了コード 1・出力が許可した集計行の集合と完全に一致すること（余計な行は失敗）・ダミーの値が出力に無いことを確かめる。リポジトリの内容（PR・base の検出対象の有無）に結果が依存しない。「意図的に混入させたダミーで検査が失敗する」ことの証拠をログに残す役割を持つ。
#
# なぜ commit しないか: ダミーのコミットは pre-commit の同じ規則に止められ（--no-verify は禁止）、
# コミットすれば CI を恒久的に赤にする。そのため一時 index・一時 object dir の中だけで足す。
# 元の index・作業ツリー・object store は変えない（object は alternates 経由で読むだけ）。
#
# ダミーの値は本ファイル自体が規則に当たらないよう、実行時に連結して組み立てる。値は出力しない（SECR-23）。
#
# 終了コード: 0 = 検出器がダミーにちょうど 1 を返した / 1 = 検出能力の異常（検出器が 0 を返した・
# 集計行が期待と違う・出力にダミーの値が出た）/ 2 = 検査不能（準備の失敗・検出器が 2 等を返した・
# 出力の読み取りエラー。fail-closed）。git などの生の診断は出さない（パスを含み得る）。
set -euo pipefail
# 相対パスの解決（cd）が CDPATH に左右されないようにする（以降の cd・lib.sh の解決より前に外す）。
unset CDPATH
# 想定外の失敗（set -e 相当）は検査不能（2）として止める。
set -E
trap 'echo "scan-canary: internal error; aborting" >&2; exit 2' ERR
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

reject_args "${0##*/}" "$@"

# 単独で起動されても、呼び出し元の GIT_*（GIT_DIR・GIT_WORK_TREE・GIT_CONFIG_* 等）に左右されないよう外し、
# 置換参照（refs/replace/）も無効にする（このあと本スクリプトが自前の一時 index・object dir を設定する）。
if ! env_names="$(compgen -e 2> /dev/null)"; then
  echo "scan-canary: failed to list environment variables" >&2
  exit 2
fi
for _v in ${env_names}; do
  case "${_v}" in
    GIT_*) unset "${_v}" ;;
  esac
done
unset _v env_names
export GIT_NO_REPLACE_OBJECTS=1
# 外部コマンドは起動前に確かめる。欠けていれば検査不能（2）で止める（makefile.md の契約。fail-closed）。
for _c in git mktemp mkdir rm sort cmp grep; do
  require_cmd "${_c}" || exit 2
done
unset _c

die() {
  echo "scan-canary: $*" >&2
  exit 2
}

# 元のリポジトリの hooks（post-index-change 等）を起動しないよう、git の呼び出しはすべてこの関数を通す。
g() {
  git -c core.hooksPath=/dev/null -c core.fsmonitor=false "$@"
}

# 一時領域は cd の前に作り、作った直後に絶対パスへ解決する（相対の TMPDIR が cd で別の場所を指さないように）。
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/scan-canary.XXXXXX" 2> /dev/null)" || die "mktemp failed"
trap 'rm -rf "${work_dir}"' EXIT
# work_dir は解決と検証が成功するまで mktemp の生のパスのままにする（EXIT トラップが作ったディレクトリを確実に消せるように）。
resolved_dir="$(cd "${work_dir}" 2> /dev/null && pwd -P && printf x)" || die "cannot resolve the temporary directory"
resolved_dir="${resolved_dir%x}"
resolved_dir="${resolved_dir%$'\n'}"
case "${resolved_dir}" in
  /*) ;;
  *) die "cannot resolve the temporary directory" ;;
esac
work_dir="${resolved_dir}"
# 検出器の mktemp なども絶対パスの一時領域を使うよう、TMPDIR も解決済みの値に置き換える。
export TMPDIR="${work_dir}"

cd "${ROOT_DIR}" 2> /dev/null || die "cannot enter the repository root"

if ! real_objects="$(g rev-parse --path-format=absolute --git-path objects 2> /dev/null)" || [ -z "${real_objects}" ]; then
  die "failed to resolve the object directory"
fi

mkdir "${work_dir}/objects" 2> /dev/null || die "mkdir failed"

# 以降の git は一時 index・一時 object dir だけを書き換える。
export GIT_INDEX_FILE="${work_dir}/index"
export GIT_OBJECT_DIRECTORY="${work_dir}/objects"
export GIT_ALTERNATE_OBJECT_DIRECTORIES="${real_objects}"

g read-tree --empty 2> /dev/null || die "failed to create the empty temporary index"

dash='-----'
key_line="${dash}BEGIN EC PRIVATE"" KEY${dash}"
plant() {
  local path="$1" content="$2" blob
  blob="$(printf '%s\n' "${content}" | g hash-object -w --stdin 2> /dev/null)" || die "hash-object failed"
  [ -n "${blob}" ] || die "hash-object returned nothing"
  g update-index --add --cacheinfo "100644,${blob},${path}" 2> /dev/null || die "update-index failed"
}
canary_dir="secret-scan-canary"
plant "${canary_dir}/dummy.txt" "${key_line}"
plant "${canary_dir}/.env" "canary placeholder"

rc=0
"${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?

fail() {
  echo "scan-canary: FAILED: $1" >&2
  # 検出器の生出力は転送しない（公開 CI ログへ走査対象の値が再掲され得る。SECR-23）。
  # 固定の診断文・終了コード・欠けた集計項目だけを出す。
  exit 1
}
# 検出器の終了状態: 1 = 検出（期待）/ 0 = 検出できていない（検出能力の異常）/ それ以外（2 等）= 検査不能。
case "${rc}" in
  1) ;;
  0) fail "scanner exit code was 0, expected 1" ;;
  *)
    echo "scan-canary: scanner could not complete (exit ${rc}); cannot judge" >&2
    exit 2
    ;;
esac
# --summary の出力は、許可した集計行の集合と完全に一致すること（ファイル名などの余計な行があれば失敗）。
# 順序には依存しないよう、期待する行・実際の行を整列して比べる。cmp の終了状態は
# 0 = 一致 / 1 = 不一致（検出能力の異常）/ 2 以上 = 読み取りエラー（検査不能）。
# shellcheck disable=SC2016 # バッククォートは展開させない固定の文言
if ! printf '%s\n' \
  'secret-scan: findings in tracked files' \
  'secret-scan: private-key: 1 line(s)' \
  'secret-scan: env-file: 1 file(s)' \
  'secret-scan: total: 2 finding(s) in 2 file(s)' \
  'replace values with a placeholder / env var reference, or unstage the file (`git restore --staged <file>`).' 2> /dev/null | sort > "${work_dir}/expected.sorted" 2> /dev/null; then
  die "failed to prepare the expected summary"
fi
if ! sort "${work_dir}/out" > "${work_dir}/out.sorted" 2> /dev/null; then
  die "failed to sort the scanner output"
fi
exact_rc=0
cmp -s "${work_dir}/expected.sorted" "${work_dir}/out.sorted" || exact_rc=$?
case "${exact_rc}" in
  0) ;;
  1) fail "scanner summary does not exactly match the expected lines" ;;
  *)
    echo "scan-canary: FAILED: could not compare the scanner output (cmp exit ${exact_rc})" >&2
    exit 2
    ;;
esac
# grep の終了状態は 0 = 一致 / 1 = 不一致 / 2 以上 = エラー。1 のときだけ「値は出ていない」とする。
grep_rc=0
grep -qF -- "${key_line}" "${work_dir}/out" 2> /dev/null || grep_rc=$?
case "${grep_rc}" in
  1) ;;
  0) fail "scanner output contains the planted value" ;;
  *)
    echo "scan-canary: FAILED: could not check the scanner output (grep exit ${grep_rc})" >&2
    exit 2
    ;;
esac

echo "scan-canary: scanner exited 1 on the planted dummies as expected (private-key: 1, env-file: 1, total: 2)"
