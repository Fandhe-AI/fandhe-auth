#!/usr/bin/env bash
# openssl-sys の `vendored` feature が依存解決に 0 件であることを `cargo tree -e features` で検査する（TASK-667 (1/4)・DPOL-13）。
# D-2026-10-17-34 の (1): `vendored` は採らない。版は固定できるが、脆弱性の修正が OS の更新で直らず再ビルドが要るため。
# 検査を外す引数・環境変数は作らない（DPOL-30〜32）。呼び出し元: scripts/lint.sh・`make no-vendored`。
#
# 検出の規則（どれか 1 件でも違反。安全側）:
#   R1: `openssl-sys feature "vendored"`（DPOL-13 の本体）
#   R2: `openssl feature "vendored"`（openssl-sys/vendored を有効にする上流の経路）
#   R3: package `openssl-src`（vendored の build 依存が木に入った痕跡）
# 走査 A は既定の feature（本番の木）で、試験専用メンバーを除いた本番対象のメンバーだけを起点にし、合否に入る。
# 試験専用メンバー（TEST_ONLY_PKGS）は自身の normal・build 依存が残るため、含めると試験専用 crate だけの vendored を本番の違反として
# 落としてしまう。そのため合否の起点から外し、含めた走査 C は参考の警告だけにする（合否に入れない）。
# 新しい試験専用メンバーは TEST_ONLY_PKGS に足す（足し忘れは合否に入る側＝安全側に倒れる）。走査 B は --all-features の上位集合で、参考の警告だけ出し合否に入れない
# （optional の feature の後ろに隠した vendored の早期発見用）。dependency-policy.md は本番の依存の監査に --all-features を使わないと
# 定めるため、試験専用 feature だけの経路で本番の違反として落とさない。本番 feature 構成の明示（PROD_FEATURES）が決まったら、A をそれに置き換える。
# B は依存の列挙にだけ使い、ビルド・clippy には使わない（DPOL-31。check-crate-deps.sh と同じ扱い）。
# 本番の範囲は normal・build（dev は見ない）。`cargo tree -i` は一致が無いと非 0 のため使わない。
# 実行のたびに自己試験（陰性対照 F1〜F5）を先に流し、検出器が常に 0 を返す状態を防ぐ。
# 出力は件数と規則名だけ。cargo tree の生の出力には絶対パスが入るため出さない。
# 限界: dev の依存・別 workspace の crates/trial-deps/fuzz・Hub の最終バイナリは対象外。
# TODO(TASK-667): 動的リンクと版の記録の対応（#318 の 2/4）を足す
#
# 終了コード: 0 合格 / 1 違反または自己試験の異常 / 2 検査不能（fail-closed）。
set -euo pipefail
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
require_cmd cargo || exit 2
require_cmd mktemp || exit 2

cd "${ROOT_DIR}"

# 偽 crate は repo の target/ の下に作る。誰でも書ける /tmp の下だと、親の .cargo/config.toml
# （build.rustc-wrapper 等）を別の利用者に置かれて任意のコードが実行され得るため。
# 準備の失敗は検査不能（2）。set -e の 1 では「違反」と区別できないため明示する。
if ! mkdir -p "${ROOT_DIR}/target"; then
  log "error: cannot create target/; cannot inspect"
  exit 2
fi
if ! WORK="$(mktemp -d "${ROOT_DIR}/target/openssl-vendored.XXXXXX")"; then
  log "error: cannot create work dir; cannot inspect"
  exit 2
fi
cleanup() { rm -rf "${WORK}"; }
trap cleanup EXIT

# 本番対象から外す試験専用メンバー（Cargo.toml の注記・check-crate-deps.sh の試験専用の表と同じ。TASK-667）。
TEST_ONLY_PKGS=(
  fandhe-auth-hub-stub
  fandhe-auth-console-stub
  fandhe-auth-it-harness
  fandhe-auth-e2e-harness
  fandhe-auth-load-harness
  fandhe-auth-trial-deps
)
EXCLUDE_ARGS=()
for p in "${TEST_ONLY_PKGS[@]}"; do EXCLUDE_ARGS+=(--exclude "${p}"); done

# run_tree <dir> <locked|offline> [追加引数...]: cargo tree の出力を stdout に返す。失敗は 2（生の出力は出さない）。
run_tree() {
  local dir="$1" mode="$2" out flag="--offline"
  shift 2
  [ "${mode}" = "locked" ] && flag="--locked"
  if ! out="$(cd "${dir}" && cargo tree "${flag}" -e features,normal,build --target all --prefix none "$@" 2>/dev/null)"; then
    log "error: cargo tree failed (${dir##*/}); cannot inspect"
    exit 2
  fi
  printf '%s\n' "${out}"
}

# count_rule <R1|R2|R3> <tree_output>: 規則に当たる行数を返す。(*) 等の付記は後ろの空白から許す。
count_rule() {
  local rule="$1" tree="$2" line n=0
  while IFS= read -r line; do
    case "${rule}" in
      R1) case "${line}" in 'openssl-sys feature "vendored"' | 'openssl-sys feature "vendored" '*) n=$((n + 1)) ;; esac ;;
      R2) case "${line}" in 'openssl feature "vendored"' | 'openssl feature "vendored" '*) n=$((n + 1)) ;; esac ;;
      R3) case "${line}" in 'openssl-src v'*) n=$((n + 1)) ;; esac ;;
    esac
  done <<<"${tree}"
  echo "${n}"
}

# mkpkg <dir> <name> <Cargo.toml の追加本文>: path 依存の偽 crate を作る。[workspace] は repo の workspace に取り込まれないため。
# 書き込みの失敗（ENOSPC 等）は検査不能（2）。set -e の 1 では「違反」と区別できないため明示する。
mkpkg() {
  if ! mkdir -p "$1/src" ||
    ! printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n%s\n[workspace]\n' "$2" "$3" >"$1/Cargo.toml" ||
    ! : >"$1/src/lib.rs"; then
    log "error: cannot write fixture ($2); cannot inspect"
    exit 2
  fi
}

# expect_count <label> <rule> <tree> <eq0|ge1>: 自己試験の期待と違えば 1 で止める。
expect_count() {
  local n
  n="$(count_rule "$2" "$3")"
  if [ "$4" = "eq0" ] && [ "${n}" -ne 0 ]; then
    log "FAIL selftest $1: false positive ${2}"
    exit 1
  fi
  if [ "$4" = "ge1" ] && [ "${n}" -lt 1 ]; then
    log "FAIL selftest $1: ${2} not detected"
    exit 1
  fi
}

selftest() {
  local f="${WORK}/fx" t a b r
  if ! mkdir -p "${f}"; then
    log "error: cannot create selftest dir; cannot inspect"
    exit 2
  fi
  mkpkg "${f}/openssl-src" "openssl-src" ""
  mkpkg "${f}/openssl-sys" "openssl-sys" $'[features]\nvendored = ["dep:openssl-src"]\n[build-dependencies]\nopenssl-src = { path = "../openssl-src", optional = true }'
  mkpkg "${f}/openssl" "openssl" $'[features]\nvendored = ["openssl-sys/vendored"]\n[dependencies]\nopenssl-sys = { path = "../openssl-sys" }'

  # F1: openssl-sys の vendored を直接有効にする → R1
  mkpkg "${f}/f1" "f1" $'[dependencies]\nopenssl-sys = { path = "../openssl-sys", features = ["vendored"] }'
  t="$(run_tree "${f}/f1" offline)"
  expect_count F1 R1 "${t}" ge1

  # F2: openssl 経由の vendored → R1・R2・R3
  mkpkg "${f}/f2" "f2" $'[dependencies]\nopenssl = { path = "../openssl", features = ["vendored"] }'
  t="$(run_tree "${f}/f2" offline)"
  for r in R1 R2 R3; do expect_count F2 "${r}" "${t}" ge1; done

  # F3: vendored なしの openssl → 0 件
  mkpkg "${f}/f3" "f3" $'[dependencies]\nopenssl = { path = "../openssl" }'
  t="$(run_tree "${f}/f3" offline)"
  for r in R1 R2 R3; do expect_count F3 "${r}" "${t}" eq0; done

  # F4: 依存なし → 0 件
  mkpkg "${f}/f4" "f4" ""
  t="$(run_tree "${f}/f4" offline)"
  for r in R1 R2 R3; do expect_count F4 "${r}" "${t}" eq0; done

  # F5: 既定外の feature の後ろに vendored → 走査 A で 0 件、走査 B で検出。cargo tree は feature 行を出さず openssl-src の package だけを出すため R3 で拾う
  mkpkg "${f}/f5" "f5" $'[features]\nx = ["openssl-sys/vendored"]\n[dependencies]\nopenssl-sys = { path = "../openssl-sys" }'
  a="$(run_tree "${f}/f5" offline)"
  b="$(run_tree "${f}/f5" offline --all-features)"
  for r in R1 R2 R3; do expect_count F5A "${r}" "${a}" eq0; done
  expect_count F5B R3 "${b}" ge1

  log "selftest ok (F1-F5)"
}

selftest

# 走査 A（合否）。検査不能（2）と違反（1）はここで確定し、参考の走査 B より先に返す。
total=0
tree="$(run_tree "${ROOT_DIR}" locked --workspace "${EXCLUDE_ARGS[@]}")"
for r in R1 R2 R3; do
  n="$(count_rule "${r}" "${tree}")"
  log "scan A ${r}: ${n}"
  total=$((total + n))
done
if [ "${total}" -ne 0 ]; then
  log "FAIL: vendored openssl detected in dependency resolution (DPOL-13)"
  exit 1
fi

# scan_info <ラベル> <追加引数...>: 参考の走査。失敗しても終了コードに影響させず警告だけ出す（A の合否を上書きしない）。
scan_info() {
  local label="$1" t r n
  shift
  if t="$(run_tree "${ROOT_DIR}" locked "$@")"; then
    for r in R1 R2 R3; do
      n="$(count_rule "${r}" "${t}")"
      log "scan ${label} (info, not gated) ${r}: ${n}"
      [ "${n}" -ne 0 ] && log "warn: vendored path found in scan ${label} (${r}); confirm it is not a production dependency"
    done
  else
    log "warn: scan ${label} (info, not gated) could not run; result not affected"
  fi
  return 0
}
# 走査 B（参考）: 本番対象のメンバーの --all-features。非既定 feature の後ろの経路。
scan_info B --workspace --all-features "${EXCLUDE_ARGS[@]}"
# 走査 C（参考）: 試験専用メンバーを含む全メンバーの既定 feature。
scan_info C --workspace
log "ok: no vendored openssl (DPOL-13)"
