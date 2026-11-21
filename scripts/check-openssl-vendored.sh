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
# 実行のたびに自己試験（陰性対照 F1〜F12）を先に流し、検出器が常に 0 を返す状態を防ぐ。
# 出力は件数と規則名だけ。cargo tree の生の出力には絶対パスが入るため出さない。
# 限界: dev の依存・別 workspace の crates/trial-deps/fuzz・Hub の最終バイナリは対象外。
#
# 動的リンクと版の記録の対応（TASK-667 (2/4)・DPOL-13）:
#   R4: 静的リンクを強いる設定が無いこと（合否）。openssl-sys の build script は環境変数 OPENSSL_STATIC と <TARGET>_OPENSSL_STATIC を見て、
#       `0` 以外なら静的にリンクする。pkg-config crate は変数が存在すれば値が `0` でも静的リンクを選ぶため、値によらず違反にする。`vendored` でなくても配備先の OpenSSL へ動的にリンクしない道になるため、検査の環境変数と
#       repo の `.cargo/config.toml`・`.cargo/config` の `[env]` のキー（TOML を構造として読み、引用キーのエスケープを復号した後の名前）の
#       `OPENSSL_STATIC` と `<TARGET>_OPENSSL_STATIC` を値によらず違反にする（安全側）。コメントや他の設定値の中の文字列は数えない。
#       TOML が読めない・不正なら検査不能（2）。名前と値は出さず件数だけ出す。
#       PKG_CONFIG_ALL_STATIC（と <TARGET>_PKG_CONFIG_ALL_STATIC）も存在だけで静的を選ぶため同じく違反にする。同じ .cargo に config と config.toml が
#       共存する場合は両方を検査する（片方が消えると残りが有効になるため。dependency-policy.md の配置禁止に合わせ、優先順位で無視される側も見ない道を作らない）。
#       `[env]` が存在してテーブルでない（`env = []` 等）場合は検査不能（2）。
#   報告: 本番の依存の木の openssl-sys の件数（package ごと。(*) の付記は除いて重複排除）を出す（参考。合否に入れない）。
#       依存の木だけではリンク方式（動的か静的か）を断定できない。1 件以上なら版の記録の要否と最終バイナリのリンク方式の確認は
#       Hub の `AOPS-2` で行う（本体は記録を持たない。D-2026-10-30-1）。0 件なら本体の依存の木から `AOPS-2` に載せるものは無い。
#   限界: 親ディレクトリ・CARGO_HOME の cargo 設定と --config の指定は呼び出し側の環境のため見ない。配備先に静的ライブラリしか無い場合と
#       最終バイナリのリンクの実機での確認（ldd 等）は Hub の担当（`AOPS-2`）。
#
# 終了コード: 0 合格 / 1 違反または自己試験の異常 / 2 検査不能（fail-closed）。
set -euo pipefail
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

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

# count_static_env [前置コマンド...]: 環境変数のうち、名前が次の 4 つの形のものの数を返す（R4）。
#   OPENSSL_STATIC・PKG_CONFIG_ALL_STATIC、<TARGET>_OPENSSL_STATIC・<TARGET>_PKG_CONFIG_ALL_STATIC（末尾一致）
# pkg-config crate は存在だけで静的を選ぶため、値が `0` でも数える。名前も値も出さない。値は判定に使わず、eval やコマンドの引数にも展開しない。
# <TARGET> はハイフンを含み得る（上流の openssl-sys はハイフンを `_` に替えた名前を読むため、ここでは上流の版が変わっても漏らさないよう、
# 安全側に多めに数える）。bash の変数名にならない名前も見落とさないよう、compgen ではなく python の os.environ で列挙する。
# 前置コマンドは試験用（env -i で子の環境を作る）。bash を経ず、解決済みの Python の実体（PY）を直接起動する。
# <TARGET> の検査契約は上流と同じ「大文字にし `-` を `_` に替えた名前」（openssl-sys 0.9.117 の build/main.rs、pkg-config 0.3.34 の lib.rs で確認）。
# ハイフン入りの名前は上流が読まないので契約の外で、見えれば多めに数える分。上流の版を上げたら、読む名前の形が変わっていないか確かめ直す。
# 出力が数字でなければ検査不能（2）。python が 0 で何も出さない場合を 0 件として扱わない（fail-closed）。
count_static_env() {
  local c
  c="$("$@" "${PY}" -I -c '
import os
n = 0
for k in os.environ:
    if k in ("OPENSSL_STATIC", "PKG_CONFIG_ALL_STATIC") or k.endswith(("_OPENSSL_STATIC", "_PKG_CONFIG_ALL_STATIC")):
        n += 1
print(n)
' 2>/dev/null)" || return 2
  case "${c}" in '' | *[!0-9]*) return 2 ;; esac
  echo "${c}"
}

# count_static_config <root>: <root>/.cargo/config.toml・config の `[env]` のキーのうち OPENSSL_STATIC か _OPENSSL_STATIC で終わるものの数を返す
# （R4。値が 0 でも数える＝安全側）。TOML を tomllib で構造として読み、復号後のキー名で判定する。コメント・他の値の文字列は数えない。
# 読めない・TOML として不正なファイルは検査不能（2）。python の診断（パスを含み得る）は出さない。
count_static_config() {
  local f c n=0
  # 優先順位で片方が無視されても、消えると残りが有効になるため、存在する両方のファイルを検査する（安全側）。
  for f in "$1/.cargo/config" "$1/.cargo/config.toml"; do
    # リンク切れのシンボリックリンクは -e が偽になるため -L も見て、読み取り検査（失敗は 2）へ進める。
    [ -e "${f}" ] || [ -L "${f}" ] || continue
    {
    [ -r "${f}" ] || return 2
    c="$(python3 -I -c '
import sys, tomllib
try:
    with open(sys.argv[1], "rb") as fh:
        d = tomllib.load(fh)
except Exception:
    sys.exit(2)
e = d.get("env", {})
if not isinstance(e, dict):
    sys.exit(2)
n = sum(1 for k in e if k in ("OPENSSL_STATIC", "PKG_CONFIG_ALL_STATIC") or k.endswith(("_OPENSSL_STATIC", "_PKG_CONFIG_ALL_STATIC")))
print(n)
' "${f}" 2>/dev/null)" || return 2
    case "${c}" in '' | *[!0-9]*) return 2 ;; esac
    n=$((n + c))
    }
  done
  echo "${n}"
}

# count_openssl_sys <tree>: 木に出る openssl-sys の package の数。参考の報告用。feature 行は数えず、(*)（重複の省略印）等の
# 末尾の付記を除いてから重複を排除する（付記の有無で同じ package が別行になり水増しされるのを防ぐ）。
count_openssl_sys() {
  local line key seen=$'\n' n=0
  while IFS= read -r line; do
    case "${line}" in 'openssl-sys v'*) ;; *) continue ;; esac
    key="${line}"
    while :; do
      case "${key}" in
        *' (*)') key="${key% (*)}" ;;
        *' (proc-macro)') key="${key% (proc-macro)}" ;;
        *) break ;;
      esac
    done
    case "${seen}" in *$'\n'"${key}"$'\n'*) continue ;; esac
    seen="${seen}${key}"$'\n'
    n=$((n + 1))
  done <<<"$1"
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

  # F10: 静的リンクの環境変数（R4）。env -i で外側の環境（PATH・HOME も）を消し、試験の変数だけを渡して解決済みの Python の実体を直接起動して数える。
  # ハイフンを含む名前（bash の変数名にならない）も数えることを確かめる。
  local ev cnt rc
  for ev in 'OPENSSL_STATIC=1:ge1' 'OPENSSL_STATIC=0:ge1' 'X86_64_UNKNOWN_LINUX_GNU_OPENSSL_STATIC=yes:ge1' 'OPENSSL_STATIC=:ge1' 'PKG_CONFIG_ALL_STATIC=1:ge1' 'x86_64-unknown-linux-gnu_OPENSSL_STATIC=1:ge1' 'aarch64-linux-android_PKG_CONFIG_ALL_STATIC=1:ge1' 'NOT_OPENSSL_STATIC_X=1:eq0' 'NOT-OPENSSL_STATIC_X=1:eq0'; do
    cnt="$(count_static_env env -i "${ev%:*}")" || exit 2
    case "${cnt}" in '' | *[!0-9]*) log "error: selftest F10: non-numeric count; cannot inspect"; exit 2 ;; esac
    if { [ "${ev##*:}" = "eq0" ] && [ "${cnt}" -ne 0 ]; } || { [ "${ev##*:}" = "ge1" ] && [ "${cnt}" -lt 1 ]; }; then
      log "FAIL selftest F10: static env check wrong (case ${ev##*:})"
      exit 1
    fi
  done

  # F10b: python3 が 0 で終わって何も出さない・数字以外を出すときは検査不能（2）。0 件として扱わない（fail-closed）
  local fk
  for fk in empty junk; do
    if ! mkdir -p "${f}/fakepy-${fk}" 2>/dev/null ||
      ! { [ "${fk}" = empty ] && printf '#!/bin/sh\nexit 0\n' || printf '#!/bin/sh\necho x1\nexit 0\n'; } 2>/dev/null >"${f}/fakepy-${fk}/python3" ||
      ! chmod +x "${f}/fakepy-${fk}/python3" 2>/dev/null; then
      log "error: cannot write fixture (f10b); cannot inspect"
      exit 2
    fi
    rc=0
    PY="${f}/fakepy-${fk}/python3" count_static_env env -i >/dev/null 2>&1 || rc=$?
    if [ "${rc}" -ne 2 ]; then
      log "FAIL selftest F10b: fake python output (${fk}) not treated as cannot-inspect"
      exit 1
    fi
  done

  # F10c: 子の bash を、環境に上流が読む正規化した名前（大文字・`-` を `_`）を入れて起動し、スクリプトを source して R4 の環境変数の側を数える。
  # シェルが環境を加工した後に Python が見る経路を通る。出力が数字だけ（source したときに本体が走らず何も出さない）で、1 以上を期待する。
  local out
  rc=0
  # shellcheck disable=SC2016 # 単引用符の中身は子の bash が展開する（$1 は子の引数）。
  out="$(env "X86_64_UNKNOWN_LINUX_GNU_OPENSSL_STATIC=1" "${BASH}" -c '. "$1"; resolve_python; count_static_env' _ "${DIR}/check-openssl-vendored.sh" 2>&1)" || rc=$?
  case "${out}" in '' | *[!0-9]*) rc=1 ;; *) [ "${out}" -ge 1 ] || rc=1 ;; esac
  if [ "${rc}" -ne 0 ]; then
    log "FAIL selftest F10c: normalized target env not counted via sourced shell, or main ran on source"
    exit 1
  fi

  # F11: repo の .cargo/config の OPENSSL_STATIC（R4）
  local cfg
  if ! mkdir -p "${f}/c1/.cargo" "${f}/c2/.cargo" "${f}/c3/.cargo" "${f}/c4" 2>/dev/null ||
    ! printf '[env]\nOPENSSL_STATIC = "1"\n' 2>/dev/null >"${f}/c1/.cargo/config.toml" ||
    ! printf '[env]\nOPENSSL_STATIC = { value = "1", force = true }\n' 2>/dev/null >"${f}/c2/.cargo/config.toml" ||
    ! printf '[env]\nOTHER = "1"\n' 2>/dev/null >"${f}/c3/.cargo/config.toml" ||
    ! mkdir -p "${f}/c5/.cargo" "${f}/c6/.cargo" "${f}/c7/.cargo" "${f}/c8/.cargo" 2>/dev/null ||
    ! printf '[env]\n"OPENSSL_\\u0053TATIC" = "1"\n' 2>/dev/null >"${f}/c5/.cargo/config.toml" ||
    ! printf '# OPENSSL_STATIC = "1"\n[env]\nOTHER = "OPENSSL_STATIC"\n' 2>/dev/null >"${f}/c6/.cargo/config.toml" ||
    ! printf 'env.X86_OPENSSL_STATIC = "0"\n' 2>/dev/null >"${f}/c7/.cargo/config.toml" ||
    ! mkdir -p "${f}/c9/.cargo" "${f}/c10/.cargo" 2>/dev/null ||
    ! printf '[env]\nPKG_CONFIG_ALL_STATIC = "1"\n' 2>/dev/null >"${f}/c9/.cargo/config.toml" ||
    ! printf '[env]\nOPENSSL_STATIC = "1"\n' 2>/dev/null >"${f}/c10/.cargo/config.toml" ||
    ! printf '[env]\nOTHER = "1"\n' 2>/dev/null >"${f}/c10/.cargo/config" ||
    ! printf '[env\nOPENSSL_STATIC = 1\n' 2>/dev/null >"${f}/c8/.cargo/config.toml" ||
    ! mkdir -p "${f}/c11/.cargo" "${f}/c12/.cargo" "${f}/c13/.cargo" 2>/dev/null ||
    ! printf '[env]\nOPENSSL_STATIC = "1"\n' 2>/dev/null >"${f}/c13/.cargo/config" ||
    ! ln -s missing-target "${f}/c13/.cargo/config.toml" 2>/dev/null ||
    ! printf '[env]\nOTHER = "1"\n' 2>/dev/null >"${f}/c11/.cargo/config.toml" ||
    ! printf '[env]\nOPENSSL_STATIC = "1"\n' 2>/dev/null >"${f}/c11/.cargo/config" ||
    ! printf 'env = []\n' 2>/dev/null >"${f}/c12/.cargo/config.toml"; then
    log "error: cannot write fixture (f11); cannot inspect"
    exit 2
  fi
  for cfg in c1:ge1 c2:ge1 c3:eq0 c4:eq0 c5:ge1 c6:eq0 c7:ge1 c9:ge1 c10:ge1 c11:ge1; do
    cnt="$(count_static_config "${f}/${cfg%%:*}")" || exit 2
    if { [ "${cfg##*:}" = "eq0" ] && [ "${cnt}" -ne 0 ]; } || { [ "${cfg##*:}" = "ge1" ] && [ "${cnt}" -lt 1 ]; }; then
      log "FAIL selftest F11: static config check wrong (${cfg%%:*})"
      exit 1
    fi
  done

  # F11b: TOML として不正な設定は検査不能（2）で止まる（構文・読み取りの失敗を 0 件に丸めない）
  for cfg in c8 c12 c13; do
    rc=0
    count_static_config "${f}/${cfg}" >/dev/null 2>&1 || rc=$?
    if [ "${rc}" -ne 2 ]; then
      log "FAIL selftest F11: invalid config not treated as cannot-inspect (${cfg})"
      exit 1
    fi
  done

  # F12: openssl-sys の件数の報告。F3（vendored なしの openssl）は 1 以上、F4（依存なし）は 0
  t="$(run_tree "${f}/f3" offline)" || exit 2
  if [ "$(count_openssl_sys "${t}")" -lt 1 ]; then
    log "FAIL selftest F12: openssl-sys not counted"
    exit 1
  fi
  t="$(run_tree "${f}/f4" offline)" || exit 2
  if [ "$(count_openssl_sys "${t}")" -ne 0 ]; then
    log "FAIL selftest F12: openssl-sys false count"
    exit 1
  fi

  # F12b: (*) の付記の有無で同じ package が別行になっても 1 件（feature 行・別名の package は数えない）
  t=$'openssl-sys v0.9.0 (/x/openssl-sys)\nopenssl-sys v0.9.0 (/x/openssl-sys) (*)\nopenssl-sys feature "default"\nopenssl-sys-extras v0.1.0'
  if [ "$(count_openssl_sys "${t}")" -ne 1 ]; then
    log "FAIL selftest F12: duplicate openssl-sys counted"
    exit 1
  fi

  log "selftest ok (F1-F12, F10b, F10c)"
}

cleanup() { rm -rf "${WORK}" 2>/dev/null || true; }

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
# resolve_python: PY に確認済みの Python の実体を解決する。解決できない・実行できないときは検査不能（2）。
resolve_python() {
  # 環境を消した子（試験の env -i）でも動くよう、確認済みの Python の実体を最初に解決する。pyenv・asdf・mise の shim は HOME 等が無いと起動に失敗するため、
  # 試験では名前（python3）ではなくこの実体を直接起動する。解決できない・実行できないときは検査不能（2）。
  PY="$(python3 -I -c 'import sys; print(sys.executable)' 2>/dev/null)" || PY=""
  case "${PY}" in /*) ;; *) PY="" ;; esac
  if [ -z "${PY}" ] || [ ! -x "${PY}" ]; then
    log "error: cannot resolve python3 executable; cannot inspect"
    exit 2
  fi
}

# main: 直接起動したときの本体（引数の拒否・外部コマンドの確認・自己試験・走査の順）。
main() {
  reject_args "$(basename "$0")" "$@"
  # 使う外部コマンドは作業ディレクトリの作成前にすべて確かめる（EXIT トラップの rm の不足で 127 にしない）。
  for c in cargo mktemp mkdir rm grep dirname basename python3 ln env chmod; do
    require_cmd "${c}" || exit 2
  done
  # TOML は構造として読む（引用キーの Unicode エスケープを文字列検索では見落とすため）。tomllib（3.11 以上）が無ければ検査不能（2）。
  if ! python3 -I -c 'import tomllib' >/dev/null 2>&1; then
    log "error: python3 with tomllib (3.11+) required; cannot inspect"
    exit 2
  fi
  resolve_python

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
  trap cleanup EXIT

  selftest

  # 走査 A（合否）。検査不能（2）と違反（1）はここで確定し、参考の走査 B より先に返す。
  total=0
  check_member_classification
  tree="$(prod_tree "${ROOT_DIR}" locked "${PROD_FEATURES:-}")" || exit 2
  # 版の記録との対応（参考。合否に入れない）。違反（R1〜R4）で終了する場合でも件数と案内が出るよう、本番の木の取得直後に出す。crate の版は OpenSSL の版ではないため出さない。
  sys_n="$(count_openssl_sys "${tree}")"
  if [ "${sys_n}" -ge 1 ]; then
    log "info: openssl-sys in production tree: ${sys_n} (dependency tree only; confirm version record and final binary link mode in Hub AOPS-2)"
  else
    log "info: openssl-sys in production tree: 0 (nothing to record in Hub AOPS-2 from this tree; final binary link mode is confirmed in Hub AOPS-2)"
  fi
  for r in R1 R2 R3; do
    n="$(count_rule "${r}" "${tree}")" || exit 2
    log "scan A ${r}: ${n}"
    total=$((total + n))
  done
  if [ "${total}" -ne 0 ]; then
    log "FAIL: vendored openssl detected in dependency resolution (DPOL-13)"
    exit 1
  fi

  # R4: 静的リンクを強いる設定（検査の環境変数と repo の .cargo/config*）。
  s_env="$(count_static_env)" || exit 2
  s_cfg="$(count_static_config "${ROOT_DIR}")" || {
    log "error: cannot read .cargo config; cannot inspect"
    exit 2
  }
  log "scan A R4: $((s_env + s_cfg))"
  if [ $((s_env + s_cfg)) -ne 0 ]; then
    log "FAIL: static OpenSSL link forced (DPOL-13)"
    exit 1
  fi
  # 走査 B（参考）: 本番対象のメンバーの --all-features。非既定 feature の後ろの経路。
  scan_info B --workspace --all-features "${EXCLUDE_ARGS[@]}"
  # 走査 C（参考）: 試験専用メンバーを含む全メンバーの既定 feature。
  scan_info C --workspace
  log "ok: no vendored openssl (DPOL-13)"
}

# source されたときは本体を走らせない（試験が関数だけを使う）。source しても検出の経路と fail-closed の性質は変わらない。
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
