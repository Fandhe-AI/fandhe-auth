#!/usr/bin/env bash
# MSRV（scripts/msrv.sh が Cargo.toml から読む版）の toolchain を rustup で入れ、既定にする。
# 使い方: scripts/install-msrv.sh [COMPONENT...]（rustfmt・clippy 等。必要なものを呼び出し側が渡す）。
# 呼び出し元: CI の build-test・lint・it-db-ci ジョブ（.github/workflows/ci.yml。Make のターゲットは無い）。
# CI を MSRV で流すのは利用者の指示（2026-11-20）。版は D-2026-11-20-12。入れるのは minimal profile と
# 指定の component だけ。失敗は終了コード 2（誤用・環境の誤り）か rustup の終了コードで返す。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

require_cmd rustup

msrv="$("${DIR}/msrv.sh")"
args=(toolchain install "${msrv}" --profile minimal)
if [ "$#" -gt 0 ]; then
  components=""
  for c in "$@"; do
    case "${c}" in
      '' | *[!a-z0-9-]*)
        echo "usage: $(basename "$0") [COMPONENT...] (component names are [a-z0-9-]+)" >&2
        exit 2
        ;;
    esac
    components="${components:+${components},}${c}"
  done
  args+=(--component "${components}")
fi

log "rustup ${args[*]}"
rustup "${args[@]}"
rustup default "${msrv}"
