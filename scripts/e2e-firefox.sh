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
run_e2e_registration "e2e-firefox" geckodriver firefox_registration
