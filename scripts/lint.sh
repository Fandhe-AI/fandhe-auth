#!/usr/bin/env bash
# Rust の静的検査（ソースは変更しない。整形の適用は `make fmt`）。
# rustfmt の差分検査 → clippy（警告をエラーにする）→ crate 間の依存の向きの検査を順に流す。
# --all-features は使わない（DPOL-31）。CI への搭載は TASK-6。
# 呼び出し元: `make lint`。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo
require_cmd cargo-fmt
require_cmd cargo-clippy

cd "${ROOT_DIR}"

log "cargo fmt --all --check"
cargo fmt --all --check

log "cargo clippy (-D warnings)"
cargo clippy --workspace --all-targets --locked -- -D warnings

"${DIR}/check-crate-deps.sh"
