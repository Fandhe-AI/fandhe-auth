#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 7（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

# --tracked 節の続き（part 5 から続く節。SCAN_ARG は part の間で引き継がないため、ここで改めて指定する）
SCAN_ARG=--tracked
new_repo; stage .gitattributes "* -diff"; stage app.txt "x = ${token}"; git commit -q -m "chore: attr"
check_scan "(tracked) -diff attribute does not hide content" 1 app.txt

new_repo; stage .gitattributes "* binary"; stage app.txt "x = ${token}"; git commit -q -m "chore: attr"
check_scan "(tracked) binary attribute does not hide content" 1 app.txt

new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=hash-object check_scan "(e) (tracked) failure resolving the empty tree" 2
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=-U0 check_scan "(e) (tracked) failure reading diff" 2
SCAN_ARG=

SCAN_ARG=--bogus
new_repo
check_scan "unknown argument" 2
SCAN_ARG=

# ---------------------------------------------------------------- secret-scan: bash 3.2 互換
# bash 3.2 は set -u 下の空配列参照が unbound variable で停止する（引数なしの staged 経路が壊れる）。
# 実行系が 3.2 でなくても検出できるよう、空になりうる配列を参照する形がスクリプトに無いことを静的に確かめる。
if grep -Eq 'base_args' "${SECRET_SCAN}"; then
  fail "secret-scan: (bash3.2) no empty-array reference under set -u" "base_args array is referenced in secret-scan.sh"
else
  pass "secret-scan: (bash3.2) no empty-array reference under set -u"
fi
# 引数なしの通常経路（staged）が差分検査まで到達して終了コード 0 になる
new_repo; stage src/ok.txt "hello"
check_scan "(bash3.2) no-argument staged path reaches diff scan" 0

# ---------------------------------------------------------------- secret-scan: 検査エラー（fail-closed）
# (e) git の失敗を「対象なし」として合格させない
# ファイル名の一覧の取得（--name-only と --diff-filter=ACMR の両方を持つ呼び出し）だけを失敗させる。
# --name-only だけでは、先に index を固定する処理（index_listing）の呼び出しに当たってしまい、この段階の失敗の扱いを試せない（#6011）
new_repo; stage .env "A=1"
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_BOTH1=--name-only FAIL_GIT_BOTH2=--diff-filter=ACMR LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: failed to list files; aborting" "${work_dir}/out"; then
  pass "secret-scan: (e) failure listing staged files exits 2 with the stage-specific message"
else
  fail "secret-scan: (e) failure listing staged files exits 2 with the stage-specific message" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

new_repo; stage app.txt "x = ${token}"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=-U0 check_scan "(e) failure reading staged diff" 2

new_repo; stage app.txt "hello"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=--show-toplevel check_scan "(e) failure resolving repository root" 2

mkdir -p "${work_dir}/not-a-repo"; cd "${work_dir}/not-a-repo"
GIT_CEILING_DIRECTORIES="${work_dir}" check_scan "(e) outside a git repository" 2

# ---------------------------------------------------------------- commit-msg-check
# check_msg <説明> <期待する終了コード>（メッセージは ${work_dir}/msg に書いておく）
check_msg() {
  local desc="$1" want="$2" rc=0 out
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${COMMIT_MSG_CHECK}" "${work_dir}/msg" > "${work_dir}/out" 2>&1 || rc=$?
  out="$(cat "${work_dir}/out")"
  if [ "${rc}" -ne "${want}" ]; then
    fail "commit-msg: ${desc}" "expected exit ${want}, got ${rc}" "output: ${out}"
    return 0
  fi
  pass "commit-msg: ${desc}"
}

msg_ok() {
  printf '%s\n' "$1" > "${work_dir}/msg"
  check_msg "OK: $(printf '%q' "$1")" 0
}

msg_ng() {
  printf '%s\n' "$1" > "${work_dir}/msg"
  check_msg "NG: $(printf '%q' "$1")" 1
}

for type in feat fix docs style refactor perf test build ci chore revert; do
  msg_ok "${type}: summary"
done
msg_ok "feat(auth): ソーシャルログイン機能を追加"
msg_ok "feat(api)!: レスポンス形式を変更"
msg_ok "fix(docs-site2): typo"
msg_ok "feat: caf"$'\xe9'" (Latin-1 subject)"
msg_ok "chore: CRLF line"$'\r'
msg_ok $'\n# comment\n\nfeat: subject after comments and blank lines'
msg_ok "Merge branch 'main' into topic"
msg_ok "Revert \"feat: something\""
msg_ok "fixup! feat: something"
msg_ok "squash! feat: something"
printf 'feat: no trailing newline' > "${work_dir}/msg"
check_msg "OK: subject without trailing newline" 0

msg_ng "bad message"
msg_ng "Feat: capitalized type"
msg_ng "feat(Auth): uppercase scope"
msg_ng "feat(a b): space in scope"
msg_ng "feat:missing space"
msg_ng "feat: "
msg_ng "feature: unknown type"
msg_ng "# only a comment"
: > "${work_dir}/msg"
check_msg "NG: empty file" 1

rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${COMMIT_MSG_CHECK}" "${work_dir}/missing" > /dev/null 2>&1 || rc=$?
if [ "${rc}" -eq 1 ]; then pass "commit-msg: NG: missing message file"; else fail "commit-msg: NG: missing message file" "expected exit 1, got ${rc}"; fi

# ---------------------------------------------------------------- doctor: component 欠落（fail-closed）
# rustup の shim は component が無くても PATH 上に在る。version コマンドが失敗したら必須ツールの欠落として扱う。
doctor_stubs="${work_dir}/doctor-stubs"
mkdir -p "${doctor_stubs}"
for tool in git make lefthook editorconfig-checker shellcheck cargo-fmt cargo-clippy; do
  printf '#!/bin/sh\necho "version: stub"\n' > "${doctor_stubs}/${tool}"
  chmod +x "${doctor_stubs}/${tool}"
done
# cargo 本体は `cargo fmt` の呼び出しだけ失敗させる（rustfmt component 欠落の模擬）
# shellcheck disable=SC2016 # スタブ内で展開させるため単引用符のままにする
printf '#!/bin/sh\n[ "$1" = fmt ] && exit 1\necho "cargo stub"\n' > "${doctor_stubs}/cargo"
chmod +x "${doctor_stubs}/cargo"
rc=0
doctor_out="$(PATH="${doctor_stubs}:${PATH}" "${HOOK_BASH}" "${HOOKS_DIR}/../doctor.sh" 2>&1)" || rc=$?
if [ "${rc}" -ne 0 ] && [[ "${doctor_out}" == *"miss cargo-fmt"* ]]; then
  pass "doctor: NG: cargo fmt --version failure is reported as missing"
else
  fail "doctor: NG: cargo fmt --version failure is reported as missing" "expected non-zero exit and 'miss cargo-fmt', got ${rc}"
fi

# ---------------------------------------------------------------- check-crate-deps: dev 限定の許可
# load-harness -> it-harness・hub-stub は dev の辺に限って許可する。normal・build に移したら拒否する。
# 使い捨ての workspace を作って実際のスクリプトで検証する（cargo が無い環境では実行しない）。
if command -v cargo > /dev/null 2>&1; then
  deps_fx="${work_dir}/crate-deps-fx"
  mkdir -p "${deps_fx}/scripts" "${deps_fx}/crates"
  cp "${HOOKS_DIR}/../check-crate-deps.sh" "${HOOKS_DIR}/../lib.sh" "${deps_fx}/scripts/"
  printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\n' > "${deps_fx}/Cargo.toml"
  for c in it-harness hub-stub; do
    mkdir -p "${deps_fx}/crates/${c}/src"
    printf '[package]\nname = "fandhe-auth-%s"\nversion = "0.0.0"\nedition = "2021"\n' "${c}" > "${deps_fx}/crates/${c}/Cargo.toml"
    : > "${deps_fx}/crates/${c}/src/lib.rs"
  done
  mkdir -p "${deps_fx}/crates/load-harness/src"
  : > "${deps_fx}/crates/load-harness/src/lib.rs"
  for section in dev-dependencies dependencies build-dependencies; do
    printf '[package]\nname = "fandhe-auth-load-harness"\nversion = "0.0.0"\nedition = "2021"\n\n[%s]\nfandhe-auth-it-harness = { path = "../it-harness" }\nfandhe-auth-hub-stub = { path = "../hub-stub" }\n' "${section}" > "${deps_fx}/crates/load-harness/Cargo.toml"
    rm -f "${deps_fx}/Cargo.lock"
    rc=0
    deps_out="$(cd "${deps_fx}" && export HOME="${real_home}" && cargo generate-lockfile --offline 2>&1 && "${HOOK_BASH}" scripts/check-crate-deps.sh 2>&1)" || rc=$?
    if [ "${section}" = "dev-dependencies" ]; then
      if [ "${rc}" -eq 0 ]; then
        pass "crate-deps: OK: load-harness -> it-harness・hub-stub as dev-dependencies"
      else
        fail "crate-deps: OK: dev-dependencies" "expected exit 0, got ${rc}: ${deps_out}"
      fi
    else
      if [ "${rc}" -ne 0 ] && [[ "${deps_out}" == *"forbidden dependency: fandhe-auth-load-harness -> fandhe-auth-it-harness"* ]]; then
        pass "crate-deps: NG: load-harness -> it-harness as ${section}"
      else
        fail "crate-deps: NG: ${section}" "expected forbidden dependency, got ${rc}: ${deps_out}"
      fi
    fi
  done
else
  echo "ok - crate-deps: skipped (cargo not found)"
fi

# ---- check-openssl-vendored.sh: R4 の設定の読み取りは最初の Cargo の前に済み、読めない・include は fail-closed（#6037）
# 偽の cargo（目印を作って 1 で終わる）を PATH の先頭に置き、Cargo を起動する前に 2 で止まることを目印の有無で確かめる。
# 前提: tomllib を持つ python3（3.11 以上）。CI（CI または GITHUB_ACTIONS が設定）では前提が無ければ失敗、ローカルでは skip する。
if ! command -v python3 > /dev/null 2>&1 || ! python3 -I -c 'import tomllib' > /dev/null 2>&1; then
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "vendored: prerequisites (python3 with tomllib) are required in CI" "missing prerequisite" ""
  else
    echo "ok - vendored: skipped (python3 with tomllib not found)"
  fi
else
  vend_fx="${work_dir}/vendored-fx"
  vend_bin="${work_dir}/vendored-fakebin"
  vend_mark="${work_dir}/vendored-cargo-ran"
  mkdir -p "${vend_fx}/scripts" "${vend_bin}"
  cp "${HOOKS_DIR}/../check-openssl-vendored.sh" "${HOOKS_DIR}/../lib.sh" "${vend_fx}/scripts/"
  printf '#!/bin/sh\n: > "%s"\nexit 1\n' "${vend_mark}" > "${vend_bin}/cargo"
  chmod +x "${vend_bin}/cargo"
  # vendored_case <名前> <期待の終了コード> <stderr に含むべき文字列（空なら不問）> <目印の期待: ran|none>
  vendored_case() {
    local name="$1" want_rc="$2" want_msg="$3" want_mark="$4" rc=0 out mark="none"
    rm -f "${vend_mark}"
    out="$(PATH="${vend_bin}:${PATH}" "${HOOK_BASH}" "${vend_fx}/scripts/check-openssl-vendored.sh" 2>&1)" || rc=$?
    [ -e "${vend_mark}" ] && mark="ran"
    if [ "${rc}" -eq "${want_rc}" ] && [ "${mark}" = "${want_mark}" ] && { [ -z "${want_msg}" ] || [[ "${out}" == *"${want_msg}"* ]]; }; then
      pass "vendored: ${name}"
    else
      fail "vendored: ${name}" "expected rc=${want_rc} cargo=${want_mark} msg='${want_msg}', got rc=${rc} cargo=${mark}: ${out}"
    fi
  }
  # V0 対照: .cargo なし。偽の cargo が実際に呼ばれて 2（目印が作られる）
  vendored_case "control: fake cargo is invoked when no .cargo config" 2 "cargo tree failed" ran
  # V1: include を含む設定は Cargo を起動する前に拒否する
  mkdir -p "${vend_fx}/.cargo"
  printf 'include = ["extra.toml"]\n' > "${vend_fx}/.cargo/config.toml"
  vendored_case "NG: include is rejected before cargo runs" 2 "include is not supported" none
  # V2: .cargo に検索権限が無い設定は検査不能（2）。権限が効かない利用者（root 等）では確かめられないため緩める
  printf '[env]\nOPENSSL_STATIC = "1"\n' > "${vend_fx}/.cargo/config.toml"
  chmod 000 "${vend_fx}/.cargo"
  if [ ! -x "${vend_fx}/.cargo" ]; then
    vendored_case "NG: .cargo without search permission is cannot-inspect before cargo runs" 2 "cannot read .cargo config" none
  elif [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    # CI で権限が効かない利用者になると V2 が黙って素通りするため、失敗にする（#6170）
    fail "vendored: .cargo without search permission is cannot-inspect before cargo runs" "search permission is not enforced for this user in CI"
  else
    echo "ok - vendored: .cargo without search permission # SKIP search permission not enforced for this user (not counted as a pass)"
  fi
  chmod u+rwx "${vend_fx}/.cargo"
fi

# ---- scripts/msrv.sh: cargo metadata の解決結果から読む（表記の違い・複数行の文字列に左右されない。読めなければ 2）
# 前提: cargo・jq・tomllib を持つ python3（3.11 以上）。CI（CI または GITHUB_ACTIONS が設定）では前提が足りなければ
# skip せず失敗にする（検査が黙って素通りしないため）。ローカルでは skip する。
msrv_prereq="ok"
if ! command -v cargo > /dev/null 2>&1 || ! command -v jq > /dev/null 2>&1 \
  || ! command -v python3 > /dev/null 2>&1 || ! python3 -I -c 'import tomllib' > /dev/null 2>&1; then
  msrv_prereq="missing"
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "msrv: prerequisites (cargo, jq, python3 with tomllib) are required in CI" "missing prerequisite" ""
  fi
fi
MSRV_SH="${HOOKS_DIR}/../msrv.sh"
if [ "${msrv_prereq}" = "ok" ]; then
  # msrv_ws <名前> <期待の終了状態> <期待の標準出力（空なら確かめない）> <ルート Cargo.toml> [メンバー crate の Cargo.toml]
  # 一時の workspace（メンバー m1 = 与えた内容）を作って msrv.sh を流す。
  msrv_ws() {
    local name="$1" want_rc="$2" want_out="$3" root_body="$4" member_body="${5-}" member2_body="${6-}" rc=0 out dir
    msrv_seq=$((${msrv_seq:-0} + 1))
    dir="${work_dir}/msrv-ws-${msrv_seq}"
    mkdir -p "${dir}/m1/src"
    printf '%s\n' "${root_body}" > "${dir}/Cargo.toml"
    : > "${dir}/m1/src/lib.rs"
    if [ -z "${member_body}" ]; then
      member_body=$'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version.workspace = true'
    fi
    printf '%s\n' "${member_body}" > "${dir}/m1/Cargo.toml"
    if [ -n "${member2_body}" ]; then
      mkdir -p "${dir}/m2/src"
      : > "${dir}/m2/src/lib.rs"
      printf '%s\n' "${member2_body}" > "${dir}/m2/Cargo.toml"
    fi
    out="$(HOME="${real_home}" "${HOOK_BASH}" "${MSRV_SH}" "${dir}/Cargo.toml" 2> "${work_dir}/out")" || rc=$?
    if [ "${rc}" -eq "${want_rc}" ] && { [ -z "${want_out}" ] || [ "${out}" = "${want_out}" ]; }; then
      pass "msrv: ${name}"
    else
      fail "msrv: ${name}" "exit ${rc} (expected ${want_rc}), stdout: ${out}" "stderr: $(cat "${work_dir}/out")"
    fi
  }
  ws_head=$'[workspace]\nresolver = "3"\nmembers = ["m1"]\n[workspace.package]\nedition = "2024"'
  msrv_ws "spaced form is read" 0 "1.100" "${ws_head}"$'\nrust-version = "1.100"'
  msrv_ws "form without spaces is read" 0 "1.100" "${ws_head}"$'\nrust-version="1.100"'
  msrv_ws "trailing comment is ignored" 0 "1.100" "${ws_head}"$'\nrust-version = "1.100" # msrv'
  msrv_ws "a three-part version is passed through" 0 "1.100.2" "${ws_head}"$'\nrust-version = "1.100.2"'
  msrv_ws "a rust-version-looking line inside a multi-line string is not read" 0 "1.100" \
    "${ws_head}"$'\nrust-version = "1.100"\ndescription = """\nrust-version = "9.9"\n"""'
  msrv_ws "a decoy in a multi-line string does not stand in for a missing rust-version" 2 "" \
    "${ws_head}"$'\ndescription = """\nrust-version = "9.9"\n"""'
  msrv_ws "a member without rust-version exits 2" 2 "" "${ws_head}"$'\nrust-version = "1.100"' \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"'
  msrv_ws "members with different rust-versions exit 2" 2 "" \
    $'[workspace]\nresolver = "3"\nmembers = ["m1", "m2"]\n[workspace.package]\nrust-version = "1.100"' \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version.workspace = true' \
    $'[package]\nname = "m2"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.101"'
  msrv_ws "missing rust-version everywhere exits 2" 2 "" "${ws_head}"
  # 全メンバーが同じ別の版を書き、ルートと食い違う（P1。#47）
  msrv_ws "all members sharing a version that differs from the root exit 2" 2 "" \
    "${ws_head}"$'\nrust-version = "1.100"' \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.99"'
  msrv_ws "members equal to the root by their own value are accepted" 0 "1.100" \
    "${ws_head}"$'\nrust-version = "1.100"' \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.100"'
  msrv_ws "a root without rust-version exits 2 even if the member declares one" 2 "" "${ws_head}" \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.100"'
  msrv_ws "a non-string root rust-version exits 2" 2 "" "${ws_head}"$'\nrust-version = 1.100' \
    $'[package]\nname = "m1"\nversion = "0.0.0"\nedition = "2024"\nrust-version = "1.100"'
  rc=0
  "${HOOK_BASH}" "${MSRV_SH}" "${work_dir}/no-such-manifest.toml" > /dev/null 2> "${work_dir}/out" || rc=$?
  expect_rc "msrv: an unreadable manifest exits 2" 2 "${rc}"
  rc=0
  HOME="${real_home}" "${HOOK_BASH}" "${MSRV_SH}" > "${work_dir}/msrv-root.out" 2> "${work_dir}/out" || rc=$?
  expect_rc "msrv: the root Cargo.toml yields a version" 0 "${rc}"
else
  echo "ok - msrv: skipped (cargo, jq or python3 with tomllib not found)"
fi

# ---- scripts/install-msrv.sh: 配布元の設定は 2 で止める・版の引き継ぎと一致確認（rustup・rustc は偽物に差し替える）
INSTALL_MSRV_SH="${HOOKS_DIR}/../install-msrv.sh"
if [ "${msrv_prereq}" = "ok" ]; then
  # install_msrv_case <名前> <期待の終了状態> <偽 rustc の版の上書き（空なら RUSTUP_TOOLCHAIN をそのまま名乗る）>
  #   <GITHUB_ENV に期待する行（空なら書かれないこと）> <rustup の呼び出しの記録の期待（空なら一度も呼ばれないこと）>
  #   [環境変数 NAME=VALUE...]
  # 偽の rustc は RUSTUP_TOOLCHAIN の値を版として返す（export が消えれば unset を返し、一致確認で落ちる）。
  # 偽の rustup は引数を 1 呼び出し 1 行で記録する（配布元の拒否が rustup の呼び出しより前であること、
  # 入れる版・既定にする版がパッチ版であることを確かめる）。偽のコマンドの中身は固定で、パスは環境変数で渡す
  # （TMPDIR 由来の値をシェルのコードへ埋め込まない）。
  # install_msrv_run <偽 rustc の版の上書き> <GITHUB_ENV のパス> [環境変数 NAME=VALUE...]
  #   偽の rustup・rustc を新しい bin ディレクトリに作り、install-msrv.sh を 1 回流す共用の処理。
  #   結果は run_rc（終了状態）・run_calls（rustup の呼び出しの記録）に入れる。stderr は ${work_dir}/out。
  install_msrv_run() {
    local override_ver="$1" gh_env="$2" bin calls
    shift 2
    install_seq=$((${install_seq:-0} + 1))
    bin="${work_dir}/install-msrv-bin-${install_seq}"
    calls="${bin}/rustup-calls"
    mkdir -p "${bin}"
    : > "${calls}"
    cat > "${bin}/rustup" <<'EOF'
#!/bin/sh
echo "$*" >> "${FAKE_RUSTUP_CALLS}"
exit 0
EOF
    cat > "${bin}/rustc" <<'EOF'
#!/bin/sh
echo "rustc ${FAKE_RUSTC_VER:-${RUSTUP_TOOLCHAIN:-unset}} (abcdef 2026-01-01)"
EOF
    chmod +x "${bin}/rustup" "${bin}/rustc"
    run_rc=0
    env -u RUSTUP_DIST_SERVER -u RUSTUP_UPDATE_ROOT -u RUSTUP_TOOLCHAIN ${@+"$@"} \
      FAKE_RUSTUP_CALLS="${calls}" FAKE_RUSTC_VER="${override_ver}" PATH="${bin}:${PATH}" HOME="${real_home}" GITHUB_ENV="${gh_env}" \
      "${HOOK_BASH}" "${INSTALL_MSRV_SH}" > /dev/null 2> "${work_dir}/out" || run_rc=$?
    run_calls="$(cat "${calls}")"
  }
  install_msrv_case() {
    local name="$1" want_rc="$2" override_ver="$3" want_env="$4" want_calls="$5" gh_env
    shift 5
    gh_env="${work_dir}/install-msrv-ghenv-$((${install_seq:-0} + 1))"
    : > "${gh_env}"
    install_msrv_run "${override_ver}" "${gh_env}" ${@+"$@"}
    if [ "${run_rc}" -eq "${want_rc}" ] && [ "$(cat "${gh_env}")" = "${want_env}" ] && [ "${run_calls}" = "${want_calls}" ]; then
      pass "install-msrv: ${name}"
    else
      fail "install-msrv: ${name}" "exit ${run_rc} (expected ${want_rc}), GITHUB_ENV: $(cat "${gh_env}"), rustup calls: ${run_calls} (expected: ${want_calls})" "stderr: $(cat "${work_dir}/out")"
    fi
  }
  want_ver="$(HOME="${real_home}" "${HOOK_BASH}" "${MSRV_SH}")"
  case "${want_ver}" in *.*.*) ;; *) want_ver="${want_ver}.0" ;; esac
  want_calls="toolchain install ${want_ver} --profile minimal"$'\n'"default ${want_ver}"
  install_msrv_case "the patch version is installed, made default, exported and written to GITHUB_ENV" 0 "" "RUSTUP_TOOLCHAIN=${want_ver}" "${want_calls}"
  install_msrv_case "a rustc of another version exits 2" 2 "1.99.0" "" "${want_calls}"
  install_msrv_case "RUSTUP_DIST_SERVER set exits 2 without calling rustup" 2 "" "" "" RUSTUP_DIST_SERVER=https://example.invalid
  install_msrv_case "RUSTUP_UPDATE_ROOT set exits 2 without calling rustup" 2 "" "" "" RUSTUP_UPDATE_ROOT=https://example.invalid
  install_msrv_case "empty RUSTUP_DIST_SERVER exits 2 without calling rustup" 2 "" "" "" RUSTUP_DIST_SERVER=
  install_msrv_case "empty RUSTUP_UPDATE_ROOT exits 2 without calling rustup" 2 "" "" "" RUSTUP_UPDATE_ROOT=
  # 書けない GITHUB_ENV（ディレクトリ。root でも確実に書き込みに失敗する）は、導入と既定化を終えた後の書き込みの段で
  # 2 で止まり、原因を示す誤り文を出す（環境の誤りは 2 の契約。完了の log も出さない）。
  ro_env="${work_dir}/install-msrv-ghenv-dir"
  mkdir -p "${ro_env}"
  install_msrv_run "" "${ro_env}"
  if [ "${run_rc}" -eq 2 ] && [ "${run_calls}" = "${want_calls}" ] \
    && grep -qF "could not write RUSTUP_TOOLCHAIN to GITHUB_ENV" "${work_dir}/out" \
    && ! grep -qF "active toolchain is" "${work_dir}/out"; then
    pass "install-msrv: an unwritable GITHUB_ENV exits 2 after installing"
  else
    fail "install-msrv: an unwritable GITHUB_ENV exits 2 after installing" "exit ${run_rc} (expected 2), rustup calls: ${run_calls} (expected: ${want_calls})" "stderr: $(cat "${work_dir}/out")"
  fi
else
  echo "ok - install-msrv: skipped (cargo, jq or python3 with tomllib not found)"
fi

# 登録を外した名前の既存の本文は、skills-lock.json だけの変更でも staged で全行を走査する（#6054）。
# 準備: vendored-a・vendored-b の本文に検出対象をコミットしておく（登録中は除外されるのでコミット時は 0）。
dropped_repo() {
  same_checkout_repo "${lock_ok}"
  stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
  stage .agents/skills/vendored-b/SKILL.md "y = ${token}"
  SCAN_BIN="${same_bin}" check_scan "dropped-skill setup: registered names are excluded when committed" 0
  git commit -q -m "chore: add skill bodies"
}
lock_only_b='{"version":2,"skills":{"vendored-b":{"source":"x"}}}'

dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
SCAN_BIN="${same_bin}" check_scan "dropped skill: a lock-only change scans the existing body" 1 .agents/skills/vendored-a/SKILL.md
expect_out "dropped skill: counted once, sibling stays excluded" "secret-scan: total: 1 finding(s) in 1 file(s)"

dropped_repo
git rm -q --cached skills-lock.json
rm -f skills-lock.json
SCAN_BIN="${same_bin}" check_scan "dropped skill: a list deleted from index and working tree scans every former name" 1 .agents/skills/vendored-a/SKILL.md
expect_out "dropped skill: both former names counted once" "secret-scan: total: 2 finding(s) in 2 file(s)"

dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
git rm -q -r --cached .agents/skills/vendored-a
SCAN_BIN="${same_bin}" check_scan "dropped skill: removing the files in the same commit scans nothing" 0

dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
printf '%s\n' "z = ${token}" >> .agents/skills/vendored-a/SKILL.md
git add .agents/skills/vendored-a/SKILL.md
SCAN_BIN="${same_bin}" check_scan "dropped skill: a body also modified in the commit" 1 .agents/skills/vendored-a/SKILL.md
expect_out "dropped skill: modified body is not counted twice" "secret-scan: total: 2 finding(s) in 1 file(s)"

# 外した名前と、登録中の別名の変更が同じコミットにあるとき、外した名前の既存の本文は走査され、登録中の別名の変更は除外される
dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
printf '%s\n' "w = ${token}" >> .agents/skills/vendored-b/SKILL.md
git add .agents/skills/vendored-b/SKILL.md
SCAN_BIN="${same_bin}" check_scan "dropped skill: the dropped body is scanned beside a registered sibling's change" 1 .agents/skills/vendored-a/SKILL.md
expect_out "dropped skill: only the dropped name is counted, the registered sibling stays excluded" "secret-scan: total: 1 finding(s) in 1 file(s)"

# 外した名前の下の、引用されるパス（二重引用符・バックスラッシュ。core.quotePath=false でも引用される）のファイルを、
# 登録の解除と同じコミットで変えても、二重に数えない（ヘッダが一覧の生のパスと一致せず、登録名の判定では主の差分の区画を落とせない）。
# 全行（コミット済みの 1 行 + 追加の 1 行 = 2 件）が 1 ファイルとして数えられる。主の差分の区画が残れば 3 件になる。
for qname in 'q"x.md' 'b\s.md'; do
  same_checkout_repo "${lock_ok}"
  mkdir -p .agents/skills/vendored-a
  printf '%s\n' "x = ${token}" > ".agents/skills/vendored-a/${qname}"
  git add -- ".agents/skills/vendored-a/${qname}"
  git commit -q -m "chore: add a quoted-path skill body"
  printf '%s\n' "${lock_only_b}" > skills-lock.json
  printf '%s\n' "z = ${token}" >> ".agents/skills/vendored-a/${qname}"
  git add -- skills-lock.json ".agents/skills/vendored-a/${qname}"
  SCAN_BIN="${same_bin}" check_scan "dropped skill: a quoted path (${qname}) modified with the drop is scanned" 1
  expect_out "dropped skill: a quoted path (${qname}) is not counted twice" "secret-scan: total: 2 finding(s) in 1 file(s)"
done

# 外した名前の本文だけが NUL を含むとき（主の差分には NUL が無い）も、NUL を空白へ置き換えた版を走査する。
# 区切りが NUL のヘッダ文は、NUL を取り除くと一致せず、空白へ置き換えると一致する
same_checkout_repo "${lock_ok}"
mkdir -p .agents/skills/vendored-a
printf 'BEGIN\000PRIVATE KEY\n' > .agents/skills/vendored-a/SKILL.md
git add .agents/skills/vendored-a/SKILL.md
SCAN_BIN="${same_bin}" check_scan "dropped-skill NUL setup: the registered body is excluded when committed" 0
git commit -q -m "chore: add skill body with a NUL"
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
SCAN_BIN="${same_bin}" check_scan "dropped skill: a NUL only in the dropped body is scanned with the space-replaced copy" 1 .agents/skills/vendored-a/SKILL.md

# 作業ツリーだけで登録を消した未ステージの変更は、無関係なコミットを止めない（HEAD と index だけで判定する）
dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
stage unrelated.txt "harmless = 1"
SCAN_BIN="${same_bin}" check_scan "dropped skill: an unstaged drop does not scan the existing body" 0

# 外れた名前の差分の git の失敗は検査不能（2）
dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=":(literal,top).agents/skills/vendored-a/" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan: dropped skill: a failing body diff exits 2"
else
  fail "secret-scan: dropped skill: a failing body diff exits 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 外れた名前の差分は固定した index の複写を読む。外れた名前が無い通常のコミットでは呼ばない
dropped_repo
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
real_index="$(git rev-parse --git-path index)"
rm -f "${work_dir}/gitlog"
PATH="${shim_dir}:${PATH}" LOG_GIT_INDEX="${work_dir}/gitlog" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || true
extra_log="$(grep -F ":(literal,top).agents/skills/vendored-a/" "${work_dir}/gitlog" || true)"
extra_idx="${extra_log%%$'\t'*}"
# 通常の本文の差分（-U0 を持つ、追加の差分以外の diff。--name-only の一覧ではない）と同じ固定コピーを指す（未指定の "-"・実の index では通らない）
main_idx="$(grep -vF ":(literal,top)" "${work_dir}/gitlog" | grep -F -- "-U0" | head -n 1 | cut -f1 || true)"
if [ -n "${extra_log}" ] && [ "${extra_idx}" != "-" ] && [ "${extra_idx}" != "${real_index}" ] && [ -n "${main_idx}" ] && [ "${extra_idx}" = "${main_idx}" ]; then
  pass "secret-scan: dropped skill: the extra diff reads the pinned index copy"
else
  fail "secret-scan: dropped skill: the extra diff reads the pinned index copy" "log: $(cat "${work_dir}/gitlog" 2> /dev/null)"
fi
dropped_repo
stage plain.txt "ok"
rm -f "${work_dir}/gitlog"
rc=0
PATH="${shim_dir}:${PATH}" LOG_GIT_INDEX="${work_dir}/gitlog" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ -s "${work_dir}/gitlog" ] && ! grep -qF ":(literal,top)" "${work_dir}/gitlog"; then
  pass "secret-scan: no dropped name means no extra body diff"
else
  fail "secret-scan: no dropped name means no extra body diff" "log: $(cat "${work_dir}/gitlog" 2> /dev/null)"
fi

# 作業ツリーの skills-lock.json だけに足した名前（HEAD に無い）は、外れた名前として扱わない
dropped_repo
printf '%s\n' '{"version":2,"skills":{"vendored-a":{"source":"x"},"vendored-b":{"source":"x"},"vendored-c":{"source":"x"}}}' > skills-lock.json
# skills-lock.json はステージしない（index には入れず、作業ツリーだけの追加にする）
stage plain.txt "ok"
rm -f "${work_dir}/gitlog"
rc=0
PATH="${shim_dir}:${PATH}" LOG_GIT_INDEX="${work_dir}/gitlog" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ -s "${work_dir}/gitlog" ] && ! grep -qF ":(literal,top)" "${work_dir}/gitlog"; then
  pass "secret-scan: a name added only to the working tree adds no extra body diff"
else
  fail "secret-scan: a name added only to the working tree adds no extra body diff" "exit ${rc}" "log: $(cat "${work_dir}/gitlog" 2> /dev/null)"
fi

# 別の checkout（検出器と走査対象が違う repo）の staged でも、対象側の skills-lock.json だけで外れた名前の既存の本文を走査する
new_repo
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
stage .agents/skills/vendored-b/SKILL.md "y = ${token}"
git add skills-lock.json
git commit -q -m "chore: add skill bodies"
printf '%s\n' "${lock_only_b}" > skills-lock.json
git add skills-lock.json
SCAN_BIN="${det_ok}" check_scan "dropped skill: another checkout scans the dropped body when only skills-lock.json is staged" 1 .agents/skills/vendored-a/SKILL.md
expect_out "dropped skill: another checkout counts the dropped name once, sibling stays excluded" "secret-scan: total: 1 finding(s) in 1 file(s)"

# ---------------------------------------------------------------- check-audit-config: cargo-audit が読む設定の検査（DPOL-34・TASK-679）
# 承認していない audit.toml が cargo-audit の読む場所に在れば 1、検査できなければ 2 で止まる（偽の cargo-audit を PATH の先頭に置く。
# 実物の cargo-audit と advisory DB は使わない）。HOME・CARGO_HOME は fixture 内に向け、開発者の実際の ~/.cargo を読ませない。
AUDIT_CFG_SH="${HOOKS_DIR}/../check-audit-config.sh"
audit_ver="$(sed -n 's/^VERIFIED_CARGO_AUDIT_VERSION="\(.*\)"$/\1/p' "${AUDIT_CFG_SH}")"
ci_audit_ver="$(sed -n 's/^  CARGO_AUDIT_VERSION: "\(.*\)"$/\1/p' "${HOOKS_DIR}/../../.github/workflows/ci.yml")"
if [ -n "${audit_ver}" ] && [ "${audit_ver}" = "${ci_audit_ver}" ]; then
  pass "audit-config: measured version matches ci.yml CARGO_AUDIT_VERSION"
else
  fail "audit-config: measured version matches ci.yml CARGO_AUDIT_VERSION" "script: ${audit_ver}, ci.yml: ${ci_audit_ver}"
fi

audit_fx_seq=0
# audit_stage: audit_case が渡す段（fetch か no-fetch）。既定は fetch。
audit_stage=fetch
# audit_db <dir>: 使い捨ての advisory DB（git の作業ツリー・advisory 1 件・変更なし）を作る。
audit_db() {
  mkdir -p "$1/crates/x"
  echo "id = RUSTSEC-2099-0001" > "$1/crates/x/RUSTSEC-2099-0001.md"
  (cd "$1" && git -c init.defaultBranch=main init -q && git add -A && git commit -q -m "chore: db")
}
# audit_fx: 使い捨ての git repo を作り、cwd をそこへ移す。偽の cargo-audit は AUDIT_FAKE_VER の版を名乗る（中身は固定）。
audit_fx() {
  audit_fx_seq=$((audit_fx_seq + 1))
  audit_root="${work_dir}/audit-fx-${audit_fx_seq}"
  mkdir -p "${audit_root}/scripts" "${audit_root}/crates/trial-deps/fuzz" "${audit_root}/home" "${audit_root}/bin"
  cp "${AUDIT_CFG_SH}" "${HOOKS_DIR}/../lib.sh" "${audit_root}/scripts/"
  cat > "${audit_root}/bin/cargo-audit" <<'FAKE'
#!/bin/sh
echo "cargo-audit-audit ${AUDIT_FAKE_VER}"
FAKE
  chmod +x "${audit_root}/bin/cargo-audit"
  (cd "${audit_root}" && git -c init.defaultBranch=main init -q && git add -A && git commit -q -m "chore: fixture")
  cd "${audit_root}"
}
# audit_case <名前> <期待の終了コード> [env NAME=VALUE...]: fixture で検査を流す（HOME は fixture 内、CARGO_HOME は未設定が既定）
audit_case() {
  local name="$1" want="$2" rc=0
  shift 2
  env -u CARGO_HOME AUDIT_FAKE_VER="${audit_ver}" PATH="${audit_root}/bin:${PATH}" HOME="${audit_root}/home" ${@+"$@"} \
    "${HOOK_BASH}" scripts/check-audit-config.sh "${audit_stage}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq "${want}" ]; then
    pass "audit-config: ${name}"
  else
    fail "audit-config: ${name}" "exit ${rc} (expected ${want})" "output: $(cat "${work_dir}/out")"
  fi
}
audit_has() {
  if grep -qF -- "$2" "${work_dir}/out"; then pass "audit-config: $1"; else fail "audit-config: $1" "missing: $2" "output: $(cat "${work_dir}/out")"; fi
}
audit_lacks() {
  if grep -qF -- "$2" "${work_dir}/out"; then fail "audit-config: $1" "unexpected: $2"; else pass "audit-config: $1"; fi
}
audit_ignore_toml='[advisories]
ignore = ["RUSTSEC-2099-0001"]'

audit_fx
audit_case "OK: nothing present exits 0" 0
audit_has "record: approved config is none" "audit-config: approved config: none"
audit_has "record: root location is absent" "audit-config: <root>/.cargo/audit.toml: absent"
audit_has "record: fuzz location is absent" "audit-config: <root>/crates/trial-deps/fuzz/.cargo/audit.toml: absent"
audit_has "record: CARGO_HOME location is absent" "audit-config: CARGO_HOME/audit.toml (CARGO_HOME from HOME): absent"
audit_has "record: result ok" "audit-config: result: ok"

audit_fx; printf '%s\n' "${audit_ignore_toml}" > audit.toml
audit_case "OK: audit.toml outside .cargo is not read by cargo-audit" 0

audit_fx; mkdir .cargo; printf '%s\n' "${audit_ignore_toml}" > .cargo/audit.toml
audit_case "NG: root .cargo/audit.toml with ignore" 1
audit_has "record: root location is present" "audit-config: <root>/.cargo/audit.toml: present (not approved)"
audit_has "record: result violation" "audit-config: result: violation"
audit_lacks "record: file content is not printed" "RUSTSEC-2099-0001"

audit_fx; mkdir crates/trial-deps/fuzz/.cargo; printf '%s\n' "${audit_ignore_toml}" > crates/trial-deps/fuzz/.cargo/audit.toml
audit_case "NG: fuzz .cargo/audit.toml" 1

audit_fx; mkdir "${audit_root}/ch"; audit_db "${audit_root}/ch/advisory-db"; printf '%s\n' "${audit_ignore_toml}" > "${audit_root}/ch/audit.toml"
audit_case "error: CARGO_HOME set (with audit.toml) exits 2 at the fetch stage" 2 CARGO_HOME="${audit_root}/ch"
audit_has "record: reason (CARGO_HOME is set, fetch)" "CARGO_HOME is set; the fetch stage"
audit_lacks "record: no absolute path" "${audit_root}"
audit_stage=no-fetch
audit_case "error: CARGO_HOME set (with audit.toml) exits 2 at the no-fetch stage" 2 CARGO_HOME="${audit_root}/ch"
audit_has "record: reason (CARGO_HOME is set, no-fetch)" "CARGO_HOME is set; the no-fetch stage"
audit_stage=fetch

# HOME は fixture の repo の外に置く（repo 内に置くと手順 4 の repo 内の件数にも数えられ、HOME 側の検査を消しても通る）
audit_fx; audit_home="${audit_root}.home"; mkdir -p "${audit_home}/.cargo"; audit_db "${audit_home}/.cargo/advisory-db"; printf '%s\n' "${audit_ignore_toml}" > "${audit_home}/.cargo/audit.toml"
audit_case "NG: HOME/.cargo/audit.toml with CARGO_HOME unset" 1 HOME="${audit_home}"
audit_has "record: HOME-side location is present" "(CARGO_HOME from HOME): present (not approved)"
audit_has "record: nothing nested in the repo" "(tracked or untracked): 0"
audit_case "NG: HOME/.cargo/audit.toml with CARGO_HOME empty" 1 CARGO_HOME= HOME="${audit_home}"
audit_has "record: HOME-side location is present (CARGO_HOME empty)" "(CARGO_HOME from HOME): present (not approved)"
audit_stage=no-fetch
audit_case "NG: HOME/.cargo/audit.toml at the no-fetch stage" 1 HOME="${audit_home}"
audit_has "record: HOME-side location is present (no-fetch)" "(CARGO_HOME from HOME): present (not approved)"
audit_has "record: nothing nested in the repo (no-fetch)" "(tracked or untracked): 0"
audit_stage=fetch

audit_fx; mkdir .cargo; ln -s "${audit_root}/missing-target" .cargo/audit.toml
audit_case "NG: dangling symlink .cargo/audit.toml" 1

audit_fx; mkdir -p .cargo/audit.toml
audit_case "NG: directory .cargo/audit.toml" 1

audit_fx; mkdir -p crates/x/.cargo; printf '%s\n' "${audit_ignore_toml}" > crates/x/.cargo/audit.toml; git add -A; git commit -q -m "chore: nested"
audit_case "NG: committed nested .cargo/audit.toml" 1
audit_has "record: nested count" "(tracked or untracked): 1"

audit_fx; mkdir -p crates/x/.cargo; printf '%s\n' "${audit_ignore_toml}" > crates/x/.cargo/audit.toml
audit_case "NG: untracked nested .cargo/audit.toml" 1

audit_fx
audit_case "error: relative CARGO_HOME exits 2" 2 CARGO_HOME=relative/dir
audit_case "error: HOME and CARGO_HOME both unset exits 2" 2 HOME=
audit_case "error: different cargo-audit version exits 2" 2 AUDIT_FAKE_VER=0.0.1
rc=0
env -u CARGO_HOME AUDIT_FAKE_VER="${audit_ver}" PATH="${audit_root}/bin:${PATH}" HOME="${audit_root}/home" \
  "${HOOK_BASH}" scripts/check-audit-config.sh --skip > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then pass "audit-config: error: argument is rejected"; else fail "audit-config: error: argument is rejected" "exit ${rc}"; fi
printf '#!/bin/sh\nexit 3\n' > "${audit_root}/bin/cargo-audit"
audit_case "error: failing cargo-audit exits 2" 2
printf '#!/bin/sh\nexit 0\n' > "${audit_root}/bin/cargo-audit"
audit_case "error: silent cargo-audit exits 2" 2

# 件数の読み取りの陰性対照: grep が失敗（2）・数字でない出力を返しても合格へ進まない
audit_fx
printf '#!/bin/sh\nexit 2\n' > "${audit_root}/bin/grep"
chmod +x "${audit_root}/bin/grep"
audit_case "error: failing grep (empty count) exits 2" 2
audit_has "record: reason (cannot count nested)" "cannot count nested"
audit_lacks "record: no ok result when count is unreadable" "audit-config: result: ok"
printf '#!/bin/sh\necho abc\nexit 0\n' > "${audit_root}/bin/grep"
audit_case "error: non-numeric count exits 2" 2
audit_has "record: reason (cannot read the nested count)" "cannot read the nested"

# 必要なコマンドが PATH に無いときは 1（違反）ではなく 2（検査不能）で止まる（lib.sh の require_cmd は使わない）
# audit_minbin <除くコマンド>: 必要最小限のコマンドだけを symlink した PATH 用のディレクトリを作る。
audit_minbin() {
  local c
  rm -rf "${audit_root}/minbin"
  mkdir -p "${audit_root}/minbin"
  for c in dirname basename git grep tr find; do
    if [ "$c" != "$1" ]; then ln -s "$(command -v "$c")" "${audit_root}/minbin/$c"; fi
  done
  cp "${audit_root}/bin/cargo-audit" "${audit_root}/minbin/cargo-audit"
}
audit_fx
audit_minbin none
audit_case "OK: minimal PATH with all required commands exits 0" 0 PATH="${audit_root}/minbin"
for missing_cmd in grep tr find git dirname; do
  audit_minbin "${missing_cmd}"
  audit_case "error: ${missing_cmd} missing from PATH exits 2" 2 PATH="${audit_root}/minbin"
  audit_has "record: reason (${missing_cmd} not found)" "required command not found on PATH: ${missing_cmd}"
done

# 一覧のパイプの全段の終了: tr の失敗（空の出力・一部の出力）を件数 0 の合格に読まない
audit_fx
printf '#!/bin/sh\nexit 2\n' > "${audit_root}/bin/tr"; chmod +x "${audit_root}/bin/tr"
audit_case "error: failing tr in the repo listing exits 2" 2
audit_has "record: reason (cannot list repository files, tr)" "cannot list repository files"
audit_lacks "record: no ok result when the listing failed" "audit-config: result: ok"
printf '#!/bin/sh\ncat >/dev/null\nexit 1\n' > "${audit_root}/bin/tr"
audit_case "error: tr failing after reading the listing exits 2" 2

# 2 段の検査（DPOL-33・DPOL-34）
audit_fx
rc=0
env -u CARGO_HOME AUDIT_FAKE_VER="${audit_ver}" PATH="${audit_root}/bin:${PATH}" HOME="${audit_root}/home" \
  "${HOOK_BASH}" scripts/check-audit-config.sh > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then pass "audit-config: error: stage argument is required"; else fail "audit-config: error: stage argument is required" "exit ${rc}"; fi
rc=0
env -u CARGO_HOME AUDIT_FAKE_VER="${audit_ver}" PATH="${audit_root}/bin:${PATH}" HOME="${audit_root}/home" \
  "${HOOK_BASH}" scripts/check-audit-config.sh unknown > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then pass "audit-config: error: unknown stage is rejected"; else fail "audit-config: error: unknown stage is rejected" "exit ${rc}"; fi

audit_fx
audit_case "OK: fetch stage with CARGO_HOME unset (no DB needed)" 0
audit_has "record: fetch stage" "audit-config: stage: fetch"
audit_case "OK: fetch stage with CARGO_HOME empty" 0 CARGO_HOME=
audit_case "error: fetch stage with CARGO_HOME set exits 2" 2 CARGO_HOME="${audit_root}/ch"
audit_has "record: reason (CARGO_HOME is set)" "CARGO_HOME is set"
audit_lacks "record: no absolute path (CARGO_HOME set)" "${audit_root}"

audit_stage=no-fetch
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"
audit_case "OK: no-fetch stage with a clean advisory-db" 0
audit_has "record: no-fetch stage" "audit-config: stage: no-fetch"
audit_has "record: advisory count" "git work tree, advisories: 1"
audit_has "record: no changes" "audit-config: advisory-db changes: 0"
audit_lacks "record: no absolute path (db)" "${audit_root}"
audit_fx; audit_db "${audit_root}/ch/advisory-db"
audit_case "error: no-fetch stage with CARGO_HOME set to a clean advisory-db exits 2" 2 CARGO_HOME="${audit_root}/ch"
audit_has "record: reason (CARGO_HOME is set)" "CARGO_HOME is set; the no-fetch stage"

# no-fetch 段の advisory の件数の読み取り: grep・find の失敗を 0 件・一部の件数に読まない
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"
printf '#!/bin/sh\nexit 2\n' > "${audit_root}/bin/grep"; chmod +x "${audit_root}/bin/grep"
audit_case "error: no-fetch stage with failing grep exits 2" 2
audit_has "record: reason (cannot count advisories, grep)" "cannot count advisories"
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"
printf '#!/bin/sh\nexit 2\n' > "${audit_root}/bin/find"; chmod +x "${audit_root}/bin/find"
audit_case "error: no-fetch stage with failing find exits 2" 2
audit_has "record: reason (cannot count advisories, find)" "cannot count advisories"
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"; echo "ignored-file" > "${audit_root}/home/.cargo/advisory-db/.gitignore"; git -C "${audit_root}/home/.cargo/advisory-db" add .gitignore; git -C "${audit_root}/home/.cargo/advisory-db" commit -q -m "chore: ignore"; echo x > "${audit_root}/home/.cargo/advisory-db/ignored-file"
audit_case "error: no-fetch stage with an ignored file exits 2" 2
audit_has "record: reason (has changes, ignored)" "has changes"

# git の上書きの陰性対照: DB 側の設定（未追跡を隠す・fsmonitor）で status の結果を変えられない（#6170）
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"; git -C "${audit_root}/home/.cargo/advisory-db" config status.showUntrackedFiles no; echo x > "${audit_root}/home/.cargo/advisory-db/untracked-file"
audit_case "error: no-fetch stage sees an untracked file despite status.showUntrackedFiles=no" 2
audit_has "record: reason (has changes, showUntrackedFiles)" "has changes"
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"
printf '#!/bin/sh\ntouch "%s/fsmonitor-called"\nprintf "\\0"\n' "${audit_root}" > "${audit_root}/fsm.sh"; chmod +x "${audit_root}/fsm.sh"
git -C "${audit_root}/home/.cargo/advisory-db" config core.fsmonitor "${audit_root}/fsm.sh"
# 陽性対照: 同じ fixture で上書き無しの git status を流すと hook が呼ばれて印が作られる。呼ばれない git では下の陰性対照が意味を持たない
git -C "${audit_root}/home/.cargo/advisory-db" status --porcelain > /dev/null 2>&1 || true
if [ -e "${audit_root}/fsmonitor-called" ]; then
  fsm_control=1
  rm -f "${audit_root}/fsmonitor-called"
else
  fsm_control=0
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "audit-config: the fsmonitor hook is invoked by a plain git status (positive control)" "git did not call core.fsmonitor in CI"
  else
    echo "ok - audit-config: the fsmonitor hook is invoked by a plain git status (positive control) # SKIP git did not call core.fsmonitor here (not counted as a pass)"
  fi
fi
audit_case "OK: no-fetch stage does not call the advisory-db core.fsmonitor" 0
if [ "${fsm_control}" -eq 0 ]; then :; elif [ ! -e "${audit_root}/fsmonitor-called" ]; then pass "audit-config: the advisory-db core.fsmonitor is not invoked"; else fail "audit-config: the advisory-db core.fsmonitor is not invoked" "marker file exists"; fi
# 継いだ GIT_DIR・GIT_WORK_TREE・GIT_INDEX_FILE で別の repo を見ない
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"
mkdir "${audit_root}/other"; (cd "${audit_root}/other" && git -c init.defaultBranch=main init -q && git -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m "chore: other")
audit_case "OK: no-fetch stage ignores an inherited GIT_DIR" 0 GIT_DIR="${audit_root}/other/.git"
audit_stage=fetch
# 継いだ git の向きを消さないと件数が変わる fixture（陽性対照つき）。
#   追跡かつ無視された nested の audit.toml: 正しい読みは追跡（-c）で 1 件。GIT_DIR・GIT_INDEX_FILE が別の（空の）index を指すと、無視された未追跡になり 0 件。
#   未追跡の nested の audit.toml: 正しい読みは未追跡（-o）で 1 件。GIT_WORK_TREE が別の作業ツリーを指すと 0 件。
audit_nested_tracked_ignored() {
  audit_fx; mkdir -p crates/x/.cargo; printf '%s\n' "${audit_ignore_toml}" > crates/x/.cargo/audit.toml
  printf 'crates/x/.cargo/audit.toml\n' > .gitignore
  git add -f crates/x/.cargo/audit.toml .gitignore && git -c user.name=t -c user.email=t@example.invalid commit -q -m "chore: tracked ignored"
  mkdir "${audit_root}/other"; (cd "${audit_root}/other" && git -c init.defaultBranch=main init -q && git -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m "chore: other")
}
audit_nested_tracked_ignored
audit_case "NG: tracked ignored nested .cargo/audit.toml is counted (positive control)" 1
audit_has "record: nested count (tracked ignored)" "(tracked or untracked): 1"
audit_case "NG: nested .cargo/audit.toml is counted despite an inherited GIT_DIR" 1 GIT_DIR="${audit_root}/other/.git"
audit_has "record: nested count with inherited GIT_DIR" "(tracked or untracked): 1"
audit_case "NG: nested .cargo/audit.toml is counted despite an inherited GIT_INDEX_FILE" 1 GIT_INDEX_FILE="${audit_root}/other/empty-index"
audit_has "record: nested count with inherited GIT_INDEX_FILE" "(tracked or untracked): 1"
audit_fx; mkdir -p crates/x/.cargo; printf '%s\n' "${audit_ignore_toml}" > crates/x/.cargo/audit.toml
mkdir "${audit_root}/other"
audit_case "NG: untracked nested .cargo/audit.toml is counted (positive control)" 1
audit_has "record: nested count (untracked)" "(tracked or untracked): 1"
audit_case "NG: nested .cargo/audit.toml is counted despite an inherited GIT_WORK_TREE" 1 GIT_WORK_TREE="${audit_root}/other"
audit_has "record: nested count with inherited GIT_WORK_TREE" "(tracked or untracked): 1"
audit_stage=no-fetch

audit_fx
audit_case "error: no-fetch stage without advisory-db exits 2" 2
audit_has "record: reason (advisory-db is missing)" "advisory-db is missing"

audit_fx; mkdir -p "${audit_root}/home/.cargo/advisory-db/crates/x"; echo "id = RUSTSEC-2099-0001" > "${audit_root}/home/.cargo/advisory-db/crates/x/RUSTSEC-2099-0001.md"
audit_case "error: no-fetch stage with advisory-db not a git work tree exits 2" 2
audit_has "record: reason (not the top of a git work tree)" "not the top of a git work tree"

audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"; git -C "${audit_root}/home/.cargo/advisory-db" rm -q -r crates; git -C "${audit_root}/home/.cargo/advisory-db" commit -q -m "chore: empty"; mkdir "${audit_root}/home/.cargo/advisory-db/crates"
audit_case "error: no-fetch stage with empty crates/ exits 2" 2
audit_has "record: reason (has no advisory)" "has no advisory"

audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"; echo "x" >> "${audit_root}/home/.cargo/advisory-db/crates/x/RUSTSEC-2099-0001.md"
audit_case "error: no-fetch stage with a modified advisory exits 2" 2
audit_has "record: reason (has changes)" "has changes"
audit_fx; audit_db "${audit_root}/home/.cargo/advisory-db"; echo "x" > "${audit_root}/home/.cargo/advisory-db/crates/x/RUSTSEC-2099-0002.md"
audit_case "error: no-fetch stage with an untracked advisory exits 2" 2
audit_has "record: reason (has changes)" "has changes"
audit_stage=fetch

# ---- scripts/*.sh の DIR の解決は CDPATH に左右されない（#6173）
# 実物の repo のルートでは起動しない。停止の検査（引数・環境変数の拒否）が壊れたとき、part 7 の再帰・.env の作成・docker・rustup の
# 起動が実物の repo に及ばないよう、scripts/ の複製（work_dir 内。パスで解決するため git は要らない）の上で起動する。
cdpath_src="${HOOKS_DIR}/../.."
cdpath_root="${work_dir}/cdpath-repo"
mkdir -p "${cdpath_root}"
cp -R "${cdpath_src}/scripts" "${cdpath_root}/scripts"
# `./` を付けない相対パスで起動する（付けると CDPATH が効かず偽の合格になる）。
# 合格は「終了コード 2」と「そのスクリプトの停止の文言が出る」の両方（到達先の取り違え・source の失敗を合格にしない）。
cdpath_case() { # name expected-text env-and-command...
  local name="$1" want="$2" rc=0 out
  shift 2
  out="$(cd "${cdpath_root}" && env CDPATH=. "$@" 2>&1)" || rc=$?
  if [ "${rc}" -eq 2 ] && printf '%s' "${out}" | grep -qF -- "${want}" && ! printf '%s' "${out}" | grep -q 'No such file or directory'; then
    pass "cdpath: ${name} resolves DIR under CDPATH=."
  else
    fail "cdpath: ${name} resolves DIR under CDPATH=." "expected exit 2 with '${want}' and no source failure, got ${rc}" "output: ${out}"
  fi
}
for cdpath_s in check-crate-deps check dev-pg-check doctor e2e-chrome e2e-firefox help it-db-ci it-db lint setup hooks/test-hooks; do
  cdpath_case "${cdpath_s}.sh" "usage: " "${HOOK_BASH}" "scripts/${cdpath_s}.sh" --cdpath-probe
done
# msrv.sh は引数 2 つで require_cmd より前に usage で止まる
cdpath_case "msrv.sh" "usage: msrv.sh" "${HOOK_BASH}" scripts/msrv.sh --cdpath-probe --cdpath-probe
# install-msrv.sh は RUSTUP_DIST_SERVER が（空でも）設定されていると rustup の前に止まる
cdpath_case "install-msrv.sh" "must not be set" RUSTUP_DIST_SERVER= "${HOOK_BASH}" scripts/install-msrv.sh
# test-hooks-core.sh・test-hooks-lib.sh は part から source される。相対パスで source し、HOOKS_DIR が 1 行の絶対パスに解決されることを確かめる
cdpath_hooks="$(cd "${cdpath_root}/scripts/hooks" && pwd)"
for cdpath_s in core lib; do
  cdpath_rc=0
  cdpath_out="$(cd "${cdpath_root}" && CDPATH=. "${HOOK_BASH}" -c "source scripts/hooks/test-hooks-${cdpath_s}.sh && printf '%s' \"\${HOOKS_DIR}\"" 2>&1)" || cdpath_rc=$?
  if [ "${cdpath_rc}" -eq 0 ] && [ "${cdpath_out}" = "${cdpath_hooks}" ]; then
    pass "cdpath: test-hooks-${cdpath_s}.sh resolves HOOKS_DIR under CDPATH=."
  else
    fail "cdpath: test-hooks-${cdpath_s}.sh resolves HOOKS_DIR under CDPATH=." "expected exit 0 and HOOKS_DIR=${cdpath_hooks}, got ${cdpath_rc}" "output: ${cdpath_out}"
  fi
done
# 陰性対照: 修正前の help.sh は CDPATH=. で lib.sh を読めず 1 で落ちる（試験が CDPATH を効かせている証拠）
mkdir -p "${work_dir}/cdpath-old/scripts"
cp "${cdpath_src}/scripts/lib.sh" "${work_dir}/cdpath-old/scripts/lib.sh"
sed '/^unset CDPATH$/d' "${cdpath_src}/scripts/help.sh" > "${work_dir}/cdpath-old/scripts/help.sh"
cdpath_rc=0
cdpath_out="$(cd "${work_dir}/cdpath-old" && CDPATH=. "${HOOK_BASH}" scripts/help.sh --cdpath-probe 2>&1)" || cdpath_rc=$?
if [ "${cdpath_rc}" -eq 1 ] && printf '%s' "${cdpath_out}" | grep -q 'No such file or directory'; then
  pass "cdpath: negative control (without unset CDPATH the source fails)"
else
  fail "cdpath: negative control (without unset CDPATH the source fails)" "expected exit 1 with a source failure, got ${cdpath_rc}" "output: ${cdpath_out}"
fi

finish "part 7"
