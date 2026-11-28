#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 5（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

# ---- 規則表（TASK-6）: 規則ごとの検出・件数の出力
enc_key_line="${dash}BEGIN ENCRYPTED PRIVATE"" KEY${dash}"

new_repo; stage a.txt "x = ${jwt_value}"
check_scan "jwt shape" 1 a.txt
expect_out "jwt rule is counted" "secret-scan: jwt: 1 line(s)"

new_repo; stage a.txt "{${q}d${q}: ${q}${jwk_value}${q}}"
check_scan "jwk private component" 1 a.txt
expect_out "jwk-private rule is counted" "secret-scan: jwk-private: 1 line(s)"

new_repo; stage a.txt "${enc_key_line}"
check_scan "encrypted private key header" 1 a.txt
expect_out "private-key rule is counted" "secret-scan: private-key: 1 line(s)"

new_repo; stage a.txt "recovery_codes = ${q}${recovery_value}${q}"
check_scan "recovery code assignment (base64url-like value)" 1 a.txt
expect_out "assignment rule is counted" "secret-scan: assignment: 1 line(s)"

new_repo; stage a.txt "passphrase: ${q}abc_DEF+ghi/JKL=mno-pqr${q}"
check_scan "passphrase assignment" 1 a.txt

new_repo; stage a.txt "x = ${token}"; stage b.txt "${password_line}"; stage c.txt "y = ${token}"
check_scan "multiple rules and files" 1 a.txt
expect_out "per-rule count" "secret-scan: vendor-token: 2 line(s)"
expect_out "password counted under assignment" "secret-scan: assignment: 1 line(s)"
expect_out "total count" "secret-scan: total: 3 finding(s) in 3 file(s)"

new_repo; stage a.txt "sha256: $(printf 'ab%.0s' {1..32})"; stage b.txt "token = ${q}short${q}"
check_scan "hex digest and short value are allowed" 0

# ---- --tracked
SCAN_ARG=--tracked
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
check_scan "(tracked) clean repository" 0
expect_out "(tracked) zero count line" "secret-scan: total: 0 finding(s) (1 tracked file(s) checked)"

new_repo; stage app.txt "x = ${token}"; git commit -q -m "chore: leak"
check_scan "(tracked) committed secret" 1 app.txt
expect_out "(tracked) count line" "secret-scan: total: 1 finding(s) in 1 file(s)"

new_repo; stage .env "A=1"; git commit -q -m "chore: env"
check_scan "(tracked) committed .env" 1 .env
expect_out "(tracked) env-file count" "secret-scan: env-file: 1 file(s)"

# --summary: 件数は同じで、該当ファイル名を出力に含めない（公開ログ用。SECR-23）
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ] && grep -qxF "secret-scan: env-file: 1 file(s)" "${work_dir}/out" && ! grep -q "^  " "${work_dir}/out" ; then
  pass "secret-scan: (tracked) --summary omits file names"
else
  fail "secret-scan: (tracked) --summary omits file names" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"; git commit -q -m "chore: skill"
SCAN_BIN="${det_ok}" check_scan "(tracked) registered skill path is excluded" 0

new_repo; stage_lock; stage .agents/skills/other/SKILL.md "x = ${token}"; stage deps.lock "x = ${token}"
stage a/b/foo.lock "x = ${token}"; stage .claude/skills/y/SKILL.md "x = ${token}"; git commit -q -m "chore: skill"
SCAN_BIN="${det_ok}" check_scan "(tracked) unregistered skill path, *.lock and .claude/skills are scanned" 1 deps.lock
expect_out "(tracked) all four paths are counted" "total: 4 finding(s) in 4 file(s)"

new_repo; stage_lock; stage .agents/skills/vendored-a-evil/x.md "x = ${token}"; stage .agents/skills/vendored-a "x = ${token}"
git commit -q -m "chore: skill"
SCAN_BIN="${det_ok}" check_scan "(tracked) sibling name and exact-name blob are scanned" 1 .agents/skills/vendored-a

new_repo; printf 'x = %s\n\000' "${token}" > bin.dat; git add bin.dat; git commit -q -m "chore: bin"
check_scan "(tracked) NUL-containing file is scanned (fail-closed)" 1 bin.dat

new_repo; printf 'pad\000x = %s\n' "${token}" > nulpre.dat; git add nulpre.dat; git commit -q -m "chore: nul prefix"
check_scan "(tracked) secret after a NUL byte is detected (fail-closed)" 1 nulpre.dat

new_repo; printf '\000\000\000x = %s\n' "${token}" > nulmulti.dat; git add nulmulti.dat; git commit -q -m "chore: nul run"
check_scan "(tracked) secret after consecutive NUL bytes is detected" 1 nulmulti.dat

new_repo; printf 'x = %s\000%s\n' "${token:0:8}" "${token:8}" > nulsplit.dat; git add nulsplit.dat; git commit -q -m "chore: nul split"
check_scan "(tracked) secret split by a NUL byte is detected" 1 nulsplit.dat

# NUL と同じ行の秘密値は 2 回の走査（NUL 除去・空白置換）でも 1 件として数える
new_repo; printf 'pad\000x = %s\n' "${token}" > nulcount.dat; git add nulcount.dat; git commit -q -m "chore: nul count"
check_scan "(tracked) NUL line is counted once" 1 nulcount.dat
expect_out "(tracked) NUL line count" "secret-scan: total: 1 finding(s) in 1 file(s)"

# --summary は git の診断（標準エラー）を捨て、通常モードは受け継いだ標準エラーに出す
new_repo
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=ls-files LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && ! grep -q "git shim" "${work_dir}/out"; then
  pass "secret-scan: (tracked) --summary drops git diagnostics (ls-files)"
else
  fail "secret-scan: (tracked) --summary drops git diagnostics (ls-files)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=hash-object LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && ! grep -q "git shim" "${work_dir}/out"; then
  pass "secret-scan: (tracked) --summary drops git diagnostics (hash-object)"
else
  fail "secret-scan: (tracked) --summary drops git diagnostics (hash-object)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=ls-files LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -q "git shim" "${work_dir}/out"; then
  pass "secret-scan: (tracked) normal mode keeps git diagnostics on stderr"
else
  fail "secret-scan: (tracked) normal mode keeps git diagnostics on stderr" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# ---- scripts/secret-scan-ci.sh（CI 向けの入口）
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan-ci: clean repository passes"
else
  fail "secret-scan-ci: clean repository passes" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 作業ツリーの index が HEAD と違っても、走査するのは HEAD の内容（専用の一時 index）
# (a) index にだけ秘密値がある（HEAD は無害）→ HEAD を走査するので 0
stage extra.txt "x = ${token}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan-ci: scans HEAD even if the working index differs (staged secret ignored)"
else
  fail "secret-scan-ci: scans HEAD even if the working index differs (staged secret ignored)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
# (b) HEAD に秘密値があり、index からは外してある → HEAD を走査するので 1
new_repo; stage app.txt "x = ${token}"; git commit -q -m "chore: leak"
git rm -q --cached app.txt
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ]; then
  pass "secret-scan-ci: scans HEAD even if the working index differs (secret in HEAD detected)"
else
  fail "secret-scan-ci: scans HEAD even if the working index differs (secret in HEAD detected)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 呼び出し元の GIT_INDEX_FILE（秘密値入りの別 index）を渡しても、対象の本来の index（HEAD と一致）を走査する
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
alt_index="${work_dir}/alt-index-${repo_seq}"
GIT_INDEX_FILE="${alt_index}" git read-tree HEAD
printf '%s\n' "x = ${token}" > leak.txt
GIT_INDEX_FILE="${alt_index}" git add leak.txt
rc=0
GIT_INDEX_FILE="${alt_index}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan-ci: caller GIT_INDEX_FILE is ignored"
else
  fail "secret-scan-ci: caller GIT_INDEX_FILE is ignored" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 親のリポジトリを走査しないよう、リポジトリのルートでない対象は exit 2
mkdir -p "${repo}/sub"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}/sub" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan-ci: non-root target exits 2"
else
  fail "secret-scan-ci: non-root target exits 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 置換参照（refs/replace）で HEAD の blob を無害なものへ差し替えても検出する
new_repo; stage app.txt "x = ${token}"; git commit -q -m "chore: leak"
secret_blob="$(git rev-parse HEAD:app.txt)"
harmless_blob="$(printf 'harmless\n' | git hash-object -w --stdin)"
git replace "${secret_blob}" "${harmless_blob}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ]; then
  pass "secret-scan-ci: refs/replace does not hide a secret in HEAD"
else
  fail "secret-scan-ci: refs/replace does not hide a secret in HEAD" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 名前が `-` のディレクトリを対象にしても、cd が OLDPWD へ移動せず、そのディレクトリを走査する
dash_parent="${work_dir}/dash-parent"
mkdir -p "${dash_parent}/-"
(
  cd "${dash_parent}/-"
  git -c init.defaultBranch=main init -q
  printf 'hello\n' > app.txt
  git add app.txt
  git commit -q -m "chore: add"
)
rc=0
(cd "${dash_parent}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" -) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan-ci: a target named '-' is scanned as a directory"
else
  fail "secret-scan-ci: a target named '-' is scanned as a directory" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 検出器を起動するサブシェルの cd が失敗したら、終了状態 1（検出）と取り違えず検査不能（2）にする
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
repo_real="$(cd "${repo}" && pwd -P)"
cd_counter="${work_dir}/cd-counter"
rm -f "${cd_counter}"
cd_fail_env="${work_dir}/cd-fail.sh"
cat > "${cd_fail_env}" <<'EOF'
cd() {
  if [ "${1:-}" = "${CD_FAIL_TARGET}" ]; then
    local n=0
    [ -f "${CD_FAIL_COUNTER}" ] && n="$(cat "${CD_FAIL_COUNTER}")"
    n=$((n + 1))
    echo "${n}" > "${CD_FAIL_COUNTER}"
    # 1 回目 = 対象の解決、2 回目 = ルートの確認、3 回目 = 検出器を起動するサブシェル
    if [ "${n}" -ge 3 ]; then
      return 1
    fi
  fi
  builtin cd "$@"
}
EOF
rc=0
CD_FAIL_TARGET="${repo_real}" CD_FAIL_COUNTER="${cd_counter}" BASH_ENV="${cd_fail_env}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo_real}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan-ci: failure to enter the target before scanning exits 2 (not 1)"
else
  fail "secret-scan-ci: failure to enter the target before scanning exits 2 (not 1)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 空文字の引数は既定の対象に落とさず、検査不能（2）にする（既定は引数を省略したときだけ）
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan-ci: an explicit empty target exits 2"
else
  fail "secret-scan-ci: an explicit empty target exits 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# パスが改行で終わるリポジトリも、末尾の改行を失わずに走査する（検出器・CI 入口とも）
nl_dir="${work_dir}/nl-parent/repo"$'\n'
mkdir -p "${nl_dir}"
(
  cd "${nl_dir}"
  git -c init.defaultBranch=main init -q
  printf 'hello\n' > app.txt
  git add app.txt
  git commit -q -m "chore: add"
)
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${nl_dir}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan-ci: a target whose path ends with a newline is scanned"
else
  fail "secret-scan-ci: a target whose path ends with a newline is scanned" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
(cd "${nl_dir}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: (tracked) a repository whose path ends with a newline is scanned"
else
  fail "secret-scan: (tracked) a repository whose path ends with a newline is scanned" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# ---- scripts/secret-scan-canary.sh: grep のエラー（終了状態 2）は失敗にする
grep_shim_dir="${work_dir}/grep-shim"
mkdir -p "${grep_shim_dir}"
real_grep="$(command -v grep)"
cat > "${grep_shim_dir}/grep" <<'EOF'
#!/bin/sh
if [ "${1:-}" = "-qF" ]; then
  exit 2
fi
exec "${REAL_GREP}" "$@"
EOF
chmod +x "${grep_shim_dir}/grep"
rc=0
REAL_GREP="${real_grep}" PATH="${grep_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "scan-canary: grep error on the leak check fails (exit 2)"
else
  fail "scan-canary: grep error on the leak check fails (exit 2)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
# 検出器の 1 行目のバイト単位の確認（grep -anxF）のエラーも、不一致と取り違えず検査不能（2）にする（#6011）
cat > "${grep_shim_dir}/grep" <<'EOF'
#!/bin/sh
if [ "${1:-}" = "-anxF" ]; then
  exit 2
fi
exec "${REAL_GREP}" "$@"
EOF
chmod +x "${grep_shim_dir}/grep"
rc=0
REAL_GREP="${real_grep}" PATH="${grep_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qF "scan-canary: could not check the scanner's first line (grep exit 2)" "${work_dir}/out"; then
  pass "scan-canary: grep error on the shebang check fails (exit 2)"
else
  fail "scan-canary: grep error on the shebang check fails (exit 2)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "scan-canary: passes without a grep error"
else
  fail "scan-canary: passes without a grep error" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# ---- index の固定: 親ディレクトリのシンボリックリンクの付け替え（#6034。part 3 の index の節の続き）
# --path-format=absolute は親ディレクトリのリンクも解いた実体を返す。実体から複写し直すだけでは、走査中の付け替えで
# git が実際に読む index が別のディレクトリへ移っても、一覧が一致して 0 を返し得る。終了時に git 自身の解決をやり直して比べる。
new_repo; stage src/app.txt "hello"
mkdir -p "${work_dir}/idx-a" "${work_dir}/idx-b" "${work_dir}/idx-c"
cp -- "$(git rev-parse --path-format=absolute --git-path index)" "${work_dir}/idx-a/index"
cp -- "${work_dir}/idx-a/index" "${work_dir}/idx-b/index"
cp -- "${work_dir}/idx-a/index" "${work_dir}/idx-c/index"
printf '%s\n' "x = ${token}" > leak.txt
GIT_INDEX_FILE="${work_dir}/idx-b/index" git add -- leak.txt
if GIT_INDEX_FILE="${work_dir}/idx-b/index" git diff --cached --name-only HEAD | grep -qxF "leak.txt"; then
  pass "secret-scan: the swap target index really contains the leak (precondition)"
else
  fail "secret-scan: the swap target index really contains the leak (precondition)" "leak.txt not staged in idx-b"
fi
ln -sfn "${work_dir}/idx-a" "${work_dir}/idx-link"
GIT_INDEX_FILE="${work_dir}/idx-link/index" check_scan "an index reached through a symlinked parent directory is scanned when the link is not swapped" 0
for swap_to in idx-b idx-c; do
  ln -sfn "${work_dir}/idx-a" "${work_dir}/idx-link"
  rm -f "${work_dir}/deleted"
  rc=0
  DELETE_INDEX_ON_ARG=-U0 DELETE_INDEX_PATH="${work_dir}/idx-link" DELETE_INDEX_SYMLINK_TO="${work_dir}/${swap_to}" DELETE_INDEX_FLAG="${work_dir}/deleted" \
    GIT_INDEX_FILE="${work_dir}/idx-link/index" PATH="${shim_dir}:${PATH}" LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
  if [ -e "${work_dir}/deleted" ] && [ "${rc}" -eq 2 ] \
    && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
    pass "secret-scan: a parent-directory symlink of the index swapped to ${swap_to} during the scan is a scan failure (2)"
  else
    fail "secret-scan: a parent-directory symlink of the index swapped to ${swap_to} during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done


# ---- scripts/secret-scan-ci.sh: 検査不能のとき、固定の診断文だけを公開ログへ通す（SECR-23・#6036）
# (a) 実物の検出器: 走査対象（別の checkout）の skills-lock.json が不正なら、原因の固定診断が出て終了状態は 2。
new_repo; stage skills-lock.json "not json"; git commit -q -m "chore: add"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] \
  && grep -qxF 'secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting' "${work_dir}/out" \
  && grep -qF 'scan could not be completed (exit 2)' "${work_dir}/out"; then
  pass "secret-scan-ci: an unreadable skills-lock.json on the target surfaces the fixed diagnostic (exit 2)"
else
  fail "secret-scan-ci: an unreadable skills-lock.json on the target surfaces the fixed diagnostic (exit 2)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
# (b)(c) 試験用の検出器: 許可リストの行だけが出て、許可外・近似・空白付きの行は出ない。終了状態 1 では診断を通さない。
ci_root="${work_dir}/ci-root"
mkdir -p "${ci_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${ci_root}/scripts/"
allowed_line='secret-scan: cannot read HEAD; aborting'
cat > "${ci_root}/scripts/hooks/secret-scan.sh" <<EOF
#!/usr/bin/env bash
echo '${allowed_line}'
echo 'secret-scan: canary-sentinel-path/x; aborting'
echo '${allowed_line} extra'
echo 'x ${allowed_line}'
echo ' ${allowed_line}'
exit "\${SCAN_STUB_RC:-2}"
EOF
chmod +x "${ci_root}/scripts/hooks/secret-scan.sh"
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${ci_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] \
  && [ "$(grep -cxF "${allowed_line}" "${work_dir}/out")" -eq 1 ] \
  && ! grep -qF 'canary-sentinel-path' "${work_dir}/out" \
  && ! grep -qF "${allowed_line} extra" "${work_dir}/out" \
  && ! grep -qF "x ${allowed_line}" "${work_dir}/out" \
  && ! grep -qxF " ${allowed_line}" "${work_dir}/out"; then
  pass "secret-scan-ci: only lines exactly matching the allowlist reach the log on exit 2"
else
  fail "secret-scan-ci: only lines exactly matching the allowlist reach the log on exit 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
SCAN_STUB_RC=1 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${ci_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ] && ! grep -qF "${allowed_line}" "${work_dir}/out"; then
  pass "secret-scan-ci: diagnostics are not passed through on exit 1"
else
  fail "secret-scan-ci: diagnostics are not passed through on exit 1" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# (d) 陰性対照（scripts/secret-scan-canary.sh）: 許可リストのファイルを作れない（検出器が同じ名前のディレクトリを作る）と、
# 開けない旨の文（パスを含む）を公開ログへ出さず、検出器の行も通さない（SECR-23。CI 入口の (c) と同じ形の確認）。
canary_root="${work_dir}/canary-root"
mkdir -p "${canary_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${canary_root}/scripts/"
cat > "${canary_root}/scripts/hooks/secret-scan.sh" <<EOF
#!/usr/bin/env bash
mkdir -p "\${TMPDIR}/allowed-diag"
echo '${allowed_line}'
exit 2
EOF
chmod +x "${canary_root}/scripts/hooks/secret-scan.sh"
git -c init.defaultBranch=main init -q "${canary_root}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${canary_root}/scripts/secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qF 'scan-canary: scanner could not complete (exit 2)' "${work_dir}/out" \
  && ! grep -qF 'allowed-diag' "${work_dir}/out" && ! grep -qiF 'Is a directory' "${work_dir}/out" \
  && ! grep -qF "${allowed_line}" "${work_dir}/out"; then
  pass "scan-canary: an unopenable allowlist file leaks no shell diagnostic and passes no detector line (exit 2)"
else
  fail "scan-canary: an unopenable allowlist file leaks no shell diagnostic and passes no detector line (exit 2)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# ---- 許可リスト（lib.sh の secret_scan_fixed_diagnostics）と検出器の固定の診断文の整合（#6145・#6036）
# 検出器が出す固定の文（"secret-scan: …; aborting" の形で、変数の展開 $ を含まないリテラル）がすべて許可リストにあり、
# 許可リストの各行が検出器の中にリテラルとして残っていること。動的な文（${what}・cmp exit・rc=・required command not found）は対象外。
# shellcheck source=../lib.sh
source "${HOOKS_DIR}/../lib.sh"
diag_allowed="${work_dir}/diag-allowed.txt"
diag_literals="${work_dir}/diag-literals.txt"
# check_diag_sync <検出器のパス>: 0 = 整合 / 1 = 食い違い / 2 = 抜き出しの失敗
check_diag_sync() {
  local det="$1" g_rc=0 line
  secret_scan_fixed_diagnostics > "${diag_allowed}" || return 2
  LC_ALL=C grep -ao '"secret-scan: [^$"]*; aborting"' "${det}" > "${diag_literals}" || g_rc=$?
  [ "${g_rc}" -le 1 ] || return 2
  [ -s "${diag_literals}" ] || return 2
  # 検出器のリテラルはすべて許可リストにある
  while IFS= read -r line; do
    line="${line#\"}"
    line="${line%\"}"
    grep -qxF -- "${line}" "${diag_allowed}" || return 1
  done < "${diag_literals}"
  # 許可リストの各行は検出器の中にリテラルとして現れる（古い行の残りを検出する）
  while IFS= read -r line; do
    LC_ALL=C grep -aqF -- "\"${line}\"" "${det}" || return 1
  done < "${diag_allowed}"
  return 0
}
rc=0
check_diag_sync "${SECRET_SCAN}" || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: the fixed diagnostics of the detector and the CI allowlist agree"
else
  fail "secret-scan: the fixed diagnostics of the detector and the CI allowlist agree" "check exit ${rc}"
fi
# 陰性対照: 許可リストに無い固定の文を足した複製は、照合に失敗する
diag_stub="${work_dir}/diag-stub.sh"
{ cat "${SECRET_SCAN}"; printf '%s\n' 'echo "secret-scan: an unlisted fixed diagnostic; aborting" >&4'; } > "${diag_stub}"
rc=0
check_diag_sync "${diag_stub}" || rc=$?
if [ "${rc}" -eq 1 ]; then
  pass "secret-scan: an unlisted fixed diagnostic in the detector is caught by the allowlist check"
else
  fail "secret-scan: an unlisted fixed diagnostic in the detector is caught by the allowlist check" "check exit ${rc}"
fi
# 陰性対照（逆向き）: 許可リストの先頭の 1 行のリテラルを検出器の複製から取り除くと、古い行の残りとして照合に失敗する（#6170）
diag_first="$(secret_scan_fixed_diagnostics | head -n 1)"
diag_stub2="${work_dir}/diag-stub2.sh"
LC_ALL=C grep -avF -- "\"${diag_first}\"" "${SECRET_SCAN}" > "${diag_stub2}" || true
rc=0
check_diag_sync "${diag_stub2}" || rc=$?
if [ -n "${diag_first}" ] && [ "${rc}" -eq 1 ]; then
  pass "secret-scan: an allowlist line missing from the detector is caught by the allowlist check"
else
  fail "secret-scan: an allowlist line missing from the detector is caught by the allowlist check" "check exit ${rc}"
fi

# ---- 除外の一覧の大きさと引数の長さ（#6053）
# 登録名ごとの exclude を並べると、有効な一覧（64 KiB 以内）でも引数が macOS の上限を超えて git を起動できなくなる。
# 実際の上限は CI の macOS で踏み、ここでは shim の MAX_GIT_ARG_BYTES で模す。
big_chars="0 1 2 3 4 5 6 7 8 9 a b c d e f g h i j k l m n o p q r s t u v w x y z"
big_body=""
big_n=0
for c1 in ${big_chars}; do
  big_body+="${q}${c1}${q}:{},"
  for c2 in ${big_chars}; do
    big_body+="${q}${c1}${c2}${q}:{},"
  done
done
for c1 in ${big_chars}; do
  for c2 in ${big_chars}; do
    for c3 in ${big_chars}; do
      [ "${#big_body}" -gt 64800 ] && break 3
      big_body+="${q}${c1}${c2}${c3}${q}:{},"
      big_n=$((big_n + 1))
    done
  done
done
lock_big="{${q}version${q}:2,${q}skills${q}:{${big_body%,}}}"
if [ "${#lock_big}" -le 65536 ] && [ "${big_n}" -gt 4000 ]; then
  pass "secret-scan: big lock fixture is within the size limit and has many names"
else
  fail "secret-scan: big lock fixture is within the size limit and has many names" "size ${#lock_big}, names ${big_n}"
fi
make_detector big "${lock_big}"
big_bin="${det_bin}"

big_repo() {
  new_repo
  printf '%s\n' "${lock_big}" > skills-lock.json
  git add -- skills-lock.json
  stage .agents/skills/aa/SKILL.md "x = ${token}"
  stage .agents/skills/zzzz/x.md "x = ${token}"
  stage src/app.txt "x = ${token}"
}
big_expect() {
  expect_out "$1: unregistered name under the skills dir is reported" "  .agents/skills/zzzz/x.md"
  expect_out "$1: outside file is reported" "  src/app.txt"
  expect_out "$1: registered name under the skills dir is excluded (2 findings)" "secret-scan: total: 2 finding(s) in 2 file(s)"
}
big_repo
PATH="${shim_dir}:${PATH}" MAX_GIT_ARG_BYTES=131072 SCAN_BIN="${big_bin}" check_scan "big lock (staged): arguments stay short" 1 src/app.txt
big_expect "big lock (staged)"
git commit -q -m "test: big lock"
SCAN_ARG=--tracked
PATH="${shim_dir}:${PATH}" MAX_GIT_ARG_BYTES=131072 SCAN_BIN="${big_bin}" check_scan "big lock (--tracked): arguments stay short" 1 src/app.txt
big_expect "big lock (--tracked)"
SCAN_ARG=

# 未登録の名前の下に大量の無害なパスと、先頭・末尾の token のファイルがあっても、git の呼び出しは固定回数で引数は短い
make_detector small "${lock_ok}"
small_bin="${det_bin}"
many_repo() {
  new_repo
  stage_lock
  stage .agents/skills/unreg/a-first.txt "x = ${token}"
  stage .agents/skills/unreg/z-last.txt "x = ${token}"
  local blob i
  blob="$(printf 'harmless\n' | git hash-object -w --stdin)"
  {
    for ((i = 0; i < 1500; i++)); do
      printf '100644 %s\t.agents/skills/unreg/f-%0100d.txt\n' "${blob}" "${i}"
    done
  } | git update-index --add --index-info
}
many_repo
PATH="${shim_dir}:${PATH}" MAX_GIT_ARG_BYTES=4096 SCAN_BIN="${small_bin}" check_scan "many paths keep the arguments short" 1 .agents/skills/unreg/z-last.txt
expect_out "many paths count each file once" "secret-scan: total: 2 finding(s) in 2 file(s)"
many_repo
FAIL_GIT_BOTH1=--name-only FAIL_GIT_BOTH2=. PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "a failing file listing is not scannable (exit 2)" 2
expect_out "a failing file listing reports its own message" "secret-scan: failed to list files; aborting"

# 一覧のファイル数と diff の区画の数が食い違えば、除外の対応が崩れているため検査不能（2。落とす区画があるときの照合）
new_repo; stage_lock
stage .agents/skills/vendored-a/x.md "harmless = 1"
stage src/app.txt "harmless = 2"
EXTRA_LIST_ENTRY=extra.txt PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "a section count that differs from the file listing is not scannable (exit 2)" 2
expect_out "a section count mismatch reports a normalization failure" "secret-scan: failed to normalize diff; aborting"
new_repo; stage_lock
stage .agents/skills/vendored-a/x.md "harmless = 1"
stage src/app.txt "harmless = 2"
PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "the same fixture without the extra listing entry passes (the filter is on the path)" 0

# TMPDIR に `\t` のようなエスケープに読める並びがあっても、一時ファイルの振り分け（登録名の除外と外した名前の区画の除外）が外れない
# （awk の `-v` で値を渡すと、gawk・mawk・busybox awk のどれも `\t` をタブへ書き換える。`\d` のような未知の並びは mawk が書き換えない）。
# 外した名前（vendored-b。索引の一覧では外れ、検出器の一覧にはある）も含め、両方のファイル（SS_F1・SS_F2）を使う経路を通す。
bs_tmp="${work_dir}/tmp\\tdir"
mkdir -p "${bs_tmp}"
new_repo
printf '%s\n' '{"version":2,"skills":{"vendored-a":{"source":"x"}}}' > skills-lock.json
git add -- skills-lock.json
stage .agents/skills/vendored-a/x.md "x = ${token}"
stage .agents/skills/vendored-b/x.md "x = ${token}"
stage src/app.txt "x = ${token}"
TMPDIR="${bs_tmp}" SCAN_BIN="${small_bin}" check_scan "an escape-like TMPDIR keeps the exclusions" 1 src/app.txt
expect_out "an escape-like TMPDIR counts the outside and the dropped files once each" "secret-scan: total: 2 finding(s) in 2 file(s)"
expect_out "an escape-like TMPDIR reports the dropped name" "  .agents/skills/vendored-b/x.md"

# NUL の後ろに 64 KiB を超える本文があっても、NUL の確認（cmp が最初の差で読むのをやめ、tr が SIGPIPE になる）で
# 検査不能にならず、NUL を含む差分として両方の版を走査する（終了コード 1・内部エラーの出力なし）
new_repo
{
  printf 'pad\000\n'
  for ((i = 0; i < 6000; i++)); do
    printf 'harmless line number %08d padding padding\n' "${i}"
  done
  printf 'x = %s\n' "${token}"
} > nulbig.dat
git add nulbig.dat
if [ "$(wc -c < nulbig.dat)" -gt 131072 ]; then
  pass "secret-scan: the NUL fixture exceeds the pipe buffer after the NUL byte"
else
  fail "secret-scan: the NUL fixture exceeds the pipe buffer after the NUL byte" "size $(wc -c < nulbig.dat)"
fi
SCAN_BIN="${small_bin}" check_scan "a NUL followed by more than 64 KiB is scanned (exit 1)" 1 nulbig.dat
if [[ "${last_out}" == *"internal error"* ]]; then
  fail "secret-scan: a NUL followed by more than 64 KiB prints no internal error" "output: ${last_out}"
else
  pass "secret-scan: a NUL followed by more than 64 KiB prints no internal error"
fi

# diff.submodule=log が設定されていても、gitlink を含む index で区画の対応が崩れず走査できる（--submodule=short の明示）
new_repo; stage_lock
# 設定を有効にしたまま、登録名の下の除外（区画を落とす分岐）と未登録の名前の検出を同時に確かめる（登録名の区画が無いと除外の経路を通らない）
git update-index --add --cacheinfo 160000,1111111111111111111111111111111111111111,docs/spec
stage .agents/skills/vendored-a/x.md "x = ${token}"
stage .agents/skills/unreg/x.md "x = ${token}"
git config diff.submodule log
SCAN_BIN="${small_bin}" check_scan "diff.submodule=log keeps the registered exclusion and the unregistered detection" 1 .agents/skills/unreg/x.md
expect_out "diff.submodule=log counts only the unregistered path" "secret-scan: total: 1 finding(s) in 1 file(s)"
git config --unset diff.submodule

# 登録名の下の変更と gitlink が同じ diff にあっても、登録名の下だけが落ち、未登録の名前は走査される（区画の対応と突き合わせの経路）
# diff.orderFile で区画の順序を変えても、パスの突き合わせで対応が保たれる
new_repo; stage_lock
git update-index --add --cacheinfo 160000,1111111111111111111111111111111111111111,docs/spec
stage .agents/skills/vendored-a/x.md "x = ${token}"
stage .agents/skills/unreg/x.md "x = ${token}"
printf '%s\n' 'docs/*' '.agents/skills/unreg/*' '.agents/skills/vendored-a/*' > "${PWD}/order.txt"
git config diff.orderFile "${PWD}/order.txt"
SCAN_BIN="${small_bin}" check_scan "registered path is excluded and unregistered path is scanned beside a gitlink" 1 .agents/skills/unreg/x.md
expect_out "only the unregistered path is counted" "secret-scan: total: 1 finding(s) in 1 file(s)"
git config --unset diff.orderFile

# 環境に SKSET_* が残っていても、未登録の名前は除外されない
new_repo; stage_lock
stage .agents/skills/unreg/x.md "x = ${token}"
SKSET_unreg=1 SKSET_aa=1 SCAN_BIN="${small_bin}" check_scan "an inherited SKSET_ variable does not exclude an unregistered name" 1 .agents/skills/unreg/x.md
expect_out "the inherited variable leaves the finding in place" "secret-scan: total: 1 finding(s) in 1 file(s)"

# 区画の形が紛らわしいパスは除外せず走査する
new_repo; stage_lock
stage ".agents/skills/vendored-a vendored-b/x.md" "x = ${token}"
stage ".agents/skills/vendored-*/x.md" "x = ${token}"
stage ".agents/skills/vendored-[ab]/x.md" "x = ${token}"
stage ".agents/skills/vendored-a/x.md" "x = ${token}"
nl_blob="$(printf 'x = %s\n' "${token}" | git hash-object -w --stdin)"
printf '100644 %s 0\t.agents/skills/vendored-a\n/x.md\0' "${nl_blob}" | git update-index --add -z --index-info
SCAN_BIN="${small_bin}" check_scan "odd path segments are scanned, not excluded" 1
expect_out "all four odd paths are counted" "secret-scan: total: 4 finding(s) in 4 file(s)"
new_repo; stage_lock; stage .agents/skills "x = ${token}"
SCAN_BIN="${small_bin}" check_scan "a file named like the skills dir is scanned" 1 .agents/skills

# git の起動回数（scan_diff）は、登録名の下のパスを含むとき（除外のフィルタが動くとき）も、ファイルの数に依らず一定
count_repo() {
  local n="$1" blob i
  new_repo
  stage_lock
  stage .agents/skills/vendored-a/x.md "x = ${token}"
  stage src/app.txt "x = ${token}"
  blob="$(printf 'harmless\n' | git hash-object -w --stdin)"
  {
    for ((i = 0; i < n; i++)); do
      printf '100644 %s\t.agents/skills/unreg/f-%d.txt\n' "${blob}" "${i}"
    done
  } | git update-index --add --index-info
}
count_log="${work_dir}/git-diff-count.log"
count_repo 3
rm -f -- "${count_log}"
COUNT_GIT_LOG="${count_log}" PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "few files with a registered path (counting git diff calls)" 1 src/app.txt
expect_out "the registered path is excluded in the counted run" "secret-scan: total: 1 finding(s) in 1 file(s)"
count_few="$(wc -l < "${count_log}" | tr -d ' ')"
count_repo 800
rm -f -- "${count_log}"
COUNT_GIT_LOG="${count_log}" PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "many files with a registered path (counting git diff calls)" 1 src/app.txt
expect_out "the registered path is excluded in the many-files run" "secret-scan: total: 1 finding(s) in 1 file(s)"
count_many="$(wc -l < "${count_log}" | tr -d ' ')"
rm -f -- "${count_log}"
if [ "${count_few}" -ge 1 ] && [ "${count_few}" -eq "${count_many}" ]; then
  pass "secret-scan: git diff call count does not depend on the number of files (${count_few})"
else
  fail "secret-scan: git diff call count does not depend on the number of files" "few ${count_few}, many ${count_many}"
fi

# 非 ASCII のパスの登録名の下の区画は、core.quotePath が true でも false でも除外される（一覧は生のパス、ヘッダは引用されない）
for qp in true false; do
  new_repo; stage_lock
  git config core.quotePath "${qp}"
  stage ".agents/skills/vendored-a/caf$(printf '\303\251').md" "x = ${token}"
  SCAN_BIN="${small_bin}" check_scan "non-ASCII path under a registered name is excluded (core.quotePath=${qp})" 0
  stage ".agents/skills/unreg/caf$(printf '\303\251').md" "x = ${token}"
  SCAN_BIN="${small_bin}" check_scan "non-ASCII path under an unregistered name is scanned (core.quotePath=${qp})" 1
  expect_out "only the unregistered non-ASCII path is counted (core.quotePath=${qp})" "secret-scan: total: 1 finding(s) in 1 file(s)"
done

# 引用されるパス（制御文字・DEL・二重引用符・バックスラッシュ。#6144）: 登録名の下は除外、未登録の下は走査する。
# 一覧の生のパスから git と同じ規則で引用したヘッダを組み、区画のヘッダ全体と突き合わせる（引用形は照合にだけ使う）
quoted_bytes="1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 34 92 127"
quoted_n=0
# stage_quoted <名前>: 引用が要る 1 バイトごとに 1 ファイルを index へ置く（件数は quoted_n）
stage_quoted() {
  local name="$1" b blob ch
  quoted_n=0
  blob="$(printf 'x = %s\n' "${token}" | git hash-object -w --stdin)"
  for b in ${quoted_bytes}; do
    printf -v ch '%b' "\\0$(printf '%03o' "${b}")"
    printf '100644 %s 0\t.agents/skills/%s/a%sb.md\0' "${blob}" "${name}" "${ch}" | git update-index --add -z --index-info
    quoted_n=$((quoted_n + 1))
  done
}
for qp in true false; do
  for scan_arg in "" --tracked; do
    new_repo; stage_lock
    git config core.quotePath "${qp}"
    stage_quoted vendored-a
    if [ -n "${scan_arg}" ]; then git commit -q -m "test: quoted"; fi
    SCAN_ARG="${scan_arg}" SCAN_BIN="${small_bin}" check_scan "quoted paths under a registered name are excluded (core.quotePath=${qp} ${scan_arg})" 0
    stage_quoted unreg
    if [ -n "${scan_arg}" ]; then git commit -q -m "test: quoted 2"; fi
    SCAN_ARG="${scan_arg}" SCAN_BIN="${small_bin}" check_scan "quoted paths under an unregistered name are scanned (core.quotePath=${qp} ${scan_arg})" 1
    expect_out "only the unregistered quoted paths are counted (core.quotePath=${qp} ${scan_arg})" "secret-scan: total: ${quoted_n} finding(s) in ${quoted_n} file(s)"
  done
done
SCAN_ARG=

# 同じ名前の未登録の兄弟（二重引用符）は 1 件だけ数えられる
new_repo; stage_lock
stage '.agents/skills/vendored-a/q"x.md' "x = ${token}"
SCAN_BIN="${small_bin}" check_scan "a quoted path under a registered name is excluded" 0
stage '.agents/skills/unreg/q"x.md' "x = ${token}"
SCAN_BIN="${small_bin}" check_scan "a quoted path under an unregistered sibling is scanned" 1
expect_out "only the unregistered quoted path is counted" "secret-scan: total: 1 finding(s) in 1 file(s)"

# 非 ASCII と二重引用符・DEL・タブ・改行を含むパスも、登録名の下は除外・未登録の下は走査
for qp in true false; do
  new_repo; stage_lock
  git config core.quotePath "${qp}"
  blob="$(printf 'x = %s\n' "${token}" | git hash-object -w --stdin)"
  mixed_names=("caf$(printf '\303\251')\"z.md" "a$(printf '\177')b.md" "a$(printf '\t')b.md" "a"$'\n'"b.md")
  for nm in "${mixed_names[@]}"; do
    printf '100644 %s 0\t.agents/skills/vendored-a/%s\0' "${blob}" "${nm}" | git update-index --add -z --index-info
  done
  SCAN_BIN="${small_bin}" check_scan "mixed quoted paths under a registered name are excluded (core.quotePath=${qp})" 0
  for nm in "${mixed_names[@]}"; do
    printf '100644 %s 0\t.agents/skills/unreg/%s\0' "${blob}" "${nm}" | git update-index --add -z --index-info
  done
  SCAN_BIN="${small_bin}" check_scan "mixed quoted paths under an unregistered name are scanned (core.quotePath=${qp})" 1
  expect_out "only the four unregistered paths are counted (core.quotePath=${qp})" "secret-scan: total: 4 finding(s) in 4 file(s)"
done

# diff.orderFile は --name-only（一覧）にも効くため、一覧と区画の並びは揃ったままで、順序の変更だけでは対応は崩れない。
# 並びを入れ替えても、未登録の区画は走査される。
new_repo; stage_lock
stage '.agents/skills/vendored-a/q"x.md' "x = ${token}"
stage '.agents/skills/unreg/q"x.md' "x = ${token}"
printf '%s\n' '.agents/skills/unreg/*' '.agents/skills/vendored-a/*' > "${PWD}/order.txt"
git config diff.orderFile "${PWD}/order.txt"
SCAN_BIN="${small_bin}" check_scan "reordered quoted sections: the unregistered one is still scanned" 1
expect_out "reordered quoted sections: only the unregistered path is counted" "secret-scan: total: 1 finding(s) in 1 file(s)"
git config --unset diff.orderFile

# 一覧と区画を実際にずらす（shim が -U0 の呼び出しにだけ区画の並びを変える）。一覧では未登録が先、区画では登録名が先になり、
# 一覧の 2 番目（登録名の印）が区画の 2 番目（未登録）に当たる。ヘッダの一致の歯止めが無いと、未登録の区画が落ちて 1 件になる。
# 歯止めがあれば、一致しない区画は落とさず、両方が走査される（2 件）。
new_repo; stage_lock
stage '.agents/skills/vendored-a/q"x.md' "x = ${token}"
stage '.agents/skills/unreg/q"x.md' "x = ${token}"
printf '%s\n' '.agents/skills/vendored-a/*' '.agents/skills/unreg/*' > "${work_dir}/order-u0.txt"
PATH="${shim_dir}:${PATH}" ORDER_U0_FILE="${work_dir}/order-u0.txt" SCAN_BIN="${small_bin}" check_scan "misaligned sections: the unregistered section at a registered position is scanned" 1
expect_out "misaligned sections: header mismatch scans both sections" "secret-scan: total: 2 finding(s) in 2 file(s)"

# ---- 主の差分はパイプで流す（#6143）
# 除外する区画の本文を、除外の前に一時ファイルへ全量で書かない（--tracked の一時領域の最大を増やさない）。
mk_val="marker-$$-${RANDOM}"
tmp_obs="${work_dir}/obs-tmp"
mkdir -p "${tmp_obs}"
marker_log="${work_dir}/marker.log"
marker_repo() {
  new_repo; stage_lock
  stage .agents/skills/vendored-a/x.md "note = ${mk_val}"
  stage src/app.txt "harmless = 1"
  [ -z "${1:-}" ] || stage .agents/skills/unreg/y.md "note = ${mk_val}"
}
marker_run() {
  rm -f -- "${marker_log}"
  TMPDIR="${tmp_obs}" TMP_MARKER="${mk_val}" TMP_MARKER_DIR="${tmp_obs}" TMP_MARKER_LOG="${marker_log}" \
    PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "$1" 0
}
marker_repo
marker_run "excluded body is not written to a temp file (staged)"
if [ ! -s "${marker_log}" ]; then pass "secret-scan: no temp file holds the excluded body (staged)"; else fail "secret-scan: no temp file holds the excluded body (staged)" "marker observed"; fi
marker_repo unreg
marker_run "unregistered body (control, staged)"
if [ -s "${marker_log}" ]; then pass "secret-scan: the observation sees a scanned body (control)"; else fail "secret-scan: the observation sees a scanned body (control)" "marker not observed"; fi
marker_repo
git commit -q -m "test: marker"
SCAN_ARG=--tracked
marker_run "excluded body is not written to a temp file (--tracked)"
if [ ! -s "${marker_log}" ]; then pass "secret-scan: no temp file holds the excluded body (--tracked)"; else fail "secret-scan: no temp file holds the excluded body (--tracked)" "marker observed"; fi
SCAN_ARG=

# 各パイプの git の失敗は 2（1 = NUL の数え・2 = 除去版・3 = 置換版。置換版は NUL があるときだけ呼ぶ）
nth_counter="${work_dir}/nth.count"
nth_run() {
  rm -f -- "${nth_counter}"
  FAIL_GIT_NTH_ARG=-U0 FAIL_GIT_NTH="$2" FAIL_GIT_NTH_COUNTER="${nth_counter}" PATH="${shim_dir}:${PATH}" SCAN_BIN="${nth_bin:-${small_bin}}" check_scan "$1" 2
  if [[ "${last_out}" == *"internal error"* ]]; then fail "secret-scan: $1 prints no internal error" "output: ${last_out}"; else pass "secret-scan: $1 prints no internal error"; fi
}
marker_repo
nth_run "a failing NUL count is not scannable (exit 2)" 1
expect_out "a failing NUL count reports a diff read failure" "secret-scan: failed to read diff; aborting"
nth_run "a failing normalization (filtered) is not scannable (exit 2)" 2
expect_out "a failing normalization reports its own message" "secret-scan: failed to normalize diff; aborting"
make_detector nofilter
nofilter_bin="${det_bin}"
new_repo; stage src/app.txt "harmless = 1"
nth_bin="${nofilter_bin}"
nth_run "a failing normalization (no filter) is not scannable (exit 2)" 2
nth_bin=""
expect_out "a failing unfiltered normalization reports its own message" "secret-scan: failed to normalize diff; aborting"
new_repo; stage src/nul.dat "x"
printf 'a\000b\n' > src/nul.dat
git add src/nul.dat
nth_run "a failing space-replaced pass is not scannable (exit 2)" 3
expect_out "a failing space-replaced pass reports its own message" "secret-scan: failed to normalize diff; aborting"

# NUL が無ければ置換版の git を呼ばない（回数が 1 だけ違う）
new_repo; stage src/app.txt "harmless = 1"
rm -f -- "${count_log}"
COUNT_GIT_LOG="${count_log}" PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "plain diff (counting git diff calls)" 0
count_plain="$(wc -l < "${count_log}" | tr -d ' ')"
git add src/nul.dat 2>/dev/null || true
printf 'a\000b\n' > src/nul.dat
git add src/nul.dat
rm -f -- "${count_log}"
COUNT_GIT_LOG="${count_log}" PATH="${shim_dir}:${PATH}" SCAN_BIN="${small_bin}" check_scan "NUL diff (counting git diff calls)" 0
count_nul="$(wc -l < "${count_log}" | tr -d ' ')"
rm -f -- "${count_log}"
if [ "$((count_nul - count_plain))" -eq 1 ]; then
  pass "secret-scan: the space-replaced pass runs only when a NUL exists (${count_plain} -> ${count_nul})"
else
  fail "secret-scan: the space-replaced pass runs only when a NUL exists" "plain ${count_plain}, nul ${count_nul}"
fi

finish "part 5"
