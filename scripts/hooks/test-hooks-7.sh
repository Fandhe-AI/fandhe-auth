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

finish "part 7"
