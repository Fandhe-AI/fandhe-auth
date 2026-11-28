#!/usr/bin/env bash
# ハーネス（crates/it-harness）の Docker を使う結合試験（#[ignore]）を流す（TASK-10 (2/6)）。手で呼ぶ。
# 呼び出し元: `make it-db`。CI では流さない。CI の DB ジョブは scripts/it-db-ci.sh（枠の試験だけ。TASK-6 5/10）。
# DB のパスワードは試験プロセスの中で生成し、子プロセスの環境変数にだけ渡す（SECR-6）。ここでは扱わない。
# docker・openssl・cargo が無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
set -euo pipefail
# 相対パスの cd が CDPATH に左右されない（CDPATH=. で cd が移動先を標準出力へ出し DIR が 2 行になる）ように、DIR の解決より前に外す（#6173）。
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd docker
require_cmd openssl
cd "${ROOT_DIR}"

cargo test -p fandhe-auth-it-harness --locked -- --ignored
