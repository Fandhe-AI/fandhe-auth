#!/usr/bin/env bash
# 本体の段階 1 の crate の通常の依存に、禁止の crate が 0 件であることを `cargo tree` で検査する（TASK-714 (1/8)・LIB-2・DPOL-1）。
# 本体（core・secret・db-tests・client）は、外部ログイン Plugin・Hub の crate・接続先ごとの SDK・HTTP のサーバー側の枠組み・
# DB ドライバー・非同期ランタイムに依存しない（LIB-2。DPOL-1 を維持する。A-2026-10-30-2）。この検査はその機械的な入れ物で、
# 禁止の一覧（FORBIDDEN）の中身の充足は #5159〜#5161、CI への搭載（scripts/lint.sh への結線）は #5164、手順の文書は #5165 が担う。
# 検査を外す引数・環境変数は作らない（DPOL-30〜32）。呼び出し元: `make lib-deps`（名前は暫定で、人が確定する）。
#
# 走査（合否）: BODY_PKGS の crate ごとに `cargo tree --locked -p <crate> -e features,normal,build --target all --prefix none` を流し、
#   出た package・feature の行の先頭の名前を FORBIDDEN の表と照合する。推移の依存・既定の feature が有効にする optional の依存も入る。
#   dev の依存は見ない。build を含めるのは LIB-2 の `-e normal` より厳しい側にするため（dependency-policy.md）。この範囲は人の確認待ち。
# feature: 既定の feature と環境変数 PROD_FEATURES（`<crate>/<feature>` のカンマ区切り。無ければ空）で足した feature。
#   PROD_FEATURES は検査を足すだけで外す効果は無い。形式が違う値は検査不能（2）。BODY_PKGS 以外の crate の要素は無視する。
# 参考の走査（合否に入れない）: 各 crate の --all-features。当たれば warn を出すだけ（本番の監査に --all-features を使わない。DPOL-31）。
#   全 feature を合否に入れるかは人が決める（未定）。
# 禁止の表: 1 行 1 件の `<区分>|exact|<crate 名>` か `<区分>|prefix|<接頭辞>`。正規表現・glob は使わず、前方一致は prefix の行だけに効く。
#   初期値は〔提案〕・暫定（PoC のパラメータは要件の値ではない）。集合を決めるのは人（ACPT-7）。表が空なら検査不能（2）。
# 実行のたびに自己試験（陰性対照 L1〜L11）を先に流し、検出器が常に 0 を返す状態を防ぐ。実の走査は現状どの crate も外部の依存が
# 0 件で空のまま通るため、禁止の混入で失敗することの証拠はこの陰性対照が持つ。
# 出力は crate 名・件数・分類名だけ。cargo tree の生の出力・標準エラーには絶対パスが入り得るため出さない。
# 限界: dev の依存・別 workspace の fuzz・Hub の最終バイナリは対象外。plugin-oidc・backend・試験専用の crate には広げない。
#
# 終了コード: 0 合格 / 1 違反または自己試験の異常 / 2 検査不能（fail-closed）。
set -euo pipefail
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

# 検査の対象（LIB-2 の前提の 4 crate）。workspace に無ければ検査不能（2）。
BODY_PKGS=(
  fandhe-auth-core
  fandhe-auth-secret
  fandhe-auth-db-tests
  fandhe-auth-client
)

# 禁止の表（〔提案〕・暫定）。区分: plugin・hub・sdk・http・db・runtime。区分ごとの見直しは #5159（plugin・hub）・#5160（sdk）・#5161（http・db・runtime）。
FORBIDDEN=(
  'plugin|prefix|fandhe-auth-plugin-'
  'plugin|exact|openidconnect'
  'plugin|exact|samael'
  'hub|exact|fandhe-auth-backend'
  'hub|exact|fandhe-auth-db-pg'
  'hub|exact|fandhe-auth-db-fandhe'
  'hub|exact|fandhe-auth-db-migrate'
  'hub|exact|fandhe-auth-server'
  'hub|exact|fandhe-backend'
  'sdk|exact|aws-config'
  'sdk|prefix|aws-sdk-'
  'sdk|prefix|google-cloud-'
  'sdk|prefix|azure_'
  'http|exact|axum'
  'http|exact|axum-core'
  'http|exact|actix-web'
  'http|exact|actix-http'
  'http|exact|rocket'
  'http|exact|warp'
  'http|exact|poem'
  'http|exact|salvo'
  'http|exact|hyper'
  'http|exact|hyper-util'
  'http|exact|tower-http'
  'db|exact|sqlx'
  'db|prefix|sqlx-'
  'db|exact|tokio-postgres'
  'db|exact|postgres'
  'db|exact|diesel'
  'db|exact|rusqlite'
  'db|exact|libsqlite3-sys'
  'db|exact|sea-orm'
  'db|exact|mysql'
  'db|exact|mysql_async'
  'db|exact|redis'
  'db|exact|mongodb'
  'runtime|exact|tokio'
  'runtime|prefix|tokio-'
  'runtime|exact|async-std'
  'runtime|exact|smol'
  'runtime|exact|actix-rt'
)

# require_table: 表が空なら検査不能（2）。空の表のまま常に合格になる状態を防ぐ。
require_table() {
  if [ "${#FORBIDDEN[@]}" -eq 0 ]; then
    log "error: forbidden table is empty; cannot inspect"
    exit 2
  fi
}

# match_forbidden <crate 名>: 表に当たれば区分を出して 0、当たらなければ 1。完全一致は exact の行だけ、前方一致は prefix の行だけ。
match_forbidden() {
  local ent cat kind val
  for ent in ${FORBIDDEN[@]+"${FORBIDDEN[@]}"}; do
    cat="${ent%%|*}"
    kind="${ent#*|}"
    val="${kind#*|}"
    kind="${kind%%|*}"
    case "${kind}" in
      exact) [ "$1" = "${val}" ] && { echo "${cat}"; return 0; } ;;
      prefix) case "$1" in "${val}"*) echo "${cat}"; return 0 ;; esac ;;
    esac
  done
  return 1
}

# find_violations <木の出力>: 禁止に当たる crate を `<名前>|<区分>` で 1 行ずつ出す（重複排除）。
# 行の先頭の空白区切りの語だけを名前として見るため、` v<版>`・` (*)`・` (proc-macro)`・path 依存の ` (<パス>)`・` feature "x"` の付記は照合に影響しない。
find_violations() {
  local line name cat seen=$'\n'
  while IFS= read -r line; do
    name="${line%% *}"
    [ -n "${name}" ] || continue
    cat="$(match_forbidden "${name}")" || continue
    case "${seen}" in *$'\n'"${name}"$'\n'*) continue ;; esac
    seen="${seen}${name}"$'\n'
    echo "${name}|${cat}"
  done <<<"$1"
}

# build_feature_args <PROD_FEATURES 相当> <crate>: FA に、その crate 宛ての `--features` を作る。空なら空配列。
# 形式は `<crate>/<feature>` のカンマ区切り。違えば検査不能（2）。他の crate 宛ての要素は無視する。
FA=()
build_feature_args() {
  FA=()
  [ -z "$1" ] && return 0
  # 文字列全体を bash の正規表現で照合する（grep は行ごとのため、改行入りの値の 1 行目だけが通り得る）。
  local c='[^[:space:][:cntrl:],/]'
  local re="^${c}+/${c}+(,${c}+/${c}+)*\$"
  if ! [[ "$1" =~ $re ]]; then
    log "error: PROD_FEATURES must be comma-separated <crate>/<feature>; cannot inspect"
    exit 2
  fi
  local item feats=""
  local IFS=,
  for item in $1; do
    if [ "${item%%/*}" = "$2" ]; then
      feats="${feats:+${feats},}${item#*/}"
    fi
  done
  if [ -n "${feats}" ]; then FA=(--features "${feats}"); fi
}

# run_tree <dir> <locked|offline> [追加引数...]: cargo tree の出力を stdout に返す。失敗は 2（生の出力は出さない）。
run_tree() {
  local dir="$1" mode="$2" out flag="--offline" err="${WORK}/tree.err" kind="other"
  shift 2
  [ "${mode}" = "locked" ] && flag="--locked"
  if ! out="$({ { cd "${dir}" && cargo tree "${flag}" -e features,normal,build --target all --prefix none "$@"; } 2>"${err}"; } 2>/dev/null)"; then
    if grep -qi 'lock file' "${err}" 2>/dev/null; then
      kind="lockfile"
    elif grep -qiE 'failed to (download|get|fetch)|network|registry|index' "${err}" 2>/dev/null; then
      kind="fetch"
    elif grep -qiE 'failed to (parse|load) manifest|could not find|no matching package' "${err}" 2>/dev/null; then
      kind="manifest"
    fi
    log "error: cargo tree failed (${dir##*/}, cause: ${kind}); cannot inspect"
    exit 2
  fi
  printf '%s\n' "${out}"
}

# pkg_tree <dir> <locked|offline> <crate> <PROD_FEATURES 相当> [追加引数...]: crate の木を返す。
# 起点の crate の行が無い出力（空を含む）は 0 件と読まず検査不能（2）。
pkg_tree() {
  local dir="$1" mode="$2" pkg="$3" t line found=0
  build_feature_args "$4" "${pkg}"
  shift 4
  t="$(run_tree "${dir}" "${mode}" -p "${pkg}" ${FA[@]+"${FA[@]}"} "$@")" || exit 2
  while IFS= read -r line; do
    case "${line}" in "${pkg} v"[0-9]*) found=1 ;; esac
  done <<<"${t}"
  if [ "${found}" -eq 0 ]; then
    log "error: root package line missing in tree (${pkg}); cannot inspect"
    exit 2
  fi
  printf '%s\n' "${t}"
}

cleanup() { rm -rf "${WORK}" 2>/dev/null || true; }

# mkpkg <dir> <name> <Cargo.toml の追加本文>: path 依存の偽 crate を作る。[workspace] は repo の workspace に取り込まれないため。
# 書き込みの失敗は検査不能（2）。set -e の 1 では「違反」と区別できないため明示する。
mkpkg() {
  if ! mkdir -p "$1/src" 2>/dev/null ||
    ! printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n%s\n[workspace]\nresolver = "3"\n' "$2" "$3" 2>/dev/null >"$1/Cargo.toml" ||
    ! : 2>/dev/null >"$1/src/lib.rs"; then
    log "error: cannot write fixture ($2); cannot inspect"
    exit 2
  fi
}

# expect_viol <label> <木の出力> <eq0|ge1> [期待する名前]: 自己試験の期待と違えば 1 で止める。
expect_viol() {
  local v n
  v="$(find_violations "$2")"
  n=0
  [ -n "${v}" ] && n="$(printf '%s\n' "${v}" | wc -l | tr -d ' ')"
  if [ "$3" = "eq0" ] && [ "${n}" -ne 0 ]; then
    log "FAIL selftest $1: false positive"
    exit 1
  fi
  if [ "$3" = "ge1" ] && [ "${n}" -lt 1 ]; then
    log "FAIL selftest $1: not detected"
    exit 1
  fi
  if [ -n "${4-}" ]; then
    case $'\n'"${v}"$'\n' in *$'\n'"$4|"*) ;; *)
      log "FAIL selftest $1: expected name not reported"
      exit 1
      ;;
    esac
  fi
}

# selftest: 陰性対照 L1〜L11。実の表の中身によらず、fixture 専用の表で検出器が働くことを示す。
selftest() {
  local f="${WORK}/fx" t rc out real=()
  if ! mkdir -p "${f}" 2>/dev/null; then
    log "error: cannot create selftest dir; cannot inspect"
    exit 2
  fi
  real=(${FORBIDDEN[@]+"${FORBIDDEN[@]}"})
  FORBIDDEN=('fx|exact|fx-forbidden' 'fx|prefix|fxhub-')

  mkpkg "${f}/fx-forbidden" "fx-forbidden" ""
  mkpkg "${f}/fxhub-core" "fxhub-core" ""
  mkpkg "${f}/fx-forbidden-ish" "fx-forbidden-ish" ""
  mkpkg "${f}/xfxhub-a" "xfxhub-a" ""
  mkpkg "${f}/mid" "mid" $'[dependencies]\nfx-forbidden = { path = "../fx-forbidden" }'

  # L1: normal の直接の依存 → 検出
  mkpkg "${f}/l1" "l1" $'[dependencies]\nfx-forbidden = { path = "../fx-forbidden" }'
  t="$(pkg_tree "${f}/l1" offline l1 "")" || exit 2
  expect_viol L1 "${t}" ge1 fx-forbidden

  # L2: 中間の crate を経由した推移の依存 → 検出
  mkpkg "${f}/l2" "l2" $'[dependencies]\nmid = { path = "../mid" }'
  t="$(pkg_tree "${f}/l2" offline l2 "")" || exit 2
  expect_viol L2 "${t}" ge1 fx-forbidden

  # L3: dev-dependencies だけ → 0 件
  mkpkg "${f}/l3" "l3" $'[dev-dependencies]\nfx-forbidden = { path = "../fx-forbidden" }'
  t="$(pkg_tree "${f}/l3" offline l3 "")" || exit 2
  expect_viol L3 "${t}" eq0

  # L4: build-dependencies だけ → 検出（normal,build を見るため）
  mkpkg "${f}/l4" "l4" $'[build-dependencies]\nfx-forbidden = { path = "../fx-forbidden" }'
  t="$(pkg_tree "${f}/l4" offline l4 "")" || exit 2
  expect_viol L4 "${t}" ge1 fx-forbidden

  # L5: 既定の feature が有効にする optional の依存 → 検出
  mkpkg "${f}/l5" "l5" $'[features]\ndefault = ["x"]\nx = ["dep:fx-forbidden"]\n[dependencies]\nfx-forbidden = { path = "../fx-forbidden", optional = true }'
  t="$(pkg_tree "${f}/l5" offline l5 "")" || exit 2
  expect_viol L5 "${t}" ge1 fx-forbidden

  # L6: 既定でない feature の後ろの optional の依存 → PROD_FEATURES なしは 0 件、ありは検出、--all-features の参考の走査で検出。
  # 他の crate 宛ての要素は無視される。
  mkpkg "${f}/l6" "l6" $'[features]\nx = ["dep:fx-forbidden"]\n[dependencies]\nfx-forbidden = { path = "../fx-forbidden", optional = true }'
  t="$(pkg_tree "${f}/l6" offline l6 "")" || exit 2
  expect_viol L6A "${t}" eq0
  t="$(pkg_tree "${f}/l6" offline l6 "other/x")" || exit 2
  expect_viol L6B "${t}" eq0
  t="$(pkg_tree "${f}/l6" offline l6 "l6/x")" || exit 2
  expect_viol L6C "${t}" ge1 fx-forbidden
  t="$(pkg_tree "${f}/l6" offline l6 "" --all-features)" || exit 2
  expect_viol L6D "${t}" ge1 fx-forbidden

  # L7: 接頭辞 fxhub- は fxhub-core を検出し、近い名前（fx-forbidden-ish・xfxhub-a）は完全一致・前方一致のどちらにも当たらない
  mkpkg "${f}/l7a" "l7a" $'[dependencies]\nfxhub-core = { path = "../fxhub-core" }'
  t="$(pkg_tree "${f}/l7a" offline l7a "")" || exit 2
  expect_viol L7A "${t}" ge1 fxhub-core
  mkpkg "${f}/l7b" "l7b" $'[dependencies]\nfx-forbidden-ish = { path = "../fx-forbidden-ish" }\nxfxhub-a = { path = "../xfxhub-a" }'
  t="$(pkg_tree "${f}/l7b" offline l7b "")" || exit 2
  expect_viol L7B "${t}" eq0

  # L8: 付記（版・(*)・(proc-macro)・パス・feature）の付いた行は付記を外して検出する
  expect_viol L8 $'l8 v0.1.0 (/p)\nfx-forbidden v0.1.0 (/p/fx) (*)' ge1 fx-forbidden
  expect_viol L8B $'fx-forbidden v0.1.0 (proc-macro) (/p/fx)' ge1 fx-forbidden
  expect_viol L8C $'fx-forbidden feature "default"' ge1 fx-forbidden

  # L9: 依存なし → 0 件
  mkpkg "${f}/l9" "l9" ""
  t="$(pkg_tree "${f}/l9" offline l9 "")" || exit 2
  expect_viol L9 "${t}" eq0

  # L10: 壊れた manifest → 検査不能（2）
  if ! mkdir -p "${f}/l10/src" 2>/dev/null || ! printf 'not a manifest [[[\n' 2>/dev/null >"${f}/l10/Cargo.toml"; then
    log "error: cannot write fixture (l10); cannot inspect"
    exit 2
  fi
  rc=0
  (pkg_tree "${f}/l10" offline l10 "") >/dev/null 2>&1 || rc=$?
  if [ "${rc}" -ne 2 ]; then
    log "FAIL selftest L10: broken manifest not treated as cannot-inspect"
    exit 1
  fi

  # L11: 空の表 → 検査不能（2）
  rc=0
  (FORBIDDEN=(); require_table) >/dev/null 2>&1 || rc=$?
  if [ "${rc}" -ne 2 ]; then
    log "FAIL selftest L11: empty table not treated as cannot-inspect"
    exit 1
  fi

  # L12: --all-features の参考の走査は warn を stderr に出す（出力を捨てない）。禁止 crate が無ければ warn なし。
  out="$( (cd "${f}/l6" && cargo generate-lockfile --offline) >/dev/null 2>&1; info_all_features "${f}/l6" l6 2>&1 >/dev/null)" || true
  case "${out}" in
    *"all-features (info, not gated): 1"*"warn: forbidden dependency behind a non-default feature (l6)"*) ;;
    *)
      log "FAIL selftest L12: all-features warn not emitted"
      exit 1
      ;;
  esac
  out="$( (cd "${f}/l9" && cargo generate-lockfile --offline) >/dev/null 2>&1; info_all_features "${f}/l9" l9 2>&1 >/dev/null)" || true
  case "${out}" in
    *"all-features (info, not gated): 0"*) ;;
    *)
      log "FAIL selftest L12B: all-features count not emitted"
      exit 1
      ;;
  esac
  case "${out}" in
    *warn:*)
      log "FAIL selftest L12B: unexpected warn"
      exit 1
      ;;
  esac

  FORBIDDEN=(${real[@]+"${real[@]}"})
  log "selftest ok (L1-L12)"
}

# 参考の走査（合否に入れない）: --all-features。cargo の stderr は run_tree が捨てる。log の出力（件数・warn）は捨てない。
info_all_features() {
  local dir="$1" pkg="$2" tree viol n=0
  # run_tree の fail-closed 用の "cannot inspect" 行は本検査の中断を示す文言なので、参考の走査では捨てて下の warn に置き換える。
  if tree="$(run_tree "${dir}" locked -p "${pkg}" --all-features 2>/dev/null)"; then
    viol="$(find_violations "${tree}")"
    [ -n "${viol}" ] && n="$(printf '%s\n' "${viol}" | wc -l | tr -d ' ')"
    log "scan ${pkg} all-features (info, not gated): ${n}"
    if [ "${n}" -ne 0 ]; then
      log "warn: forbidden dependency behind a non-default feature (${pkg}); confirm it is not a production dependency"
    fi
  else
    log "warn: scan ${pkg} all-features (info, not gated) could not run; result not affected"
  fi
  return 0
}

# main: 直接起動したときの本体（引数の拒否・外部コマンドの確認・表の確認・自己試験・走査の順）。
main() {
  local c pkg tree viol total=0 n v
  reject_args "$(basename "$0")" "$@"
  for c in cargo mktemp mkdir rm grep dirname basename wc tr; do
    require_cmd "${c}" || exit 2
  done
  require_table
  if ! cd "${ROOT_DIR}" 2>/dev/null; then
    log "error: cannot enter repository root; cannot inspect"
    exit 2
  fi
  # 偽 crate は repo の target/ の下に作る。誰でも書ける /tmp の下だと、親の .cargo/config.toml
  # （build.rustc-wrapper 等）を別の利用者に置かれて任意のコードが実行され得るため。
  if ! mkdir -p "${ROOT_DIR}/target" 2>/dev/null; then
    log "error: cannot create target/; cannot inspect"
    exit 2
  fi
  if ! WORK="$(mktemp -d "${ROOT_DIR}/target/lib-deps.XXXXXX" 2>/dev/null)"; then
    log "error: cannot create work dir; cannot inspect"
    exit 2
  fi
  trap cleanup EXIT

  selftest

  for pkg in "${BODY_PKGS[@]}"; do
    tree="$(pkg_tree "${ROOT_DIR}" locked "${pkg}" "${PROD_FEATURES:-}")" || exit 2
    viol="$(find_violations "${tree}")"
    n=0
    if [ -n "${viol}" ]; then
      n="$(printf '%s\n' "${viol}" | wc -l | tr -d ' ')"
      while IFS= read -r v; do
        log "violation: ${pkg} -> ${v%%|*} [${v#*|}]"
      done <<<"${viol}"
    fi
    log "scan ${pkg}: ${n}"
    total=$((total + n))
  done
  if [ "${total}" -ne 0 ]; then
    log "FAIL: forbidden dependency in body crates (LIB-2)"
    exit 1
  fi

  # 参考の走査（合否に入れない）: --all-features。失敗しても合否は変えない。
  for pkg in "${BODY_PKGS[@]}"; do
    info_all_features "${ROOT_DIR}" "${pkg}"
  done
  log "ok: no forbidden dependency in body crates (LIB-2)"
}

# source されたときは本体を走らせない。
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
