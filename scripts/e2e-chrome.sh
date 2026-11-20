#!/usr/bin/env bash
# ハーネス（crates/e2e-harness）の Chrome の仮想 authenticator の E2E（#[ignore]）を流す（TASK-11 (2/6)）。手で呼ぶ。
# 呼び出し元: `make e2e-chrome`。CI では流さない（runner のブラウザは前提にしない。TASK-6）。
# chromedriver と Chrome が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。結果は〔仮想〕で実機・Safari に数えない（ACPT-7）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd chromedriver
cd "${ROOT_DIR}"

# chrome_registration.rs は unix 限定（cfg(unix)）。uname はホストの判定で Cargo の対象と一致しないため、実行した試験の件数で確かめる。
# 登録試験が 0 件のまま成功しない（DPOL-30〜32）。
out="$(mktemp)"
trap 'rm -f "${out}"' EXIT
cargo test -p fandhe-auth-e2e-harness --locked --test chrome_registration -- --ignored 2>&1 | tee "${out}"
if ! grep -Eq 'test result: ok\. [1-9][0-9]* passed' "${out}"; then
  echo "e2e-chrome: no registration test ran (non-unix target or runner?); failing instead of passing with 0 tests" >&2
  exit 1
fi
