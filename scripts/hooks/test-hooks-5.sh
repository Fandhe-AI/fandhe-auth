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


finish "part 5"
