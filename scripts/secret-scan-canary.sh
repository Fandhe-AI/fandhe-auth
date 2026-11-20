#!/usr/bin/env bash
# 秘密情報の検査（scripts/hooks/secret-scan.sh --tracked）の陰性対照（TASK-6。CI の secret-scan ジョブが
# `make secret-scan` の後に呼ぶ）。検出器が壊れて常に合格する状態（fail-open）を CI で見逃さないため、
# HEAD の tree に実行時に組み立てたダミーを足した一時 index を検出器に走査させ、ちょうど 1 で終わる
# ことを確かめる。「意図的に混入させたダミーで検査が失敗する」ことの証拠をログに残す役割を持つ。
#
# なぜ commit しないか: ダミーのコミットは pre-commit の同じ規則に止められ（--no-verify は禁止）、
# コミットすれば CI を恒久的に赤にする。そのため一時 index・一時 object dir の中だけで足す。
# 元の index・作業ツリー・object store は変えない（object は alternates 経由で読むだけ）。
#
# 前提: HEAD の tree に検出が無いこと（CI では先の `make secret-scan` が保証する）。
# ダミーの値は本ファイル自体が規則に当たらないよう、実行時に連結して組み立てる。値は出力しない（SECR-23）。
#
# 終了コード: 0 = 検出器がダミーにちょうど 1 を返した / 1 = 期待と異なる / 2 = 準備に失敗（fail-closed）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd git
cd "${ROOT_DIR}"

die() {
  echo "scan-canary: $*" >&2
  exit 2
}

if ! real_objects="$(git rev-parse --path-format=absolute --git-path objects)" || [ -z "${real_objects}" ]; then
  die "failed to resolve the object directory"
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/scan-canary.XXXXXX")" || die "mktemp failed"
trap 'rm -rf "${work_dir}"' EXIT
mkdir "${work_dir}/objects" || die "mkdir failed"

# 以降の git は一時 index・一時 object dir だけを書き換える。
export GIT_INDEX_FILE="${work_dir}/index"
export GIT_OBJECT_DIRECTORY="${work_dir}/objects"
export GIT_ALTERNATE_OBJECT_DIRECTORIES="${real_objects}"

git read-tree HEAD || die "failed to read HEAD into the temporary index"

dash='-----'
key_line="${dash}BEGIN EC PRIVATE"" KEY${dash}"
plant() {
  local path="$1" content="$2" blob
  blob="$(printf '%s\n' "${content}" | git hash-object -w --stdin)" || die "hash-object failed"
  [ -n "${blob}" ] || die "hash-object returned nothing"
  git update-index --add --cacheinfo "100644,${blob},${path}" || die "update-index failed"
}
# HEAD に追跡ファイル・ディレクトリとして存在しない名前を選ぶ（同名があると update-index が衝突して失敗する）。
canary_dir=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  candidate="secret-scan-canary-${RANDOM}${RANDOM}"
  existing="$(git ls-files --cached -- "${candidate}")" || die "ls-files failed"
  if [ -z "${existing}" ]; then
    canary_dir="${candidate}"
    break
  fi
done
[ -n "${canary_dir}" ] || die "could not pick a canary path absent from HEAD"
plant "${canary_dir}/dummy.txt" "${key_line}"
plant "${canary_dir}/.env" "canary placeholder"

rc=0
"${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked > "${work_dir}/out" 2>&1 || rc=$?

fail() {
  echo "scan-canary: FAILED: $1" >&2
  # 検出器の生出力は転送しない（公開 CI ログへ走査対象の値が再掲され得る。SECR-23）。
  # 固定の診断文・終了コード・欠けた集計項目だけを出す。
  exit 1
}
[ "${rc}" -eq 1 ] || fail "scanner exit code was ${rc}, expected 1"
for expected in \
  'secret-scan: private-key: 1 line(s)' \
  'secret-scan: env-file: 1 file(s)' \
  'secret-scan: total: 2 finding(s) in 2 file(s)'; do
  grep -qxF -- "${expected}" "${work_dir}/out" || fail "missing expected line: ${expected}"
done
if grep -qF -- "${key_line}" "${work_dir}/out"; then
  fail "scanner output contains the planted value"
fi

echo "scan-canary: scanner exited 1 on the planted dummies as expected (private-key: 1, env-file: 1, total: 2)"
