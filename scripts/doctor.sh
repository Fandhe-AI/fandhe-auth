#!/usr/bin/env bash
# 開発環境の診断のみを行う。導入・更新・修復は一切しない。
# 必須ツールが欠けていれば非 0 で終了し、次の行動は呼び出し元（人・`make doctor`）が決める。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"

status=0

# check_tool <required|optional> <name> <version-command...>
check_tool() {
  local level="$1" name="$2"
  shift 2
  local out=""
  # rustup の shim は component が無くても PATH 上に在るため、version コマンドの終了コードで判定する。
  # コマンド置換を echo の中に置かず代入に切り出す（bash 3.2 でも失敗を取りこぼさない）。
  if command -v "${name}" >/dev/null 2>&1 && out="$("$@" 2>&1)"; then
    echo "ok   ${name}: $(printf '%s\n' "${out}" | head -n1)"
  elif [ "${level}" = "required" ]; then
    echo "miss ${name}: not found or not runnable (required)"
    status=1
  else
    echo "skip ${name}: not found or not runnable (optional)"
  fi
}

echo "environment diagnosis for ${ROOT_DIR} (read-only, no changes made)"
check_tool required git git --version
check_tool required make make --version
check_tool required lefthook lefthook version
check_tool required editorconfig-checker editorconfig-checker --version
check_tool required shellcheck sh -c 'shellcheck --version | grep "^version:"'
check_tool required cargo cargo --version
check_tool required cargo-fmt cargo fmt --version
check_tool required cargo-clippy cargo clippy --version
check_tool optional direnv direnv version
check_tool optional docker-compose docker compose version
check_tool optional openssl openssl version

if [ -f "${ROOT_DIR}/.env" ]; then
  echo "ok   .env: present (values are not displayed)"
else
  echo "note .env: not found; \`make setup\` copies .env.example to .env"
fi

if [ "${status}" -ne 0 ]; then
  echo "one or more required tools are missing; see README.md \"必要なツール\" for install instructions" >&2
fi

exit "${status}"
