#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テスト（CI の test-hooks ジョブ・`make test-hooks`）。
#
# 一時ディレクトリに使い捨ての git リポジトリを作り、secret-scan.sh の検出・許可ケースと
# commit-msg-check.sh の OK / NG ケースを実際のスクリプトで検証する。
# 基本の外部依存は bash と git。msrv・install-msrv の節は cargo・jq・tomllib を持つ python3（3.11 以上）も要し、
# 前提が足りなければ CI（CI か GITHUB_ACTIONS が設定されている）では失敗、ローカルでは skip する。
# crate-deps の節は cargo が無ければ skip する。フックではなく CI で実行する（.claude/rules/git-hooks.md）。
#
# 回帰ケースは過去に見つかった fail-open の不具合クラスに対応する:
#   (a) 検査エラーを「一致なし」と同一視する
#   (b) 非 UTF-8 の内容でロケール依存の照合が一致しなくなる
#   (c) hunk 内の `++ b/...` で始まる追加行を `+++ b/` ヘッダと誤認する
#   (d) 利用者の git 設定（色・外部 diff・textconv・prefix）で diff の出力形式が変わる
#   (e) git の失敗がパイプ・here-string に隠れる
#
# フックは本スクリプトを実行している bash（${BASH}）で起動する。macOS 標準の bash 3.2 での
# 互換性は `/bin/bash scripts/hooks/test-hooks.sh` で確認できる（本スクリプトも 3.2 互換で書く）。
#
# 検出させる例示値はすべて実行時に組み立て、本ファイル自体のコミットが secret-scan に
# 検知されないようにする。
set -euo pipefail
HOOKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib.sh
source "${HOOKS_DIR}/../lib.sh"

reject_args "$(basename "$0")" "$@"

SECRET_SCAN="${HOOKS_DIR}/secret-scan.sh"
COMMIT_MSG_CHECK="${HOOKS_DIR}/commit-msg-check.sh"
HOOK_BASH="${BASH}"

# テストハーネス自身の文字列比較はバイト単位で行う。フックは下の UTF-8 ロケールで起動する。
export LC_ALL=C

# (b) の回帰を実際に再現させるため、フックは UTF-8 ロケールで起動する（フック側が内部で
# C ロケールへ切り替えることを検証する）。見つからない環境では検証にならないため失敗させる。
# （`locale -a | grep -q` は grep の早期終了で locale が SIGPIPE になり pipefail で偽になり得るため、
# 一覧を変数へ受けてから照合する）
available_locales="$(locale -a 2>/dev/null || true)"
utf8_locale=""
for candidate in C.UTF-8 C.utf8 en_US.UTF-8 en_US.utf8; do
  if [[ $'\n'"${available_locales}"$'\n' == *$'\n'"${candidate}"$'\n'* ]]; then
    utf8_locale="${candidate}"
    break
  fi
done
if [ -z "${utf8_locale}" ]; then
  echo "test-hooks: no UTF-8 locale available (need C.UTF-8 or en_US.UTF-8)" >&2
  exit 1
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/test-hooks.XXXXXX")"
trap 'rm -rf "${work_dir}"' EXIT

# 開発者・CI ランナーの git 設定（署名・色・フック・テンプレート）から隔離する。
# lefthook 経由で起動された場合に備え、親リポジトリを指す環境変数も外す。
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT
real_home="${HOME}"
export HOME="${work_dir}/home"
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=hook-test GIT_AUTHOR_EMAIL=hook-test@example.invalid
export GIT_COMMITTER_NAME=hook-test GIT_COMMITTER_EMAIL=hook-test@example.invalid
mkdir -p "${HOME}"

# (e) 用: 指定した引数を含む git 呼び出しだけを失敗させるシム。実体の git は先に解決しておく。
real_git="$(command -v git)"
shim_dir="${work_dir}/shim"
mkdir -p "${shim_dir}"
# コードは引用付きのヒアドキュメントで固定し、パスは環境変数で渡す（TMPDIR・PATH 由来の値をシェルのコードへ埋め込まない）。
export REAL_GIT="${real_git}"
cat > "${shim_dir}/git" <<'EOF'
#!/bin/sh
for arg in "$@"; do
  if [ "${arg}" = "${FAIL_GIT_ARG:-}" ]; then
    echo "git shim: forced failure on ${arg}" >&2
    exit 128
  fi
done
exec "${REAL_GIT}" "$@"
EOF
chmod +x "${shim_dir}/git"

# 例示値（実行時に組み立てる）
q='"'
at='@'
dash='-----'
token="gh""p_$(printf 'A1b2C3d4E5f6G7h8I9j0K1l2')"
password_value="Hunter2Hunter2Hunter2"
password_line="password = ${q}${password_value}${q}"
dsn_line="db: postgres://app:s3cr3tpass${at}db.internal:5432/app"
key_line="${dash}BEGIN OPENSSH PRIVATE"" KEY${dash}"
# 出力に値そのものが含まれていないことを確認する対象
leak_needles="${token}
${password_value}
s3cr3tpass"

last_out=""
passed=0
failed=0
repo_seq=0
repo=""

pass() {
  passed=$((passed + 1))
  echo "ok - $1"
}

fail() {
  failed=$((failed + 1))
  echo "not ok - $1"
  shift
  local detail
  for detail in ${@+"$@"}; do
    echo "    ${detail}"
  done
}

# new_repo: 空コミット 1 つを持つ使い捨てリポジトリを作り、cwd をそこへ移す。
new_repo() {
  repo_seq=$((repo_seq + 1))
  repo="${work_dir}/repo-${repo_seq}"
  mkdir -p "${repo}"
  cd "${repo}"
  git -c init.defaultBranch=main init -q
  git commit -q --allow-empty -m "chore: init"
}

# check_scan <説明> <期待する終了コード> [報告されるべきファイル名]
# cwd で secret-scan.sh を UTF-8 ロケールで実行し、終了コード・報告ファイル名・値の非出力を検証する。
check_scan() {
  local desc="$1" want="$2" want_file="${3:-}" rc=0 out needle
  # SCAN_ARG は空（staged モード）か `--tracked` など 1 語の引数
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" ${SCAN_ARG:+"${SCAN_ARG}"} > "${work_dir}/out" 2>&1 || rc=$?
  out="$(cat "${work_dir}/out")"
  last_out="${out}"
  if [ "${rc}" -ne "${want}" ]; then
    fail "secret-scan: ${desc}" "expected exit ${want}, got ${rc}" "output: ${out}"
    return 0
  fi
  if [ -n "${want_file}" ] && [[ "${out}" != *$'\n'"  ${want_file}"$'\n'* ]]; then
    fail "secret-scan: ${desc}" "expected report of file: ${want_file}" "output: ${out}"
    return 0
  fi
  while IFS= read -r needle; do
    if [[ "${out}" == *"${needle}"* ]]; then
      fail "secret-scan: ${desc}" "secret value leaked to output"
      return 0
    fi
  done <<< "${leak_needles}"
  pass "secret-scan: ${desc}"
}

# expect_out <説明> <直前の check_scan の出力に含まれるべき文字列>（件数の行の確認）
expect_out() {
  if [[ "${last_out}" == *"$2"* ]]; then
    pass "secret-scan: $1"
  else
    fail "secret-scan: $1" "expected output to contain: $2" "output: ${last_out}"
  fi
}

# stage <path> <内容（printf の書式ではなくそのまま書き込む）>
stage() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" > "$1"
  git add -- "$1"
}

echo "# hooks under test run with: ${HOOK_BASH} (${BASH_VERSION}), locale ${utf8_locale}"

# ---------------------------------------------------------------- secret-scan: 検出
new_repo; stage src/app.txt "x = ${token}"
check_scan "vendor token in new file" 1 src/app.txt

new_repo; stage config.yml "${password_line}"
check_scan "quoted password assignment" 1 config.yml

new_repo; stage config.yml "API_KEY: ${q}abcdefghijklmnop1234${q}"
check_scan "api key assignment (case-insensitive)" 1 config.yml

new_repo; stage config.yml "${dsn_line}"
check_scan "connection string with credentials" 1 config.yml

new_repo; stage id.txt "${key_line}"
check_scan "private key header" 1 id.txt

new_repo; stage notes.txt "first"; git commit -q -m "chore: add"
printf '%s\n%s\n' "first" "x = ${token}" > notes.txt; git add notes.txt
check_scan "token added to existing file" 1 notes.txt

new_repo; stage .env "A=1"
check_scan ".env staged" 1 .env

new_repo; stage config/.env.local "A=1"
check_scan ".env.local in subdirectory staged" 1 config/.env.local

new_repo; stage "日本語/.env" "A=1"
check_scan ".env under non-ASCII directory (core.quotePath)" 1 "日本語/.env"

new_repo; stage "dir with space/app.txt" "x = ${token}"
check_scan "file name with spaces is reported correctly" 1 "dir with space/app.txt"

new_repo; stage src/app.txt "x = ${token}"; mkdir -p sub/dir; cd sub/dir
check_scan "run from a subdirectory scans the whole index" 1 src/app.txt

new_repo; mkdir -p .agents/skills/x; stage .agents/skills/x/doc.md "x = ${token}"
git commit -q -m "chore: vendored"; mkdir -p src; git mv .agents/skills/x/doc.md src/doc.md
check_scan "file moved out of an excluded path" 1 src/doc.md

# (b) 非 UTF-8 のバイトと同じ行にあるトークン
new_repo; stage latin1.txt "caf"$'\xe9'" ${token}"
check_scan "(b) Latin-1 byte on the same line" 1 latin1.txt

new_repo; stage bytes.txt $'\xff\xfe'" ${token}"
check_scan "(b) invalid UTF-8 prefix on the same line" 1 bytes.txt

# (c) 追加行の本文が `++` で始まる（diff 上は `+++...` になる）
new_repo; stage app.txt "++ b/other.txt ${token}"
check_scan "(c) added line starting with '++ b/' is scanned" 1 app.txt

new_repo; stage app.txt "++${token}"
check_scan "(c) added line starting with '++' is scanned" 1 app.txt

new_repo; printf '%s\n%s\n' "++ b/decoy.txt" "x = ${token}" > app.txt; git add app.txt
check_scan "(c) '++ b/' line in a hunk does not change the reported file" 1 app.txt

# (d) 利用者の git 設定
new_repo; git config color.diff always; stage app.txt "x = ${token}"
check_scan "(d) color.diff=always" 1 app.txt

new_repo; git config color.ui always; stage app.txt "x = ${token}"
check_scan "(d) color.ui=always" 1 app.txt

new_repo; git config diff.external true; stage app.txt "x = ${token}"
check_scan "(d) diff.external" 1 app.txt

new_repo; printf '%s\n' '*.txt diff=hide' > .gitattributes; git config diff.hide.textconv 'sh -c :'
stage app.txt "x = ${token}"
check_scan "(d) textconv driver" 1 app.txt

new_repo; git config diff.noprefix true; stage app.txt "x = ${token}"
check_scan "(d) diff.noprefix keeps file attribution" 1 app.txt

new_repo; git config diff.mnemonicPrefix true; stage app.txt "x = ${token}"
check_scan "(d) diff.mnemonicPrefix keeps file attribution" 1 app.txt

new_repo; git config diff.renames copies; mkdir -p .claude/skills/x
stage .claude/skills/x/doc.md "x = ${token}"; git commit -q -m "chore: vendored"
cp .claude/skills/x/doc.md copy.md; git add copy.md
check_scan "(d) diff.renames=copies does not hide a copied file" 1 copy.md

# ---------------------------------------------------------------- secret-scan: 許可
new_repo
check_scan "nothing staged" 0

new_repo; stage app.txt "hello"
check_scan "clean file" 0

new_repo; stage .env.example "A=changeme"; stage .env.sample "A=1"; stage .env.template "A=1"
check_scan ".env.example / .sample / .template" 0

new_repo
# shellcheck disable=SC2016 # ${DB_PASSWORD} は展開させない例示値
stage config.yml 'password = "${DB_PASSWORD}"'
check_scan "env var placeholder" 0

new_repo; stage README.md "see https://example.com/path and user${at}example.com"
check_scan "URL without credentials" 0

new_repo; stage .agents/skills/x/SKILL.md "x = ${token}"; stage .claude/skills/y/SKILL.md "${dsn_line}"
stage deps.lock "x = ${token}"
check_scan "excluded paths (.agents/skills, .claude/skills, *.lock)" 0

new_repo; stage leaked.txt "$(printf '%s\n%s' "keep" "x = ${token}")"; git commit -q -m "chore: leak"
printf '%s\n' "keep" > leaked.txt; git add leaked.txt
check_scan "removing a leaked secret is allowed" 0

new_repo; stage leaked.txt "x = ${token}"; git commit -q -m "chore: leak"; git rm -q leaked.txt
check_scan "deleting a file with a secret is allowed" 0

# ---- 規則表（TASK-6）: 規則ごとの検出・件数の出力
jwt_value="eyJ""hbGciOiJFUzI1NiJ9.eyJ""zdWIiOiJ1c2VyMSJ9.abcdefghij"
jwk_value="Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij"
recovery_value="Abcd1234-Efgh5678-Ijkl9012"
enc_key_line="${dash}BEGIN ENCRYPTED PRIVATE"" KEY${dash}"
leak_needles="${leak_needles}
${jwt_value}
${jwk_value}
${recovery_value}"

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

new_repo; stage .agents/skills/x/SKILL.md "x = ${token}"; git commit -q -m "chore: skill"
check_scan "(tracked) excluded paths" 0

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
CI_ENTRY="${HOOKS_DIR}/../secret-scan-ci.sh"
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
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "scan-canary: passes without a grep error"
else
  fail "scan-canary: passes without a grep error" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# ---- 終了状態の丸めの回帰（0 と既知の 1 以外は検査不能 = 2。--summary は固定の文言だけ）
# secret-scan-ci.sh: 環境変数の列挙（compgen）の失敗を「該当なし」と取り違えない
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
compgen_fail_env="${work_dir}/compgen-fail.sh"
printf 'compgen() { return 2; }\n' > "${compgen_fail_env}"
rc=0
BASH_ENV="${compgen_fail_env}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan-ci: failure listing environment variables exits 2"
else
  fail "secret-scan-ci: failure listing environment variables exits 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# secret-scan.sh: cmp のエラー（終了状態 2 以上）は検査不能
cmp_shim_dir="${work_dir}/cmp-shim"
mkdir -p "${cmp_shim_dir}"
printf '#!/bin/sh\nexit 2\n' > "${cmp_shim_dir}/cmp"
chmod +x "${cmp_shim_dir}/cmp"
rc=0
PATH="${cmp_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan: (tracked) cmp error exits 2"
else
  fail "secret-scan: (tracked) cmp error exits 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# secret-scan.sh --summary: シェルが出す外部コマンドの診断（mktemp 等）も捨て、固定の文言だけを出す
mktemp_shim_dir="${work_dir}/mktemp-shim"
mkdir -p "${mktemp_shim_dir}"
printf '#!/bin/sh\necho "RAW-DIAGNOSTIC-MARKER /raw/path" >&2\nexit 1\n' > "${mktemp_shim_dir}/mktemp"
chmod +x "${mktemp_shim_dir}/mktemp"
rc=0
PATH="${mktemp_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && ! grep -q "RAW-DIAGNOSTIC-MARKER" "${work_dir}/out" && grep -q "secret-scan: mktemp failed" "${work_dir}/out"; then
  pass "secret-scan: --summary drops raw diagnostics of external commands (mktemp)"
else
  fail "secret-scan: --summary drops raw diagnostics of external commands (mktemp)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
rc=0
PATH="${mktemp_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" --tracked > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -q "RAW-DIAGNOSTIC-MARKER" "${work_dir}/out"; then
  pass "secret-scan: normal mode keeps raw diagnostics of external commands (mktemp)"
else
  fail "secret-scan: normal mode keeps raw diagnostics of external commands (mktemp)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# secret-scan-canary.sh: 検出器が検査不能（2）を返したら、陰性対照も 2 で終わる（1 は検出能力の異常だけ）
rc=0
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=--name-only LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "scan-canary: scanner exit 2 makes the canary exit 2"
else
  fail "scan-canary: scanner exit 2 makes the canary exit 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# secret-scan-canary.sh: 元のリポジトリの hooks（post-index-change）を起動しない
hook_dir="${work_dir}/hooks"
mkdir -p "${hook_dir}"
# shellcheck disable=SC2016 # 展開させない固定のコード
printf '%s\n' '#!/bin/sh' 'touch "${HOOK_RAN_FILE}"' > "${hook_dir}/post-index-change"
chmod +x "${hook_dir}/post-index-change"
rm -f "${work_dir}/hook-ran"
# 呼び出し元の GIT_* は canary が外すため、グローバル設定（HOME の .gitconfig）で hooksPath を差し込む
printf '[core]\n\thooksPath = %s\n' "${hook_dir}" > "${HOME}/.gitconfig"
rc=0
HOOK_RAN_FILE="${work_dir}/hook-ran" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
rm -f "${HOME}/.gitconfig"
if [ "${rc}" -eq 0 ] && [ ! -e "${work_dir}/hook-ran" ]; then
  pass "scan-canary: does not run hooks of the original repository"
else
  fail "scan-canary: does not run hooks of the original repository" "exit ${rc}" "hook ran: $([ -e "${work_dir}/hook-ran" ] && echo yes || echo no)" "output: $(cat "${work_dir}/out")"
fi

# secret-scan-canary.sh: --summary の出力に許可した集計行以外の行（ファイル名等）があれば失敗（1）
extra_shim_dir="${work_dir}/extra-shim"
mkdir -p "${extra_shim_dir}"
cat > "${extra_shim_dir}/cmp" <<'EOF'
#!/bin/sh
echo "  some/extra/file.txt" >&4
exec "${REAL_CMP}" "$@"
EOF
chmod +x "${extra_shim_dir}/cmp"
rc=0
REAL_CMP="$(command -v cmp)" PATH="${extra_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ]; then
  pass "scan-canary: an extra line in the scanner summary fails the canary"
else
  fail "scan-canary: an extra line in the scanner summary fails the canary" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi


# ---- TASK-6 (4/10) 2 段目: CI 入口・陰性対照の堅牢化の回帰
# 1 つの結果を記録する補助（引数: 名前・期待する終了状態・実際の終了状態）
expect_rc() {
  local name="$1" want="$2" got="$3"
  if [ "${got}" -eq "${want}" ]; then
    pass "${name}"
  else
    fail "${name}" "exit ${got} (expected ${want})" "output: $(cat "${work_dir}/out")"
  fi
}

# 相対の TMPDIR でも、一時領域を絶対パスへ解決してから git に渡す（CI 入口・陰性対照とも）
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
rel_cwd="${work_dir}/rel-cwd"
mkdir -p "${rel_cwd}/reltmp"
rc=0
(cd "${rel_cwd}" && TMPDIR=reltmp LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}") > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: a relative TMPDIR is resolved to an absolute path" 0 "${rc}"
rc=0
(cd "${rel_cwd}" && TMPDIR=reltmp LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh") > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "scan-canary: a relative TMPDIR is resolved to an absolute path" 0 "${rc}"

# CI の secret-scan ジョブと同じ形の target（作業ツリーを展開せず、浅く取得したオブジェクトに HEAD を張っただけ）を走査する。
# 検出器は作業ツリーではなく HEAD の tree を読むこと（件数が追跡ファイル数と一致し、仕込んだ値を検出する）を確かめる。
# make_ci_target <ソース repo> <作る target のパス>: 別の空の repo から git init・fetch --depth=1・update-ref HEAD で作る。
make_ci_target() {
  local src="$1" dst="$2" sha
  sha="$(git -C "${src}" rev-parse HEAD)"
  rm -rf "${dst}"
  git -c init.defaultBranch=main init -q "${dst}"
  git -C "${dst}" fetch --quiet --no-tags --depth=1 "file://${src}" "${sha}" > /dev/null 2>&1
  git -C "${dst}" update-ref HEAD "${sha}"
}
new_repo; stage app.txt "hello"; stage sub/b.txt "world"; stage c.txt "plain"
# 取得元は到達可能な sha の取得だけを許す（GitHub と同じ条件）。fork の head の取得の互換性は本試験の範囲外
git config uploadpack.allowReachableSHA1InWant true
git commit -q -m "chore: files"
ci_src="${repo}"
tracked_count="$(git -C "${ci_src}" ls-files | wc -l | tr -d ' ')"
ci_target="${work_dir}/ci-target"
make_ci_target "${ci_src}" "${ci_target}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${ci_target}" > "${work_dir}/out" 2>&1 || rc=$?
want_line="secret-scan: total: 0 finding(s) (${tracked_count} tracked file(s) checked)"
if [ "${rc}" -eq 0 ] && [ -z "$(find "${ci_target}" -mindepth 1 -maxdepth 1 ! -name .git)" ] \
  && [ "$(grep -cxF -- "${want_line}" "${work_dir}/out" || true)" -eq 1 ] \
  && [ "$(grep -c '^secret-scan: total:' "${work_dir}/out" || true)" -eq 1 ]; then
  pass "secret-scan-ci: a target without a working tree is scanned from HEAD (clean, ${tracked_count} files checked)"
else
  fail "secret-scan-ci: a target without a working tree is scanned from HEAD (clean, ${tracked_count} files checked)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
# 秘密値の形のダミーを仕込んだコミットは検出（1）。作業ツリーが空でも HEAD の内容を走査する
stage leaked.txt "x = ${token}"
git commit -q -m "chore: leak"
make_ci_target "${ci_src}" "${ci_target}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${ci_target}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ] && grep -qxF -- "secret-scan: total: 1 finding(s) in 1 file(s)" "${work_dir}/out" \
  && ! grep -qF -- "${token}" "${work_dir}/out" && ! grep -qF -- "leaked.txt" "${work_dir}/out"; then
  pass "secret-scan-ci: a target without a working tree reports a planted dummy (exit 1, no value or path)"
else
  fail "secret-scan-ci: a target without a working tree reports a planted dummy (exit 1, no value or path)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 絶対パスへの解決（cd）が失敗しても、mktemp が作った一時領域を残さない（EXIT トラップが消せること）
cd_tmp_fail_env="${work_dir}/cd-tmp-fail.sh"
cat > "${cd_tmp_fail_env}" <<'EOF'
cd() {
  case "${1:-}" in
    */secret-scan-ci.* | */scan-canary.*) return 1 ;;
  esac
  builtin cd "$@"
}
EOF
leak_tmp="${work_dir}/leak-tmp"
rm -rf "${leak_tmp}"
mkdir -p "${leak_tmp}"
rc=0
TMPDIR="${leak_tmp}" BASH_ENV="${cd_tmp_fail_env}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${CI_ENTRY}" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && [ -z "$(ls -A "${leak_tmp}")" ] && grep -qF "secret-scan-ci: cannot resolve the temporary directory" "${work_dir}/out"; then
  pass "secret-scan-ci: a failure to resolve the temporary directory leaves no directory behind"
else
  fail "secret-scan-ci: a failure to resolve the temporary directory leaves no directory behind" "exit ${rc}" "left: $(ls -A "${leak_tmp}")" "output: $(cat "${work_dir}/out")"
fi
rm -rf "${leak_tmp}"
mkdir -p "${leak_tmp}"
rc=0
TMPDIR="${leak_tmp}" BASH_ENV="${cd_tmp_fail_env}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${HOOKS_DIR}/../secret-scan-canary.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && [ -z "$(ls -A "${leak_tmp}")" ] && grep -qF "scan-canary: cannot resolve the temporary directory" "${work_dir}/out"; then
  pass "scan-canary: a failure to resolve the temporary directory leaves no directory behind"
else
  fail "scan-canary: a failure to resolve the temporary directory leaves no directory behind" "exit ${rc}" "left: $(ls -A "${leak_tmp}")" "output: $(cat "${work_dir}/out")"
fi
rm -rf "${leak_tmp}"

# 検出器が 0 を返しても、走査完了の集計行（検出 0 件）がちょうど 1 行で、ほかの許可行が無いことを合格の条件にする。
# 検出器を差し替えた複製（scripts/ だけ）で、出力と終了状態を固定の検出器から返す。
stub_root="${work_dir}/stub-root"
mkdir -p "${stub_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${stub_root}/scripts/"
cat > "${stub_root}/scripts/hooks/secret-scan.sh" <<'EOF'
#!/bin/sh
if [ -n "${STUB_OUT:-}" ]; then
  printf '%s\n' "${STUB_OUT}"
fi
exit "${STUB_RC:-0}"
EOF
chmod +x "${stub_root}/scripts/hooks/secret-scan.sh"
git -C "${stub_root}" -c init.defaultBranch=main init -q
clean_line='secret-scan: total: 0 finding(s) (5 tracked file(s) checked)'
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
rc=0
STUB_OUT="" STUB_RC=0 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${stub_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: scanner exit 0 without a completion summary line exits 2" 2 "${rc}"
rc=0
STUB_OUT="${clean_line}"$'\n'"${clean_line}" STUB_RC=0 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${stub_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: scanner exit 0 with two completion summary lines exits 2" 2 "${rc}"
rc=0
STUB_OUT='secret-scan: total: 1 finding(s) (5 tracked file(s) checked)' STUB_RC=0 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${stub_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: scanner exit 0 with a non-zero finding count exits 2" 2 "${rc}"
rc=0
STUB_OUT="${clean_line}"$'\n''secret-scan: private-key: 1 line(s)' STUB_RC=0 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${stub_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: scanner exit 0 with a finding line exits 2" 2 "${rc}"
# 集計行がちょうど 1 行なら走査は完了とみなし、次の陰性対照へ進む（差し替えた検出器は陰性対照で 1 を返さないため 1 で止まる）
rc=0
STUB_OUT="${clean_line}" STUB_RC=0 LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${stub_root}/scripts/secret-scan-ci.sh" "${repo}" > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: exactly one completion summary line proceeds to the canary" 1 "${rc}"

# 外部コマンドが欠けていれば、起動前に検査不能（2）で止める。欠けた 1 つ以外だけを置いた PATH で起動する
run_without_cmd() {
  # 引数: 除く コマンド名・スクリプト・必要なコマンド（残り）。出力は ${work_dir}/out、終了状態は rc に入れる。
  local skip="$1" script="$2" c bin
  shift 2
  bin="${work_dir}/nobin-${skip}"
  rm -rf "${bin}"
  mkdir -p "${bin}"
  for c in ${@+"$@"}; do
    if [ "${c}" != "${skip}" ]; then
      ln -s "$(command -v "${c}")" "${bin}/${c}"
    fi
  done
  rc=0
  PATH="${bin}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${script}" ${run_arg+"${run_arg}"} > "${work_dir}/out" 2>&1 || rc=$?
}
new_repo; stage app.txt "hello"; git commit -q -m "chore: add"
run_arg="${repo}"
for missing in git mktemp grep rm; do
  run_without_cmd "${missing}" "${CI_ENTRY}" git mktemp grep rm
  if [ "${rc}" -eq 2 ] && grep -q "required command not found on PATH: ${missing}" "${work_dir}/out"; then
    pass "secret-scan-ci: missing ${missing} exits 2 before starting"
  else
    fail "secret-scan-ci: missing ${missing} exits 2 before starting" "exit ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done
unset run_arg
for missing in git mktemp mkdir rm sort cmp grep; do
  run_without_cmd "${missing}" "${HOOKS_DIR}/../secret-scan-canary.sh" git mktemp mkdir rm sort cmp grep
  # 依存の確認より前に、確認していない外部コマンド（basename 等）を呼ばない（command not found が出ないこと）
  if [ "${rc}" -eq 2 ] && grep -q "required command not found on PATH: ${missing}" "${work_dir}/out" \
    && ! grep -q "command not found$" "${work_dir}/out"; then
    pass "scan-canary: missing ${missing} exits 2 before starting"
  else
    fail "scan-canary: missing ${missing} exits 2 before starting" "exit ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done

# lib.sh のルートの解決に失敗したら、空の ROOT_DIR で先へ進まず終了コード 2 で止める
cd_up_fail_env="${work_dir}/cd-up-fail.sh"
cat > "${cd_up_fail_env}" <<'EOF'
cd() {
  case "${1:-}" in
    */..) return 1 ;;
  esac
  builtin cd "$@"
}
EOF
rc=0
# shellcheck disable=SC2016 # 子の bash に渡す固定のコード（展開させない）
BASH_ENV="${cd_up_fail_env}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" -c 'source "$1"; echo reached' _ "${HOOKS_DIR}/../lib.sh" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && ! grep -q '^reached$' "${work_dir}/out" && grep -qF "error: cannot resolve the repository root" "${work_dir}/out"; then
  pass "lib.sh: failure to resolve the repository root stops with exit 2"
else
  fail "lib.sh: failure to resolve the repository root stops with exit 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# lib.sh の repo_root は末尾の改行も保つ。パスが改行で終わるリポジトリに scripts/ の複製を置き、
# 引数なし（既定の対象 = ROOT_DIR）で CI 入口・陰性対照を起動する
nl_root="${work_dir}/nl-root-parent/repo"$'\n'
mkdir -p "${nl_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${nl_root}/scripts/"
cp "${SECRET_SCAN}" "${nl_root}/scripts/hooks/"
(
  cd "${nl_root}"
  git -c init.defaultBranch=main init -q
  printf 'hello\n' > app.txt
  git add app.txt
  git commit -q -m "chore: add"
)
rc=0
(cd "${work_dir}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${nl_root}/scripts/secret-scan-ci.sh") > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "secret-scan-ci: ROOT_DIR keeps a trailing newline (default target)" 0 "${rc}"
rc=0
(cd "${work_dir}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${nl_root}/scripts/secret-scan-canary.sh") > "${work_dir}/out" 2>&1 || rc=$?
expect_rc "scan-canary: ROOT_DIR keeps a trailing newline" 0 "${rc}"


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
new_repo; stage .env "A=1"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=--name-only check_scan "(e) failure listing staged files" 2

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

echo "# passed: ${passed}, failed: ${failed}"
if [ "${failed}" -ne 0 ]; then
  exit 1
fi
