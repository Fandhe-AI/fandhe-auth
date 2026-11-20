#!/usr/bin/env bash
# openssl-sys の `vendored` feature が依存解決に 0 件であることを `cargo tree -e features` で検査する（TASK-667 (1/4)・DPOL-13）。
# D-2026-10-17-34 の (1): `vendored` は採らない。版は固定できるが、脆弱性の修正が OS の更新で直らず再ビルドが要るため。
# 検査を外す引数・環境変数は作らない（DPOL-30〜32）。呼び出し元: scripts/lint.sh・`make no-vendored`。
#
# 検出の規則（どれか 1 件でも違反。安全側）:
#   R1: `openssl-sys feature "vendored"`（DPOL-13 の本体）
#   R2: `openssl feature "vendored"`（openssl-sys/vendored を有効にする上流の経路）
#   R3: package `openssl-src`（vendored の build 依存が木に入った痕跡）
# 走査 A は本番の feature 構成（既定の feature + 環境変数 PROD_FEATURES）で、試験専用メンバーを除いた本番対象のメンバーだけを起点にし、合否に入る。
# PROD_FEATURES は dependency-policy.md の手順と同じ `<crate>/<feature>` のカンマ区切り（無ければ空）。本番で有効にする非既定 feature を足す変更では、
# 同じ変更で PROD_FEATURES の既定（CI・make の呼び出し側）も更新する。PROD_FEATURES は検査を足すだけで、外す効果は無い。形式が違う値は検査不能（2）。
# 試験専用メンバー（TEST_ONLY_PKGS）は自身の normal・build 依存が残るため、含めると試験専用 crate だけの vendored を本番の違反として
# 落としてしまう。そのため合否の起点から外し、含めた走査 C は参考の警告だけにする（合否に入れない）。
# メンバーの分類は TEST_ONLY_PKGS と PROD_PKGS の 2 表で持ち、workspace のメンバーがどちらにも無い、または両方にあれば検査不能（2）で止める
# （分類の漏れで本番のメンバーが合否から外れる道を塞ぐ）。新しいメンバーはどちらかに足す。本番の crate が試験専用 crate に依存しても、
# その依存の木は走査 A に含まれる。走査 B は --all-features の上位集合で、参考の警告だけ出し合否に入れない
# （optional の feature の後ろに隠した vendored の早期発見用）。dependency-policy.md は本番の依存の監査に --all-features を使わないと
# 定めるため、試験専用 feature だけの経路で本番の違反として落とさない。
# B は依存の列挙にだけ使い、ビルド・clippy には使わない（DPOL-31。check-crate-deps.sh と同じ扱い）。
# 本番の範囲は normal・build（dev は見ない）。`cargo tree -i` は一致が無いと非 0 のため使わない。
# 実行のたびに自己試験（陰性対照 F1〜F9）を先に流し、検出器が常に 0 を返す状態を防ぐ。
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
# 使う外部コマンドは作業ディレクトリの作成前にすべて確かめる（EXIT トラップの rm の不足で 127 にしない）。
for c in cargo mktemp mkdir rm grep dirname basename; do
  require_cmd "${c}" || exit 2
done

# 生の診断（絶対パス・ホームのパスを含み得る mkdir・mktemp・cd・cargo・redirect の標準エラー）は出さない。
# 失敗した箇所は固定の英語の文で報告して終了コード 2 にする。出力の抑制は合否を変えず、終了コードで判定する。
if ! cd "${ROOT_DIR}" 2>/dev/null; then
  log "error: cannot enter repository root; cannot inspect"
  exit 2
fi

# 偽 crate は repo の target/ の下に作る。誰でも書ける /tmp の下だと、親の .cargo/config.toml
# （build.rustc-wrapper 等）を別の利用者に置かれて任意のコードが実行され得るため。
# 準備の失敗は検査不能（2）。set -e の 1 では「違反」と区別できないため明示する。
if ! mkdir -p "${ROOT_DIR}/target" 2>/dev/null; then
  log "error: cannot create target/; cannot inspect"
  exit 2
fi
if ! WORK="$(mktemp -d "${ROOT_DIR}/target/openssl-vendored.XXXXXX" 2>/dev/null)"; then
  log "error: cannot create work dir; cannot inspect"
  exit 2
fi
cleanup() { rm -rf "${WORK}" 2>/dev/null || true; }
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

# 本番対象のメンバー（段階 1・段階 2 の枠。db-tests は契約の試験の crate だが本番側に置き、合否に入れる＝安全側）。
PROD_PKGS=(
  fandhe-auth-core
  fandhe-auth-secret
  fandhe-auth-db-tests
  fandhe-auth-client
  fandhe-auth-plugin-oidc
  fandhe-auth-backend
)

# build_feature_args <PROD_FEATURES 相当の文字列>: FA に `--features` の引数を作る。空なら空配列。
# 形式は `<crate>/<feature>` のカンマ区切り（dependency-policy.md）。違えば検査不能（2）。
FA=()
build_feature_args() {
  FA=()
  [ -z "$1" ] && return 0
  # 文字列全体を bash の正規表現で照合する（grep は行ごとのため、改行入りの値の 1 行目だけが通り得る）。
  # 区切り（カンマ）・空要素・`/` の有無・空白と制御文字（改行・CR・タブ等）は拒否する。名前の文字の妥当性（`+` 等）と
  # 未知の feature は Cargo に任せる（未知の feature は cargo tree の失敗として run_tree が 2 にする）。
  local c='[^[:space:][:cntrl:],/]'
  local re="^${c}+/${c}+(,${c}+/${c}+)*\$"
  if ! [[ "$1" =~ $re ]]; then
    log "error: PROD_FEATURES must be comma-separated <crate>/<feature>; cannot inspect"
    exit 2
  fi
  FA=(--features "$1")
}

# prod_tree <dir> <locked|offline> <features>: 走査 A の木（本番メンバーだけを起点。features は PROD_FEATURES 形式）。
prod_tree() {
  build_feature_args "$3"
  run_tree "$1" "$2" --workspace "${EXCLUDE_ARGS[@]}" ${FA[@]+"${FA[@]}"}
}

# check_member_classification: workspace のメンバーが PROD_PKGS・TEST_ONLY_PKGS のどちらか 1 つだけにあることを確かめる。
check_member_classification() {
  local out line name rest p known
  out="$(run_tree "${ROOT_DIR}" locked --workspace --depth 0)" || exit 2
  for p in "${PROD_PKGS[@]}"; do
    for known in "${TEST_ONLY_PKGS[@]}"; do
      if [ "${p}" = "${known}" ]; then
        log "error: member classified as both production and test-only: ${p}; cannot inspect"
        exit 2
      fi
    done
  done
  while IFS= read -r line; do
    name="${line%% *}"
    rest="${line#* }"
    case "${rest}" in v[0-9]*) ;; *) continue ;; esac
    known=0
    for p in "${PROD_PKGS[@]}" "${TEST_ONLY_PKGS[@]}"; do
      [ "${p}" = "${name}" ] && known=1
    done
    if [ "${known}" -eq 0 ]; then
      log "error: workspace member not classified (add to PROD_PKGS or TEST_ONLY_PKGS): ${name}; cannot inspect"
      exit 2
    fi
  done <<<"${out}"
}

# run_tree <dir> <locked|offline> [追加引数...]: cargo tree の出力を stdout に返す。失敗は 2（生の出力は出さない）。
# 失敗の原因は分類名だけ出す（絶対パス・認証情報を含み得る生の stderr は出さない）。
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

# count_rule <R1|R2|R3> <tree_output>: 規則に当たる行数を返す。(*) 等の付記は後ろの空白から許す。
count_rule() {
  local rule="$1" tree="$2" line n=0
  while IFS= read -r line; do
    case "${rule}" in
      R1) case "${line}" in 'openssl-sys feature "vendored"' | 'openssl-sys feature "vendored" '*) n=$((n + 1)) ;; esac ;;
      R2) case "${line}" in 'openssl feature "vendored"' | 'openssl feature "vendored" '*) n=$((n + 1)) ;; esac ;;
      R3) case "${line}" in 'openssl-src v'*) n=$((n + 1)) ;; esac ;;
    esac
  done <<<"${tree}" || return 2
  echo "${n}"
}

# mkpkg <dir> <name> <Cargo.toml の追加本文>: path 依存の偽 crate を作る。[workspace] は repo の workspace に取り込まれないため。
# 書き込みの失敗（ENOSPC 等）は検査不能（2）。set -e の 1 では「違反」と区別できないため明示する。
mkpkg() {
  if ! mkdir -p "$1/src" 2>/dev/null ||
    ! printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n%s\n[workspace]\nresolver = "3"\n' "$2" "$3" 2>/dev/null >"$1/Cargo.toml" ||
    ! : 2>/dev/null >"$1/src/lib.rs"; then
    log "error: cannot write fixture ($2); cannot inspect"
    exit 2
  fi
}

# expect_count <label> <rule> <tree> <eq0|ge1>: 自己試験の期待と違えば 1 で止める。
expect_count() {
  local n
  n="$(count_rule "$2" "$3")" || exit 2
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
  if ! mkdir -p "${f}" 2>/dev/null; then
    log "error: cannot create selftest dir; cannot inspect"
    exit 2
  fi
  mkpkg "${f}/openssl-src" "openssl-src" ""
  mkpkg "${f}/openssl-sys" "openssl-sys" $'[features]\nvendored = ["dep:openssl-src"]\n[build-dependencies]\nopenssl-src = { path = "../openssl-src", optional = true }'
  mkpkg "${f}/openssl" "openssl" $'[features]\nvendored = ["openssl-sys/vendored"]\n[dependencies]\nopenssl-sys = { path = "../openssl-sys" }'

  # F1: openssl-sys の vendored を直接有効にする → R1
  mkpkg "${f}/f1" "f1" $'[dependencies]\nopenssl-sys = { path = "../openssl-sys", features = ["vendored"] }'
  t="$(run_tree "${f}/f1" offline)" || exit 2
  expect_count F1 R1 "${t}" ge1

  # F2: openssl 経由の vendored → R1・R2・R3
  mkpkg "${f}/f2" "f2" $'[dependencies]\nopenssl = { path = "../openssl", features = ["vendored"] }'
  t="$(run_tree "${f}/f2" offline)" || exit 2
  for r in R1 R2 R3; do expect_count F2 "${r}" "${t}" ge1; done

  # F3: vendored なしの openssl → 0 件
  mkpkg "${f}/f3" "f3" $'[dependencies]\nopenssl = { path = "../openssl" }'
  t="$(run_tree "${f}/f3" offline)" || exit 2
  for r in R1 R2 R3; do expect_count F3 "${r}" "${t}" eq0; done

  # F4: 依存なし → 0 件
  mkpkg "${f}/f4" "f4" ""
  t="$(run_tree "${f}/f4" offline)" || exit 2
  for r in R1 R2 R3; do expect_count F4 "${r}" "${t}" eq0; done

  # F5: 既定外の feature の後ろに vendored → 走査 A で 0 件、走査 B で検出。cargo tree は feature 行を出さず openssl-src の package だけを出すため R3 で拾う
  mkpkg "${f}/f5" "f5" $'[features]\nx = ["openssl-sys/vendored"]\n[dependencies]\nopenssl-sys = { path = "../openssl-sys" }'
  a="$(run_tree "${f}/f5" offline)" || exit 2
  b="$(run_tree "${f}/f5" offline --all-features)" || exit 2
  for r in R1 R2 R3; do expect_count F5A "${r}" "${a}" eq0; done
  expect_count F5B R3 "${b}" ge1

  # F7: 同じ openssl-sys を normal 依存と dev 依存で共有し、dev 側だけが vendored を要求 → 走査 A（dev を見ない）で 0 件。
  # 辺指定に dev が混入すると本番の依存を誤って違反にするため、その回帰を防ぐ（DPOL-13・resolver は本番と同じ 3）。
  mkpkg "${f}/f7" "f7" $'[dependencies]\nopenssl-sys = { path = "../openssl-sys" }\n[dev-dependencies]\nopenssl-sys = { path = "../openssl-sys", features = ["vendored"] }'
  a="$(run_tree "${f}/f7" offline)" || exit 2
  for r in R1 R2 R3; do expect_count F7A "${r}" "${a}" eq0; done

  # F6: 本番メンバーと試験専用メンバーを含む workspace → 走査 A（--exclude）で 0 件、全メンバーの走査 C で検出
  local w="${f}/f6" tp="${TEST_ONLY_PKGS[0]}"
  if ! mkdir -p "${w}/prod/src" "${w}/${tp}/src" 2>/dev/null ||
    ! printf '[workspace]\nresolver = "3"\nmembers = ["prod", "%s"]\n' "${tp}" 2>/dev/null >"${w}/Cargo.toml" ||
    ! printf '[package]\nname = "prod"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nopenssl-sys = { path = "../../openssl-sys" }\n' 2>/dev/null >"${w}/prod/Cargo.toml" ||
    ! printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nopenssl-sys = { path = "../../openssl-sys", features = ["vendored"] }\n' "${tp}" 2>/dev/null >"${w}/${tp}/Cargo.toml" ||
    ! : 2>/dev/null >"${w}/prod/src/lib.rs" || ! : 2>/dev/null >"${w}/${tp}/src/lib.rs"; then
    log "error: cannot write fixture (f6); cannot inspect"
    exit 2
  fi
  a="$(prod_tree "${w}" offline "")" || exit 2
  b="$(run_tree "${w}" offline --workspace)" || exit 2
  for r in R1 R2 R3; do expect_count F6A "${r}" "${a}" eq0; done
  expect_count F6C R1 "${b}" ge1

  # F8: 本番メンバーの非既定 feature が openssl-sys/vendored を引く → PROD_FEATURES なしは 0 件、PROD_FEATURES ありは走査 A で検出
  local w8="${f}/f8"
  if ! mkdir -p "${w8}/prod8/src" 2>/dev/null ||
    ! printf '[workspace]\nresolver = "3"\nmembers = ["prod8"]\n' 2>/dev/null >"${w8}/Cargo.toml" ||
    ! printf '[package]\nname = "prod8"\nversion = "0.1.0"\nedition = "2021"\n[features]\nx = ["openssl-sys/vendored"]\n[dependencies]\nopenssl-sys = { path = "../../openssl-sys" }\n' 2>/dev/null >"${w8}/prod8/Cargo.toml" ||
    ! : 2>/dev/null >"${w8}/prod8/src/lib.rs"; then
    log "error: cannot write fixture (f8); cannot inspect"
    exit 2
  fi
  a="$(prod_tree "${w8}" offline "")" || exit 2
  for r in R1 R2 R3; do expect_count F8A "${r}" "${a}" eq0; done
  a="$(prod_tree "${w8}" offline "prod8/x")" || exit 2
  expect_count F8B R3 "${a}" ge1

  # F9: PROD_FEATURES の形式検査。改行・CR・タブ・空白・末尾のカンマ入りの値は検査不能（2）で止まる（1 行目だけが通る穴を塞ぐ）
  local bad rc
  for bad in $'prod8/x\nevil' $'prod8/x\r' $'prod8/x\t' $'prod8/x\n' 'prod8/x,' 'prod8 /x' 'prod8'; do
    rc=0
    (build_feature_args "${bad}") 2>/dev/null || rc=$?
    if [ "${rc}" -ne 2 ]; then
      log "FAIL selftest F9: malformed PROD_FEATURES accepted"
      exit 1
    fi
  done
  rc=0
  (build_feature_args "prod8/x,prod8/y,prod8/x+y") 2>/dev/null || rc=$?
  if [ "${rc}" -ne 0 ]; then
    log "FAIL selftest F9: valid PROD_FEATURES rejected"
    exit 1
  fi

  log "selftest ok (F1-F9)"
}

selftest

# 走査 A（合否）。検査不能（2）と違反（1）はここで確定し、参考の走査 B より先に返す。
total=0
check_member_classification
tree="$(prod_tree "${ROOT_DIR}" locked "${PROD_FEATURES:-}")" || exit 2
for r in R1 R2 R3; do
  n="$(count_rule "${r}" "${tree}")" || exit 2
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
      if ! n="$(count_rule "${r}" "${t}")"; then
        log "warn: scan ${label} (info, not gated) could not be counted; result not affected"
        return 0
      fi
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
