#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 12（TASK-714 (1/8)）。check-lib-deps.sh を使い捨ての workspace で実際に流す。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。cargo が要る。無ければ CI（CI か GITHUB_ACTIONS が設定）では失敗、ローカルでは TAP の `# SKIP`（合格に数えない）。
# 作業場所は repo の target/ の下（共有の /tmp の親の .cargo/config.toml を別の利用者に置かれて任意のコードが走るのを防ぐ。本体の check-lib-deps.sh と同じ理由）。
# 期待は緩めない（AUDB-6）。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-core.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-core.sh"

cd "${work_dir}"

# (A) plugin・hub の表の試験（TASK-714 (2/8)・LIB-2）。cargo 不要。実のスクリプトを source して実の表を照合する。
# 行を消す・緩めるとここが落ちる。当たってはならない名前（本体・近い名前）も固定する。
ld_tbl() {
  (
    # shellcheck source=../check-lib-deps.sh
    source "${HOOKS_DIR}/../check-lib-deps.sh"
    match_forbidden "$1" || true
  )
}
ld_expect() {
  local want="$1" n got
  shift
  for n in "$@"; do
    got="$(ld_tbl "${n}")"
    if [ "${got}" = "${want}" ]; then
      pass "lib-deps table: ${n} -> '${want}'"
    else
      fail "lib-deps table: ${n} -> '${want}'" "got '${got}'"
    fi
  done
}
ld_expect plugin fandhe-auth-plugin-oidc fandhe-auth-plugin-saml openidconnect oauth2 samael
ld_expect hub fandhe-auth-backend fandhe-auth-db-pg fandhe-auth-db-fandhe fandhe-auth-db-migrate fandhe-auth-server fandhe-auth-db \
  fandhe-service-hub fandhe-service-hub-x
ld_expect http axum axum-core actix-web actix-http rocket warp poem salvo hyper hyper-util tower-http fandhe-backend fandhe-backend-core fandhe-backend-http fandhe-backend-plugin-jwt
ld_expect db sqlx sqlx-core sqlx-postgres tokio-postgres postgres diesel rusqlite libsqlite3-sys sea-orm mysql mysql_async redis mongodb deadpool-postgres bb8
ld_expect runtime tokio tokio-util tokio-macros async-std smol actix-rt glommio monoio futures-executor
ld_expect "" http-body futures-core futures-util async-trait tokiox sqlxx pin-project-lite
ld_expect sdk aws-config aws-sdk-s3 aws-sdk-sts google-cloud-storage google-apis-common yup-oauth2 gcp_auth egg-mode facebook facebook-graph twitter twitter-v2 azure_core azure_identity
ld_expect "" aws-lc-rs aws-lc-sys aws-configx googletest yup-oauth2-x gcp_auth_x azure ring jsonwebtoken
ld_expect "" fandhe-auth-core fandhe-auth-secret fandhe-auth-db-tests fandhe-auth-client fandhe-auth-plugin openidconnect-x fandhe-backendx
# 表が本体の crate 自身に当たるなら検査不能（2）
ld_rc=0
(
  # shellcheck source=../check-lib-deps.sh
  source "${HOOKS_DIR}/../check-lib-deps.sh"
  FORBIDDEN=('x|prefix|fandhe-auth-')
  require_body_not_forbidden
) > /dev/null 2>&1 || ld_rc=$?
if [ "${ld_rc}" -eq 2 ]; then
  pass "lib-deps table: body crate hit is cannot-inspect (2)"
else
  fail "lib-deps table: body crate hit is cannot-inspect (2)" "got ${ld_rc}"
fi

if ! command -v cargo > /dev/null 2>&1; then
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "lib-deps: cargo is required in CI" "missing prerequisite"
  else
    echo "ok - lib-deps # SKIP cargo not found (not counted as a pass)"
  fi
  finish "part 12"
  exit 0
fi

ld_repo="$(cd "${HOOKS_DIR}/../.." && pwd)"
mkdir -p "${ld_repo}/target"
ld_root="$(mktemp -d "${ld_repo}/target/test-hooks-12.XXXXXX")"
# part の終了時に作業場所を消す（core の trap を引き継ぎ、自分の分を足す）。
trap 'chmod -R u+rwx "${work_dir}" "${ld_root}" 2> /dev/null; rm -rf "${work_dir}" "${ld_root}"' EXIT
ld_fx="${ld_root}/lib-deps-fx"
ld_pf=""
ld_bodies="core secret db-tests client"
# 禁止の crate は実の表にある完全一致の名前（runtime 区分）。表から消えたら (2) 以降が落ちて気付ける。
ld_bad="tokio"

# ld_write <crate> <追加の Cargo.toml 本文>: body の crate の manifest を書く。
ld_write() {
  mkdir -p "${ld_fx}/crates/$1/src"
  printf '[package]\nname = "fandhe-auth-%s"\nversion = "0.0.0"\nedition = "2021"\n%s\n' "$1" "$2" > "${ld_fx}/crates/$1/Cargo.toml"
  : > "${ld_fx}/crates/$1/src/lib.rs"
}

# ld_run <名前> <期待の終了状態> [期待する出力の断片]: 使い捨ての workspace で本物のスクリプトを流す。
ld_run() {
  local name="$1" want="$2" frag="${3-}" rc=0 out
  rm -f "${ld_fx}/Cargo.lock"
  out="$(cd "${ld_fx}" && export HOME="${real_home}" PROD_FEATURES="${ld_pf}" && cargo generate-lockfile --offline 2>&1 && "${HOOK_BASH}" scripts/check-lib-deps.sh 2>&1)" || rc=$?
  if [ "${rc}" -eq "${want}" ] && { [ -z "${frag}" ] || [[ "${out}" == *"${frag}"* ]]; }; then
    pass "lib-deps: ${name}"
  else
    fail "lib-deps: ${name}" "expected exit ${want} with '${frag}', got ${rc}: ${out}"
  fi
}

mkdir -p "${ld_fx}/scripts" "${ld_fx}/crates/${ld_bad}/src" "${ld_fx}/crates/mid/src"
cp "${HOOKS_DIR}/../check-lib-deps.sh" "${HOOKS_DIR}/../lib.sh" "${ld_fx}/scripts/"
printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\n' > "${ld_fx}/Cargo.toml"
printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2021"\n' "${ld_bad}" > "${ld_fx}/crates/${ld_bad}/Cargo.toml"
: > "${ld_fx}/crates/${ld_bad}/src/lib.rs"
printf '[package]\nname = "mid"\nversion = "0.0.0"\nedition = "2021"\n[dependencies]\n%s = { path = "../%s" }\n' "${ld_bad}" "${ld_bad}" > "${ld_fx}/crates/mid/Cargo.toml"
: > "${ld_fx}/crates/mid/src/lib.rs"

dep_line="${ld_bad} = { path = \"../${ld_bad}\" }"
for c in ${ld_bodies}; do ld_write "${c}" ""; done

# (1) 依存なし
ld_run "OK: no dependencies" 0
# (2) core が禁止 crate に normal で依存
ld_write core "[dependencies]
${dep_line}"
ld_run "NG: direct normal dependency" 1 "violation: fandhe-auth-core -> ${ld_bad}"
# (3) dev-dependencies だけ
ld_write core "[dev-dependencies]
${dep_line}"
ld_run "OK: dev-dependencies only" 0
ld_write core ""
# (4) client が中間 crate 経由で推移的に依存
ld_write client '[dependencies]
mid = { path = "../mid" }'
ld_run "NG: transitive dependency" 1 "violation: fandhe-auth-client -> ${ld_bad}"
ld_write client ""
# (4a) core が plugin の crate に normal で依存（TASK-714 (2/8)）
ld_write plugin-oidc ""
ld_write core '[dependencies]
fandhe-auth-plugin-oidc = { path = "../plugin-oidc" }'
ld_run "NG: plugin crate dependency" 1 "violation: fandhe-auth-core -> fandhe-auth-plugin-oidc [plugin]"
ld_write core ""
# (4b) client が中間 crate 経由で hub の crate に推移的に依存
ld_write db-pg ""
mkdir -p "${ld_fx}/crates/mid-hub/src"
printf '[package]\nname = "mid-hub"\nversion = "0.0.0"\nedition = "2021"\n[dependencies]\nfandhe-auth-db-pg = { path = "../db-pg" }\n' > "${ld_fx}/crates/mid-hub/Cargo.toml"
: > "${ld_fx}/crates/mid-hub/src/lib.rs"
ld_write client '[dependencies]
mid-hub = { path = "../mid-hub" }'
ld_run "NG: transitive hub crate dependency" 1 "violation: fandhe-auth-client -> fandhe-auth-db-pg [hub]"
ld_write client ""
# (4c) core が sdk の crate に normal で直接依存（TASK-714 (3/8)）
mkdir -p "${ld_fx}/crates/aws-sdk-s3/src"
printf '[package]\nname = "aws-sdk-s3"\nversion = "0.0.0"\nedition = "2021"\n' > "${ld_fx}/crates/aws-sdk-s3/Cargo.toml"
: > "${ld_fx}/crates/aws-sdk-s3/src/lib.rs"
ld_write core '[dependencies]
aws-sdk-s3 = { path = "../aws-sdk-s3" }'
ld_run "NG: direct sdk dependency" 1 "violation: fandhe-auth-core -> aws-sdk-s3 [sdk]"
ld_write core ""
# (4d) secret が中間 crate 経由で sdk の crate に推移的に依存
mkdir -p "${ld_fx}/crates/yup-oauth2/src" "${ld_fx}/crates/mid-sdk/src"
printf '[package]\nname = "yup-oauth2"\nversion = "0.0.0"\nedition = "2021"\n' > "${ld_fx}/crates/yup-oauth2/Cargo.toml"
: > "${ld_fx}/crates/yup-oauth2/src/lib.rs"
printf '[package]\nname = "mid-sdk"\nversion = "0.0.0"\nedition = "2021"\n[dependencies]\nyup-oauth2 = { path = "../yup-oauth2" }\n' > "${ld_fx}/crates/mid-sdk/Cargo.toml"
: > "${ld_fx}/crates/mid-sdk/src/lib.rs"
ld_write secret '[dependencies]
mid-sdk = { path = "../mid-sdk" }'
ld_run "NG: transitive sdk dependency" 1 "violation: fandhe-auth-secret -> yup-oauth2 [sdk]"
ld_write secret ""
# (4e) client が近い名前（暗号の実装）の crate に依存しても誤検出しない
mkdir -p "${ld_fx}/crates/aws-lc-rs/src"
printf '[package]\nname = "aws-lc-rs"\nversion = "0.0.0"\nedition = "2021"\n' > "${ld_fx}/crates/aws-lc-rs/Cargo.toml"
: > "${ld_fx}/crates/aws-lc-rs/src/lib.rs"
ld_write client '[dependencies]
aws-lc-rs = { path = "../aws-lc-rs" }'
ld_run "OK: aws-lc-rs is not sdk" 0
ld_write client ""
# (4f) db-tests が http の crate に normal で直接依存（TASK-714 (4/8)）
mkdir -p "${ld_fx}/crates/axum/src"
printf '[package]\nname = "axum"\nversion = "0.0.0"\nedition = "2021"\n' > "${ld_fx}/crates/axum/Cargo.toml"
: > "${ld_fx}/crates/axum/src/lib.rs"
ld_write db-tests '[dependencies]
axum = { path = "../axum" }'
ld_run "NG: direct http dependency" 1 "violation: fandhe-auth-db-tests -> axum [http]"
ld_write db-tests ""
# (4g) client が中間 crate 経由で db の crate に推移的に依存
mkdir -p "${ld_fx}/crates/sqlx-postgres/src" "${ld_fx}/crates/mid-db/src"
printf '[package]\nname = "sqlx-postgres"\nversion = "0.0.0"\nedition = "2021"\n' > "${ld_fx}/crates/sqlx-postgres/Cargo.toml"
: > "${ld_fx}/crates/sqlx-postgres/src/lib.rs"
printf '[package]\nname = "mid-db"\nversion = "0.0.0"\nedition = "2021"\n[dependencies]\nsqlx-postgres = { path = "../sqlx-postgres" }\n' > "${ld_fx}/crates/mid-db/Cargo.toml"
: > "${ld_fx}/crates/mid-db/src/lib.rs"
ld_write client '[dependencies]
mid-db = { path = "../mid-db" }'
ld_run "NG: transitive db dependency" 1 "violation: fandhe-auth-client -> sqlx-postgres [db]"
ld_write client ""
# (5) 対象の crate を 1 つ外す
rm -rf "${ld_fx}/crates/secret"
ld_run "cannot inspect: body crate missing" 2
mkdir -p "${ld_fx}/crates/secret/src"
ld_write secret ""
# (6) PROD_FEATURES の crate 名が検査の対象に無い（workspace に無い名前・対象外の crate）は黙って無視せず 2
ld_pf="nonexistent/x"
ld_run "cannot inspect: PROD_FEATURES names a crate not in workspace" 2 "not in the inspected set"
ld_pf="mid/x"
ld_run "cannot inspect: PROD_FEATURES names a non-body crate" 2 "not in the inspected set"
# (7) 対象の crate 宛ての要素は名前の検査を通る。形式の誤りは従来どおり 2
ld_write core "[features]
extra = []"
ld_pf="fandhe-auth-core/extra"
ld_run "OK: PROD_FEATURES names a body crate" 0
ld_pf="fandhe-auth-core"
ld_run "cannot inspect: PROD_FEATURES malformed" 2 "must be comma-separated"
ld_pf=""
ld_write core ""
# (8) glob の文字を含む名前は展開されない。cwd に一致するファイルがあっても BODY_PKGS に無い名前として 2（#6170）
mkdir -p "${ld_fx}/co-glob-probe"
: > "${ld_fx}/co-glob-probe/x"
ld_pf="co*/x"
ld_run "cannot inspect: PROD_FEATURES with a glob character is not expanded" 2 "not in the inspected set (co*)"
ld_pf=""
rm -rf "${ld_fx}/co-glob-probe"

finish "part 12"
