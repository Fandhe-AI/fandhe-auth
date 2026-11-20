#!/usr/bin/env bash
# MSRV（scripts/msrv.sh が読む版）の toolchain を rustup で入れ、既定にする。
# 使い方: scripts/install-msrv.sh [COMPONENT...]（rustfmt・clippy 等。必要なものを呼び出し側が渡す）。
# 呼び出し元: CI の build-test・lint・it-db-ci ジョブ（.github/workflows/ci.yml。Make のターゲットは無い）。
# CI を MSRV で流すのは利用者の指示（2026-11-20）。版は D-2026-11-20-12。入れるのは minimal profile と
# 指定の component だけ。rust-version の `X.Y` は最低版の `X.Y.0` を意味するため、2 部分なら `X.Y.0` に補って
# パッチ版まで指定する（`1.100` を渡すと rustup はパッチ版が出た後に新しい版を選ぶ）。`X.Y.Z` はそのまま使う。
# 終了コード: 誤用・環境の誤り（rustup 等が無い・版を読めない）は 2。rustup の失敗はその終了コード。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

require_cmd rustup || exit 2

# msrv.sh の失敗（終了コード 2）は代入の失敗として伝え、空の版で先へ進めない。
if ! msrv="$("${DIR}/msrv.sh")"; then
  echo "error: could not determine the MSRV" >&2
  exit 2
fi
if [[ "${msrv}" =~ ^[0-9]+\.[0-9]+$ ]]; then
  msrv="${msrv}.0"
fi
if ! [[ "${msrv}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: unexpected MSRV format" >&2
  exit 2
fi

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
