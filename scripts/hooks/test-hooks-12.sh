#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 12（TASK-714 (1/8)）。check-lib-deps.sh を使い捨ての workspace で実際に流す。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。cargo が要る。無ければ CI（CI か GITHUB_ACTIONS が設定）では失敗、ローカルでは TAP の `# SKIP`（合格に数えない）。
# 作業場所は repo の target/ の下（共有の /tmp の親の .cargo/config.toml を別の利用者に置かれて任意のコードが走るのを防ぐ。本体の check-lib-deps.sh と同じ理由）。
# 期待は緩めない（AUDB-6）。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-core.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-core.sh"

cd "${work_dir}"

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

finish "part 12"
