#!/usr/bin/env bash
# MSRV（scripts/msrv.sh が読む版）の toolchain を rustup で入れ、既定にし、後続のステップにも使わせる。
# 使い方: scripts/install-msrv.sh [COMPONENT...]（rustfmt・clippy 等。必要なものを呼び出し側が渡す）。
# 呼び出し元: CI の build-test・lint・it-db-ci ジョブ（.github/workflows/ci.yml。Make のターゲットは無い）。
# CI を MSRV で流すのは利用者の指示（2026-11-20）。版は D-2026-11-20-12。入れるのは minimal profile と
# 指定の component だけ。rust-version の `X.Y` は最低版の `X.Y.0` を意味するため、2 部分なら `X.Y.0` に補って
# パッチ版まで指定する（`1.100` を渡すと rustup はパッチ版が出た後に新しい版を選ぶ）。`X.Y.Z` はそのまま使う。
# 後続のステップへの引き継ぎ: `rustup default` は RUSTUP_TOOLCHAIN や rust-toolchain.toml に負けるため、
# GitHub Actions では $GITHUB_ENV に RUSTUP_TOOLCHAIN=<版> を書く（環境変数は rust-toolchain.toml より優先される）。
# あわせて、導入の直後に実際に使われる rustc の版が MSRV と一致することを確かめ、違えば 2 で止める。
# 配布元: RUSTUP_DIST_SERVER・RUSTUP_UPDATE_ROOT が空でない値で設定されていれば、何も入れずに 2 で止める
# （公式の配布元だけを使う条件。.claude/rules/github-actions.md のチェックリスト 9 の例外）。
# 終了コード: 誤用・環境の誤り（rustup 等が無い・版を読めない・配布元の設定・版の不一致）は 2。rustup の失敗はその終了コード。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

for v in RUSTUP_DIST_SERVER RUSTUP_UPDATE_ROOT; do
  if [ -n "${!v:-}" ]; then
    echo "error: ${v} must not be set (only the official rustup distribution is allowed)" >&2
    exit 2
  fi
done

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

# 後続のステップ・以降のコマンドに同じ版を使わせる。この環境変数は rust-toolchain.toml・rustup default より優先される。
export RUSTUP_TOOLCHAIN="${msrv}"
# 実際に使われる rustc が MSRV の版であることを確かめる（食い違えば検査を MSRV で流せていない）。
if ! active="$(rustc --version 2> /dev/null)" || [[ "${active}" != "rustc ${msrv} "* ]]; then
  echo "error: the active rustc does not match the MSRV ${msrv}" >&2
  exit 2
fi
if [ -n "${GITHUB_ENV:-}" ]; then
  printf 'RUSTUP_TOOLCHAIN=%s\n' "${msrv}" >> "${GITHUB_ENV}"
fi
log "active toolchain is ${msrv}"
