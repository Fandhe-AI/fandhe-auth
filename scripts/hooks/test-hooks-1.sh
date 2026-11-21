#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 1（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

print_header

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

# 移動元は検出器と走査対象の index の両方に登録した除外の下に置き、除外の境界をまたぐ移動にする（#6011）
new_repo; stage_lock; mkdir -p .agents/skills/vendored-a; stage .agents/skills/vendored-a/doc.md "x = ${token}"
git commit -q -m "chore: vendored"; mkdir -p src; git mv .agents/skills/vendored-a/doc.md src/doc.md
SCAN_BIN="${det_ok}" check_scan "file moved out of an excluded path" 1 src/doc.md
# 除外の無い場所どうしの移動も、移動先の全行を走査する（--no-renames。リネームの検出で本文が出なくならない）
new_repo; stage old/doc.md "x = ${token}"
git commit -q -m "chore: add"; mkdir -p src; git mv old/doc.md src/doc.md
check_scan "file renamed between scanned paths" 1 src/doc.md

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

# コピー元は検出器と走査対象の index の両方に登録した除外の下に置き、除外の境界をまたぐコピーにする（#6011）
new_repo; git config diff.renames copies; stage_lock; mkdir -p .agents/skills/vendored-a
stage .agents/skills/vendored-a/doc.md "x = ${token}"; git commit -q -m "chore: vendored"
cp .agents/skills/vendored-a/doc.md copy.md; git add copy.md
SCAN_BIN="${det_ok}" check_scan "(d) diff.renames=copies does not hide a copied file" 1 copy.md

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

new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"; stage .agents/skills/vendored-b/sub/n.md "${dsn_line}"
SCAN_BIN="${det_ok}" check_scan "paths registered in the detector's skills-lock.json are excluded" 0

# 走査対象の index で登録が消えた名前は、検出器の一覧に残っていても除外しない（共通部分だけを除外に使う。#6000）
new_repo; stage skills-lock.json '{"version":2,"skills":{"vendored-b":{"source":"x"}}}'
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "a registration removed in the target index no longer excludes the directory" 1 .agents/skills/vendored-a/SKILL.md
new_repo; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "a target index without skills-lock.json excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# git add -N（intent-to-add）の skills-lock.json は未登録として扱い、無関係な変更を止めない。実際にステージした空ファイルは拒否する（#6000）
new_repo; stage src/app.txt "hello"; : > skills-lock.json; git add -N skills-lock.json
SCAN_BIN="${det_ok}" check_scan "an intent-to-add skills-lock.json is treated as unregistered" 0
# intent-to-add の判定は固定した index だけで行い、作業ツリーに依らない（add -N の後に作業ツリーから消しても未登録のまま。#6011）
new_repo; stage src/app.txt "hello"; : > skills-lock.json; git add -N skills-lock.json; rm skills-lock.json
SCAN_BIN="${det_ok}" check_scan "an intent-to-add skills-lock.json removed from the working tree is treated as unregistered" 0
# 判定の出力の形を環境に依らせない（GIT_PRINT_SHA1_ELLIPSIS=yes は省略した oid の後に ... を付ける。--no-abbrev で固定する。#6011）
new_repo; stage src/app.txt "hello"; : > skills-lock.json; git add -N skills-lock.json
GIT_PRINT_SHA1_ELLIPSIS=yes SCAN_BIN="${det_ok}" check_scan "an intent-to-add skills-lock.json is treated as unregistered with GIT_PRINT_SHA1_ELLIPSIS=yes" 0
new_repo; stage src/app.txt "hello"; : > skills-lock.json; git add skills-lock.json; rm skills-lock.json
SCAN_BIN="${det_ok}" check_scan "a really staged empty skills-lock.json removed from the working tree is still rejected" 2
new_repo; stage src/app.txt "hello"; : > skills-lock.json; git add skills-lock.json
SCAN_BIN="${det_ok}" check_scan "a really staged empty skills-lock.json is still rejected" 2

# 除外は検出器の skills-lock.json に登録された .agents/skills/<名前>/ の下だけ。パスの形では決めない（#6000）
new_repo; stage_lock; stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "unregistered .agents/skills directory is scanned" 1 .agents/skills/other/SKILL.md

new_repo; stage_lock; stage .agents/skills/vendored-a-evil/x.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "name prefix match does not exclude a sibling directory" 1 .agents/skills/vendored-a-evil/x.md

new_repo; stage_lock; stage .agents/skills/vendored-a "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "a file named exactly like a registered skill is scanned" 1 .agents/skills/vendored-a

new_repo; stage_lock; stage .agents/skills/loose.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "a file directly under .agents/skills is scanned" 1 .agents/skills/loose.md

new_repo; stage_lock; stage .agents/skills/VENDORED-A/x.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "case-changed skill name is scanned" 1 .agents/skills/VENDORED-A/x.md
GIT_ICASE_PATHSPECS=1 SCAN_BIN="${det_ok}" check_scan "case-changed skill name is scanned (GIT_ICASE_PATHSPECS=1)" 1 .agents/skills/VENDORED-A/x.md

new_repo; stage_lock; stage nested/.agents/skills/vendored-a/x.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "nested .agents/skills path is scanned" 1 nested/.agents/skills/vendored-a/x.md

new_repo; stage deps.lock "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "*.lock at the root is scanned" 1 deps.lock

new_repo; stage a/b/foo.lock "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "nested *.lock is scanned" 1 a/b/foo.lock

new_repo; stage .claude/skills/y/SKILL.md "${dsn_line}"
SCAN_BIN="${det_ok}" check_scan ".claude/skills real directory is scanned" 1 .claude/skills/y/SKILL.md

# 走査対象の repo が自分の skills-lock.json に名前を足しても、除外にならない（中心のケース）
new_repo
printf '%s\n' '{' '  "version": 2,' '  "skills": {' '    "other": {' '      "source": "x"' '    }' '  }' '}' > skills-lock.json
git add skills-lock.json; stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${det_ok}" check_scan "the scanned repo's own skills-lock.json does not add exclusions" 1 .agents/skills/other/SKILL.md

# 走査対象の一覧は除外に使わないが、壊れていれば検査不能（2）にしてマージ前に見つける（#6000）
new_repo
printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
git add skills-lock.json; stage src/app.txt "hello"
SCAN_BIN="${det_ok}" check_scan "the scanned repo's broken skills-lock.json is rejected (not adopted)" 2

new_repo
printf '%s\n' '{ "version": 3, "skills": { "other": { "source": "x" } } }' > skills-lock.json
git add skills-lock.json; stage src/app.txt "hello"
SCAN_BIN="${det_ok}" check_scan "the scanned repo's skills-lock.json with an unsupported version is rejected" 2

new_repo; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_nolock}" check_scan "detector without skills-lock.json excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 除外にかからないパスのダミーは検出される（陰性対照。#6000 の受け入れ条件）
new_repo; stage src/app.txt "${dsn_line}"
SCAN_BIN="${det_ok}" check_scan "dummy under a non-excluded path is detected" 1 src/app.txt

check_bad_lock "skills-lock.json with version 3 is rejected" '{
  "version": 3,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json without skills is rejected" '{
  "version": 2
}'
check_bad_lock "skills-lock.json with an empty skills map is rejected" '{
  "version": 2,
  "skills": {
  }
}'
check_bad_lock "skills-lock.json name with a slash is rejected" '{
  "version": 2,
  "skills": {
    "a/b": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json name with dot-dot is rejected" '{
  "version": 2,
  "skills": {
    "..": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json name with a glob character is rejected" '{
  "version": 2,
  "skills": {
    "a*": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json name with a space is rejected" '{
  "version": 2,
  "skills": {
    "a b": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with unbalanced braces is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }'
check_bad_lock "skills-lock.json with a duplicate name is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    },
    "vendored-a": {
      "source": "y"
    }
  }
}'
check_bad_lock "skills-lock.json with an unexpected top-level key is rejected" '{
  "version": 2,
  "extra": 1,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'


# ---------------------------------------------------------------- shellcheck-staged.sh（pre-commit の shellcheck の並列実行）
# 本物の shellcheck には頼らず（macOS の CI にあるとは限らない）、PATH 先頭に置いた偽物で呼び方と集約を確かめる。
# 偽物は呼び出しを 1 行ずつ SC_LOG へ記録する。名前に bad を含むファイルは 3 行を出して 1 で終わり、slow を含むものは遅れて終わる。
sc_shim_dir="${work_dir}/sc-shim"
mkdir -p "${sc_shim_dir}"
cat > "${sc_shim_dir}/shellcheck" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "${SC_LOG}"
for arg in "$@"; do
  case "${arg}" in
    *slow*) sleep 0.3 ;;
  esac
  case "${arg}" in
    *bad*)
      echo "${arg}: begin"
      echo "${arg}: middle"
      echo "${arg}: end"
      exit 1
      ;;
  esac
done
exit 0
EOF
chmod +x "${sc_shim_dir}/shellcheck"
SC_STAGED="${HOOKS_DIR}/shellcheck-staged.sh"
sc_log="${work_dir}/sc.log"
# sc_run <引数...>: 偽の shellcheck を先頭に置いて入口を起動する。終了コードは sc_rc、出力は ${work_dir}/out、呼び出しの記録は sc_log。
sc_run() {
  : > "${sc_log}"
  if SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" "$@" > "${work_dir}/out" 2>&1; then
    sc_rc=0
  else
    sc_rc=$?
  fi
}

sc_run
if [ "${sc_rc}" -eq 0 ] && [ ! -s "${sc_log}" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: no arguments exits 0 without starting shellcheck"
else
  fail "shellcheck-staged: no arguments exits 0 without starting shellcheck" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

sc_run a.sh b.sh c.sh
if [ "${sc_rc}" -eq 0 ] && [ "$(sort "${sc_log}")" = "$(printf '%s\n' '-- a.sh' '-- b.sh' '-- c.sh')" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: every file passing exits 0 and each file is checked once"
else
  fail "shellcheck-staged: every file passing exits 0 and each file is checked once" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 失敗したファイルがあれば非 0。ほかのファイルも検査され、出力はファイル単位でまとまり、引数の順に並ぶ
sc_run a.sh slow-bad1.sh c.sh bad2.sh
sc_want="$(printf '%s\n' 'slow-bad1.sh: begin' 'slow-bad1.sh: middle' 'slow-bad1.sh: end' 'bad2.sh: begin' 'bad2.sh: middle' 'bad2.sh: end')"
if [ "${sc_rc}" -eq 1 ] && [ "$(cat "${work_dir}/out")" = "${sc_want}" ] && [ "$(wc -l < "${sc_log}" | tr -d ' ')" = "4" ]; then
  pass "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per file in argument order"
else
  fail "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per file in argument order" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 1 件だけの検査で失敗しても非 0（失敗するファイルが 1 件だけの場合）
sc_run a.sh only-bad.sh
if [ "${sc_rc}" -eq 1 ]; then
  pass "shellcheck-staged: exactly one failing file among passing ones exits 1"
else
  fail "shellcheck-staged: exactly one failing file among passing ones exits 1" "exit ${sc_rc}"
fi

# 空白を含むファイル名は 1 つの引数のまま渡る（空の引数で起動しない）
sc_run "dir with space/my file.sh" "-dash.sh"
if [ "${sc_rc}" -eq 0 ] && [ "$(sort "${sc_log}")" = "$(printf '%s\n' '-- -dash.sh' '-- dir with space/my file.sh')" ]; then
  pass "shellcheck-staged: file names with spaces or a leading dash are passed intact after --"
else
  fail "shellcheck-staged: file names with spaces or a leading dash are passed intact after --" "exit ${sc_rc}, calls: $(cat "${sc_log}")"
fi

# 件数が並列数の上限を超えても、全件が 1 回ずつ検査される
sc_many=()
for sc_i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  sc_many+=("f${sc_i}.sh")
done
sc_run "${sc_many[@]}"
if [ "${sc_rc}" -eq 0 ] && [ "$(wc -l < "${sc_log}" | tr -d ' ')" = "30" ] && [ "$(sort -u "${sc_log}" | wc -l | tr -d ' ')" = "30" ]; then
  pass "shellcheck-staged: more files than the parallel limit are all checked once"
else
  fail "shellcheck-staged: more files than the parallel limit are all checked once" "exit ${sc_rc}, calls: $(wc -l < "${sc_log}")"
fi

# PATH に shellcheck が無いときは、未検査で成功にせず 2 で止まる
sc_empty_path="${work_dir}/sc-empty-path"
mkdir -p "${sc_empty_path}"
if PATH="${sc_empty_path}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
if [ "${sc_rc}" -eq 2 ] && grep -qF "shellcheck not found" "${work_dir}/out"; then
  pass "shellcheck-staged: a missing shellcheck exits 2"
else
  fail "shellcheck-staged: a missing shellcheck exits 2" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

finish "part 1"
