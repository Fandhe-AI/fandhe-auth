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
# 偽物は呼び出しを 1 行ずつ、呼び出しごとの別ファイル（${SC_LOG}.d/ の下）へ記録する。呼び出しは call と arg:<値>（1 引数 1 行）で記録する。呼ばれた引数の全件を順に見て、名前に bad を含むファイルは 3 行を出し（終了コード 1）、err を含むものは 1 行を出して（終了コード 2。shellcheck 自身の失敗。1 より優先）、slow を含むものは遅れて終わり、die を含むものは呼び出し元（結果を書く子の bash）を落とし、hang を含むものは自分の pid を SC_PIDFILE へ書いて 30 秒眠る。
sc_shim_dir="${work_dir}/sc-shim"
mkdir -p "${sc_shim_dir}"
cat > "${sc_shim_dir}/shellcheck" <<'EOF'
#!/bin/sh
# 1 回の呼び出しを「call」の行と、引数 1 つにつき 1 行の「arg:<値>」で記録する（空白を含む引数の境界を見分けるため）。
# 並列の呼び出しの記録が混ざらないよう（共有の 1 ファイルへの追記は macOS で行の途中で混ざった。#6076）、呼び出しごとに別のファイルへ書く。
# 名前は pid と連番で、noclobber の排他的な作成で確保する（pid が再利用されても既存の記録を上書きしない）。
# 確保できない・書けないときは、記録の欠落を成功と取り違えないよう 99 で落とす。
# 確保した後の書き込みは >| で行う（環境から受け継いだ noclobber でも、予約済みのファイルへ書けるように。#6063）。
# 記録先が渡っていないときは、記録せずに通る経路を作らないよう 99 で落とす。
[ -n "${SC_LOG:-}" ] || exit 99
sc_n=0
while :; do
  sc_f="${SC_LOG}.d/$$.${sc_n}"
  if (set -C; : > "${sc_f}") 2> /dev/null; then
    break
  fi
  sc_n=$((sc_n + 1))
  if [ "${sc_n}" -gt 1000 ]; then
    exit 99
  fi
done
{ printf 'call\n' && printf 'arg:%s\n' "$@"; } >| "${sc_f}" || exit 99
rc=0
for arg in "$@"; do
  case "${arg}" in
    *slow*) sleep 0.3 ;;
    *die*) kill -9 "${PPID}" ;;
    *hang*)
      echo "$$" >| "${SC_PIDFILE}"
      exec sleep 30
      ;;
    *err*)
      echo "${arg}: could not be read"
      rc=2
      ;;
  esac
  case "${arg}" in
    *bad*)
      echo "${arg}: begin"
      echo "${arg}: middle"
      echo "${arg}: end"
      if [ "${rc}" -eq 0 ]; then
        rc=1
      fi
      ;;
  esac
done
exit "${rc}"
EOF
# 並列数（バッチ数の上限）を固定するため、getconf も偽物にする（SC_JOBS を返す）
cat > "${sc_shim_dir}/getconf" <<'EOF'
#!/bin/sh
echo "${SC_JOBS:-8}"
EOF
chmod +x "${sc_shim_dir}/getconf"
chmod +x "${sc_shim_dir}/shellcheck"
SC_STAGED="${HOOKS_DIR}/shellcheck-staged.sh"
sc_log="${work_dir}/sc.log"
sc_jobs=8
# 記録は呼び出しごとのファイル（${sc_log}.d/）に書かれる。sc_reset で空に作り直し、入口の終了後に sc_collect で sc_log へ連結する
# （1 呼び出し = 1 ファイルなので、連結しても各呼び出しの記録は途中で混ざらない）。読むのは入口が終わった後だけ。
# 連結後の sc_log が空なら呼び出しは 0 件。連結の失敗は 0 件と取り違えないよう止める。
sc_reset() {
  rm -rf "${sc_log}.d"
  mkdir "${sc_log}.d"
  : > "${sc_log}"
}
sc_collect() {
  # 記録の置き場が読めない・辿れないときは、0 件と取り違えず 2 で止める。予約だけで中身の無い記録（偽物の書き込み途中の失敗）も同じ
  if [ ! -d "${sc_log}.d" ] || [ ! -r "${sc_log}.d" ] || [ ! -x "${sc_log}.d" ]; then
    echo "test-hooks: cannot read the shellcheck call records" >&2
    exit 2
  fi
  : > "${sc_log}"
  for sc_rec in "${sc_log}.d"/*; do
    if [ -e "${sc_rec}" ]; then
      if [ ! -s "${sc_rec}" ]; then
        echo "test-hooks: empty shellcheck call record" >&2
        exit 2
      fi
      cat "${sc_rec}" >> "${sc_log}" || exit 2
    fi
  done
}
# sc_run <引数...>: 偽の shellcheck を先頭に置いて入口を起動する。終了コードは sc_rc、出力は ${work_dir}/out、呼び出しの記録は sc_log。
sc_run() {
  sc_reset
  if SC_JOBS="${sc_jobs}" SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" "$@" > "${work_dir}/out" 2>&1; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_collect
}

# 記録の読み方: sc_ncalls = 呼び出しの回数、sc_files = 渡された引数（-- を除く）を 1 行 1 つで並べ替えたもの、
# sc_groups = 呼び出しごとの引数を | でつないだ行を並べ替えたもの（引数の境界が行の境界に対応する）。
sc_ncalls() { grep -c '^call$' "${sc_log}" || true; }
sc_dashes() { grep -c '^arg:--$' "${sc_log}" || true; }
sc_files() { grep '^arg:' "${sc_log}" | sed 's/^arg://' | grep -vx -e '--' | sort || true; }
sc_groups() {
  awk '/^call$/ { if (g != "") print g; g = ""; next } { sub(/^arg:/, ""); if ($0 == "--") next; g = (g == "" ? $0 : g "|" $0) } END { if (g != "") print g }' "${sc_log}" | sort
}

sc_run
if [ "${sc_rc}" -eq 0 ] && [ ! -s "${sc_log}" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: no arguments exits 0 without starting shellcheck"
else
  fail "shellcheck-staged: no arguments exits 0 without starting shellcheck" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

sc_run a.sh b.sh c.sh
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_files)" = "$(printf '%s\n' a.sh b.sh c.sh)" ] && [ "$(sc_ncalls)" = "$(sc_dashes)" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: every file passing exits 0 and each file is checked once"
else
  fail "shellcheck-staged: every file passing exits 0 and each file is checked once" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 失敗したファイルがあれば非 0。ほかのファイルも検査され、出力はファイル単位でまとまり、引数の順に並ぶ
sc_run a.sh slow-bad1.sh c.sh bad2.sh
sc_want="$(printf '%s\n' 'slow-bad1.sh: begin' 'slow-bad1.sh: middle' 'slow-bad1.sh: end' 'bad2.sh: begin' 'bad2.sh: middle' 'bad2.sh: end')"
if [ "${sc_rc}" -eq 1 ] && [ "$(cat "${work_dir}/out")" = "${sc_want}" ] && [ "$(sc_ncalls)" = "4" ]; then
  pass "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per batch in batch order"
else
  fail "shellcheck-staged: a failing file exits 1, the others are still checked and output is grouped per batch in batch order" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
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
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "2" ] && [ "$(sc_dashes)" = "2" ] && [ "$(sc_files)" = "$(printf '%s\n' '-dash.sh' 'dir with space/my file.sh')" ]; then
  pass "shellcheck-staged: file names with spaces or a leading dash are passed intact after --"
else
  fail "shellcheck-staged: file names with spaces or a leading dash are passed intact after --" "exit ${sc_rc}, calls: $(cat "${sc_log}")"
fi

# 同じ名前の引数（lefthook は glob が 2 つあると両方に一致するファイルを 2 回渡す）は 1 回だけ検査する
sc_run a.sh b.sh a.sh "x y.sh" "x y.sh" b.sh
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "3" ] && [ "$(sc_files)" = "$(printf '%s\n' a.sh b.sh 'x y.sh')" ]; then
  pass "shellcheck-staged: duplicate arguments are checked once"
else
  fail "shellcheck-staged: duplicate arguments are checked once" "exit ${sc_rc}, calls: $(cat "${sc_log}")"
fi

# 検査が結果を書く前に落ちたとき（子の bash が kill される）は、成功にせず 2（検査不能）で止まり、ファイル名つきの診断を出す
sc_run a.sh die.sh c.sh
if [ "${sc_rc}" -eq 2 ] && grep -qF "no valid result for die.sh" "${work_dir}/out"; then
  pass "shellcheck-staged: a check that dies before writing its result exits 2 and names the file"
else
  fail "shellcheck-staged: a check that dies before writing its result exits 2 and names the file" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 空のファイル名（名前が子に渡らない形）は検査を始めず、成功にもしない
sc_run ""
if [ "${sc_rc}" -eq 2 ] && [ ! -s "${sc_log}" ] && grep -qF "no valid result for" "${work_dir}/out"; then
  pass "shellcheck-staged: an empty file name is never reported as a pass"
else
  fail "shellcheck-staged: an empty file name is never reported as a pass" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 検査の道具自身の失敗（shellcheck が 2）は検査不能で、指摘（1）より優先して 2 で終わる。指摘だけのときは 1 のまま
sc_run bad1.sh err1.sh bad2.sh a.sh
if [ "${sc_rc}" -eq 2 ] && grep -qF "could not check err1.sh (exit 2)" "${work_dir}/out" && grep -qF "bad1.sh: begin" "${work_dir}/out"; then
  pass "shellcheck-staged: one unchecked file among findings exits 2 (unchecked wins over findings)"
else
  fail "shellcheck-staged: one unchecked file among findings exits 2 (unchecked wins over findings)" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 一時領域を作れない（TMPDIR が存在しない）ときは、指摘ありの 1 ではなく検査不能の 2 で止まる
sc_reset
if SC_LOG="${sc_log}" TMPDIR="${work_dir}/no-such-dir" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
if [ "${sc_rc}" -eq 2 ] && grep -qF "cannot create a temporary directory" "${work_dir}/out"; then
  pass "shellcheck-staged: a missing TMPDIR exits 2"
else
  fail "shellcheck-staged: a missing TMPDIR exits 2" "exit ${sc_rc}" "output: $(cat "${work_dir}/out")"
fi

# 件数が並列数の上限を超えても、shellcheck の起動はバッチ数（上限）以下で、全件がちょうど 1 回ずつ渡る（起動の回数をファイル数に比例させない）
sc_many=()
for sc_i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  sc_many+=("f${sc_i}.sh")
done
sc_jobs=4
sc_run "${sc_many[@]}"
sc_calls="$(sc_ncalls)"
sc_args="$(sc_files)"
sc_want_args="$(printf '%s\n' "${sc_many[@]}" | sort)"
if [ "${sc_rc}" -eq 0 ] && [ "${sc_calls}" -ge 1 ] && [ "${sc_calls}" -le 4 ] && [ "${sc_args}" = "${sc_want_args}" ]; then
  pass "shellcheck-staged: 30 files with 4 jobs start shellcheck at most 4 times and pass every file exactly once"
else
  fail "shellcheck-staged: 30 files with 4 jobs start shellcheck at most 4 times and pass every file exactly once" "exit ${sc_rc}, calls: ${sc_calls}" "args: $(printf '%s' "${sc_args}" | tr '\n' ' ')"
fi
sc_jobs=8

# 大きいファイルから順に、合計の小さいバッチへ入れる（貪欲法）。大きさは wc -c の 1 回の起動で取り、空白を含む名前も対応が崩れない。
# 大きさ 1000・600・500・400 を 2 バッチに分けると、{1000, 400} と {600, 500} になる。
sc_dir="${work_dir}/sc-sizes"
mkdir -p "${sc_dir}"
cd "${sc_dir}"
head -c 1000 /dev/zero > big.sh
head -c 600 /dev/zero > "mid one.sh"
head -c 500 /dev/zero > mid2.sh
head -c 400 /dev/zero > small.sh
sc_jobs=2
sc_run small.sh "mid one.sh" mid2.sh big.sh
sc_want="$(printf '%s\n' 'big.sh|small.sh' 'mid one.sh|mid2.sh' | sort)"
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_groups)" = "${sc_want}" ]; then
  pass "shellcheck-staged: files are split into batches of similar total size (largest first)"
else
  fail "shellcheck-staged: files are split into batches of similar total size (largest first)" "exit ${sc_rc}" "calls: $(cat "${sc_log}")"
fi
sc_jobs=8
cd "${work_dir}"

# getconf が桁あふれする数字の列を返しても、比較が誤って 0 のまま素通りしない。固定値（4）に落ちて全件が検査され、0 で終わる
# （期待は「正常に検査されて 0」。2 で止まるのではなく、検査の範囲が変わらないことを確かめる）
sc_jobs=99999999999999999999
sc_run a.sh b.sh c.sh d.sh e.sh
sc_args="$(sc_files)"
sc_calls="$(sc_ncalls)"
if [ "${sc_rc}" -eq 0 ] && [ "${sc_args}" = "$(printf '%s\n' a.sh b.sh c.sh d.sh e.sh)" ] && [ "${sc_calls}" -le 4 ]; then
  pass "shellcheck-staged: an overflowing getconf value falls back to a fixed parallel limit and every file is checked"
else
  fail "shellcheck-staged: an overflowing getconf value falls back to a fixed parallel limit and every file is checked" "exit ${sc_rc}, calls: ${sc_calls}" "output: $(cat "${work_dir}/out")"
fi
sc_jobs=8

# 出力先（番号付きの .out）を開けないときは、bash が shellcheck を起動せずに返す 1 を「指摘あり」にせず、検査不能の 2 にする。
# 偽の mktemp が、1 番目のバッチの出力先を一時領域の中の存在しない相対パスへのリンクにした一時領域を返す（開けず、親の読み出しにも引っかからない）
sc_mk_dir="${work_dir}/sc-mk-shim"
mkdir -p "${sc_mk_dir}"
cat > "${sc_mk_dir}/mktemp" <<'EOF'
#!/bin/sh
d="$("${SC_REAL_MKTEMP}" "$@")" || exit 1
ln -s no-such-dir/out "${d}/0.out"
printf '%s\n' "${d}"
EOF
chmod +x "${sc_mk_dir}/mktemp"
sc_real_mktemp="$(command -v mktemp)"
sc_reset
if SC_REAL_MKTEMP="${sc_real_mktemp}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_mk_dir}:${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
sc_collect
if [ "${sc_rc}" -eq 2 ] && [ ! -s "${sc_log}" ] && grep -qF "cannot open the output file of batch 0; a.sh was not checked" "${work_dir}/out" && ! grep -qF "shellcheck could not check a.sh" "${work_dir}/out"; then
  pass "shellcheck-staged: an unopenable output file exits 2 (not treated as a finding)"
else
  fail "shellcheck-staged: an unopenable output file exits 2 (not treated as a finding)" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 受け継いだ noclobber（SHELLOPTS）でも、出力先は 1 回だけ開くので shellcheck は起動され、0 で終わる。
# 先に作ってから開き直す実装では、2 回目に開けず bash が shellcheck を起動せずに 1 を返す（#6063）。
sc_reset
if env SHELLOPTS=noclobber SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" a.sh > "${work_dir}/out" 2>&1; then
  sc_rc=0
else
  sc_rc=$?
fi
sc_collect
if [ "${sc_rc}" -eq 0 ] && [ "$(sc_ncalls)" = "1" ] && [ ! -s "${work_dir}/out" ]; then
  pass "shellcheck-staged: an inherited noclobber still opens the output file once and starts shellcheck"
else
  fail "shellcheck-staged: an inherited noclobber still opens the output file once and starts shellcheck" "exit ${sc_rc}, calls: $(cat "${sc_log}")" "output: $(cat "${work_dir}/out")"
fi

# 後始末の kill -9 は、pid が偽物の shellcheck（exec した sleep）のままのときだけ送る（pid の再利用で無関係のプロセスを落とさない）。
sc_is_fake_sleep() {
  sc_comm="$(ps -p "$1" -o comm= 2> /dev/null)" || return 1
  sc_comm="${sc_comm#"${sc_comm%%[![:space:]]*}"}"
  sc_comm="${sc_comm%"${sc_comm##*[![:space:]]}"}"
  case "${sc_comm}" in
    sleep | */sleep) return 0 ;;
    *) return 1 ;;
  esac
}

# 中断（TERM・HUP）では、起動済みのバッチの shellcheck まで止め、2 で終わる（止めずに孫の shellcheck を残さない）。
# 偽の shellcheck が pid を書いて眠っている間に親へ信号を送り、終わった後にその pid のプロセスが残っていないことを確かめる
# INT は試験しない（背景で起動したシェルでは INT が無視された状態で始まり、trap できない）。
# HUP を trap できないシェルでは、30 秒待たずに注記を出して飛ばす。
for sc_sig in TERM HUP; do
  if [ "${sc_sig}" = "HUP" ] && [ "$("${HOOK_BASH}" -c 'trap "exit 7" HUP; kill -HUP $$; exit 0' > /dev/null 2>&1; echo $?)" != "7" ]; then
    echo "NOTE: skipping the HUP stop test (HUP cannot be trapped in this shell)"
    continue
  fi
  sc_pidfile="${work_dir}/sc.pid"
  rm -f "${sc_pidfile}"
  sc_reset
  SC_PIDFILE="${sc_pidfile}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh a.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_waited=0
  while [ ! -s "${sc_pidfile}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sc_t0="${SECONDS}"
  kill -"${sc_sig}" "${sc_script_pid}" 2> /dev/null || true
  if wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_stop_secs=$((SECONDS - sc_t0))
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  sc_alive=1
  if [ -n "${sc_fake_pid}" ]; then
    sc_waited=0
    while kill -0 "${sc_fake_pid}" 2> /dev/null && sc_is_fake_sleep "${sc_fake_pid}" && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_fake_pid}" 2> /dev/null && sc_is_fake_sleep "${sc_fake_pid}"; then
      kill -9 "${sc_fake_pid}" 2> /dev/null || true
    else
      sc_alive=0
    fi
  fi
  # 偽物は 30 秒眠るため、止まるまでに 10 秒以上かかったなら、停止が伝わらず眠りの終わりを待っただけ
  if [ "${sc_rc}" -eq 2 ] && [ -n "${sc_fake_pid}" ] && [ "${sc_alive}" -eq 0 ] && [ "${sc_stop_secs}" -lt 10 ]; then
    pass "shellcheck-staged: a ${sc_sig} stops the running shellcheck and exits 2"
  else
    fail "shellcheck-staged: a ${sc_sig} stops the running shellcheck and exits 2" "exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_alive}, seconds to stop: ${sc_stop_secs}" "output: $(cat "${work_dir}/out")"
  fi
done

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
