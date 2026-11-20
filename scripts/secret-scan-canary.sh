#!/usr/bin/env bash
# 秘密情報の検査（scripts/hooks/secret-scan.sh --tracked --summary。CI と同じ引数）の陰性対照（TASK-6。2 段目で CI の secret-scan ジョブが
# `scripts/secret-scan-ci.sh` 経由で呼ぶ）。検出器が壊れて常に合格する状態（fail-open）を CI で見逃さないため、
# 実行時に組み立てた既知のダミーだけを入れた一時 index（空から開始）を検出器に走査させ、
# 終了コード 1・合計 2 件になることを確かめる。リポジトリの内容（PR・base の検出対象の有無）に結果が依存しない。「意図的に混入させたダミーで検査が失敗する」ことの証拠をログに残す役割を持つ。
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
# 想定外の失敗（set -e 相当）は検査不能（2）として止める。
set -E
trap 'echo "scan-canary: internal error; aborting" >&2; exit 2' ERR
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd git

die() {
  echo "scan-canary: $*" >&2
  exit 2
}

cd "${ROOT_DIR}" 2> /dev/null || die "cannot enter the repository root"

if ! real_objects="$(git rev-parse --path-format=absolute --git-path objects 2> /dev/null)" || [ -z "${real_objects}" ]; then
  die "failed to resolve the object directory"
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/scan-canary.XXXXXX" 2> /dev/null)" || die "mktemp failed"
trap 'rm -rf "${work_dir}"' EXIT
mkdir "${work_dir}/objects" 2> /dev/null || die "mkdir failed"

# 以降の git は一時 index・一時 object dir だけを書き換える。
export GIT_INDEX_FILE="${work_dir}/index"
export GIT_OBJECT_DIRECTORY="${work_dir}/objects"
export GIT_ALTERNATE_OBJECT_DIRECTORIES="${real_objects}"

git read-tree --empty 2> /dev/null || die "failed to create the empty temporary index"

dash='-----'
key_line="${dash}BEGIN EC PRIVATE"" KEY${dash}"
plant() {
  local path="$1" content="$2" blob
  blob="$(printf '%s\n' "${content}" | git hash-object -w --stdin 2> /dev/null)" || die "hash-object failed"
  [ -n "${blob}" ] || die "hash-object returned nothing"
  git update-index --add --cacheinfo "100644,${blob},${path}" 2> /dev/null || die "update-index failed"
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
for expected in \
  'secret-scan: private-key: 1 line(s)' \
  'secret-scan: env-file: 1 file(s)' \
  'secret-scan: total: 2 finding(s) in 2 file(s)'; do
  # grep の終了状態は 0 = 一致 / 1 = 不一致（検出能力の異常）/ 2 以上 = 読み取りエラー（検査不能）。
  expected_rc=0
  grep -qxF -- "${expected}" "${work_dir}/out" 2> /dev/null || expected_rc=$?
  case "${expected_rc}" in
    0) ;;
    1) fail "missing expected line: ${expected}" ;;
    *)
      echo "scan-canary: FAILED: could not check the scanner output (grep exit ${expected_rc})" >&2
      exit 2
      ;;
  esac
done
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
