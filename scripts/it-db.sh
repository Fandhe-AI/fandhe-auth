#!/usr/bin/env bash
# ハーネス（crates/it-harness）の Docker を使う結合試験（#[ignore]）を流す（TASK-10 (2/6)）。手で呼ぶ。
# 呼び出し元: `make it-db`。CI では流さない（runner の DB は前提にしない。TASK-6）。
# DB のパスワードは試験プロセスの中で生成し、子プロセスの環境変数にだけ渡す（SECR-6）。ここでは扱わない。
# docker・openssl・cargo が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd docker
require_cmd openssl
cd "${ROOT_DIR}"

cargo test -p fandhe-auth-it-harness --locked -- --ignored
