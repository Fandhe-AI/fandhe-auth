#!/usr/bin/env bash
# ハーネス（crates/e2e-harness）の Firefox の仮想 authenticator の E2E（#[ignore]）を流す（TASK-11 (3/6)）。手で呼ぶ。
# 呼び出し元: `make e2e-firefox`。CI では流さない（runner のブラウザは前提にしない。TASK-6）。
# geckodriver と Firefox が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。結果は〔仮想〕で実機・Safari に数えない（ACPT-7）。
# 前提: unix（Linux・macOS 等。geckodriver をプロセスグループで起動・停止するため）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd geckodriver
require_cmd mktemp
require_cmd tee
require_cmd grep
cd "${ROOT_DIR}"

# firefox_registration.rs は unix 限定（cfg(unix)）。uname はホストの判定で Cargo の対象と一致しないため、実行した試験の件数で確かめる。
# 登録試験が 0 件のまま成功しない（DPOL-30〜32）。
out="$(mktemp)"
trap 'rm -f "${out}"' EXIT
cargo test -p fandhe-auth-e2e-harness --locked --test firefox_registration -- --ignored 2>&1 | tee "${out}"
if ! grep -Eq 'test result: ok\. [1-9][0-9]* passed' "${out}"; then
  echo "e2e-firefox: no registration test ran (non-unix target or runner?); failing instead of passing with 0 tests" >&2
  exit 1
fi
