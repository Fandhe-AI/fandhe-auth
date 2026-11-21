#!/usr/bin/env bash
# scripts/*.sh が共有するヘルパー。直接実行せず source して使う:
#   DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   source "${DIR}/lib.sh"
#
# 呼び出し元は `set -euo pipefail` を引き継ぐため、未定義変数・失敗した
# コマンド・パイプライン途中の失敗はその場でスクリプトを中断し、終了コードが
# そのまま呼び出し元（人・`make`・lefthook）へ伝播する。
set -euo pipefail

# 本ファイルの親ディレクトリ（= リポジトリルート）を cwd に依存せず解決する。
# `scripts/check.sh` を直接叩いても `make check` 経由でも同じ対象を扱うための前提。
# 末尾の改行で終わるパスも保つため、`$(...)` が末尾の改行を落とす点を番兵（x）で避ける。
# 出力は改行を足さないパスそのもので、受け取る側も番兵を付けて受けてから外す（下の ROOT_DIR）。
repo_root() {
  local lib_dir root src src_dir
  # 組み込みだけでパスを解決する（依存の確認より前に外部コマンド dirname を呼ばない）。
  src="${BASH_SOURCE[0]}"
  case "${src}" in
    */*) src_dir="${src%/*}" ;;
    *) src_dir="." ;;
  esac
  [ -n "${src_dir}" ] || src_dir="/"
  lib_dir="$(cd -- "${src_dir}" && pwd)" || return 1
  root="$(cd "${lib_dir}/.." && pwd && printf x)" || return 1
  root="${root%x}"
  root="${root%$'\n'}"
  [ -n "${root}" ] || return 1
  printf '%s' "${root}"
}

# 解決の失敗・空の結果は、空の ROOT_DIR のまま先へ進めず終了コード 2（検査不能・環境の誤り）で止める。
# `&&` の左では errexit が効かないため、代入の終了状態を明示して確かめる。
if ! ROOT_DIR="$(repo_root && printf x)"; then
  echo "error: cannot resolve the repository root" >&2
  exit 2
fi
ROOT_DIR="${ROOT_DIR%x}"
if [ -z "${ROOT_DIR}" ]; then
  echo "error: cannot resolve the repository root" >&2
  exit 2
fi
# shellcheck disable=SC2034 # source した側のスクリプトで参照する
readonly ROOT_DIR

# require_cmd <name>: PATH 上に無ければ明確なメッセージで失敗する。
# 導入は一切試みない（導入手順の案内は doctor.sh / README の役割）。
require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "error: required command not found on PATH: ${cmd} (run \`make doctor\` for details)" >&2
    return 1
  fi
}

# reject_args <script-name> "$@": 引数を取らないスクリプトで未知の入力を
# 黙って無視せず、usage を出して終了コード 2（誤用）で止める。
reject_args() {
  local script_name="$1"
  shift
  if [ "$#" -ne 0 ]; then
    echo "usage: ${script_name} (no arguments)" >&2
    exit 2
  fi
}

log() {
  echo "[$(basename "$0")] $*" >&2
}

# run_e2e <script-name> <driver-cmd> <test-target>...: ドライバ付きの E2E（#[ignore]）の試験を target ごとに流す共通処理。
# 呼び出し元: scripts/e2e-chrome.sh・scripts/e2e-firefox.sh（コマンド契約の一元化。makefile.md）。
# 試験の対象は cfg(unix) 限定のため、uname ではなく実行した試験の件数で確かめる。
# どの target も 0 件のまま成功しない（DPOL-30〜32）。`--test-threads=1` はブラウザを同時に複数起動しないための安全側の選択、
# `--show-output` は成功した試験の実行の記録の本文（format: で始まる行。5/6）も実行のログに出すため。
run_e2e() {
  local script_name="$1" driver="$2" target out
  shift 2
  [ "$#" -ge 1 ] || {
    echo "usage: run_e2e <script-name> <driver-cmd> <test-target>..." >&2
    return 2
  }
  require_cmd cargo
  require_cmd "${driver}"
  require_cmd mktemp
  require_cmd tee
  require_cmd grep
  cd "${ROOT_DIR}"
  out="$(mktemp)"
  # trap 本文にパスを埋め込まない（TMPDIR 由来の文字がシェルコードとして解釈されるのを避ける）。
  # 固定の本文から、スクリプト全体で生きるグローバル変数を引用付きで渡す。
  E2E_REG_OUT="${out}"
  trap 'rm -f -- "${E2E_REG_OUT:-}"' EXIT
  for target in "$@"; do
    : >"${out}"
    cargo test -p fandhe-auth-e2e-harness --locked --test "${target}" -- --ignored --test-threads=1 --show-output 2>&1 | tee "${out}"
    if ! grep -Eq 'test result: ok\. [1-9][0-9]* passed' "${out}"; then
      echo "${script_name}: no test ran in target ${target} (non-unix target or runner?); failing instead of passing with 0 tests" >&2
      return 1
    fi
  done
}
