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
# 並列の呼び出しの記録が混ざらないよう、共有の 1 ファイルへ追記せず呼び出しごとに別のファイルへ書く。
# #6076 では、共有の 1 ファイルへ追記した並列の呼び出しの記録が行の境界で入り混じった（macOS の /bin/sh は bash。観測した条件に限る）。printf が出力の各行を個別の write(2) で書くとは保証されず、1 回の printf が複数の書き込みに分かれ得るため、O_APPEND は 1 回の write(2) の中しか保護せず、呼び出し単位の記録をまとめては保護できない。
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
    *sigtrap*)
      # 受けた CONT・TERM を SC_SIGLOG へ記録する（sleep を exec しないので、信号を受けても生き続け、記録できる）。眠りは 60 秒で自分から終わる。
      echo "$$" >| "${SC_PIDFILE}"
      trap 'echo CONT >> "${SC_SIGLOG}"' CONT
      trap 'echo TERM >> "${SC_SIGLOG}"; exit 0' TERM
      sc_i=0
      while [ "${sc_i}" -lt 600 ]; do
        sleep 0.1
        sc_i=$((sc_i + 1))
      done
      exit 0
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

# pid が偽物の shellcheck（exec した sleep）になったかを見る（待ち合わせ専用。kill には使わない）。
sc_is_fake_sleep() {
  sc_comm="$(ps -p "$1" -o comm= 2> /dev/null)" || return 1
  sc_comm="${sc_comm#"${sc_comm%%[![:space:]]*}"}"
  sc_comm="${sc_comm%"${sc_comm##*[![:space:]]}"}"
  case "${sc_comm}" in
    sleep | */sleep) return 0 ;;
    *) return 1 ;;
  esac
}

# pid が試験の起動したスクリプト（$2）の子孫の偽物（sleep）であるときだけ、信号 $1 を $3 へ送る（pid の再利用で無関係のプロセスへ送らない。深さ 4 まで辿る）。
sc_signal_owned() {
  sc_so_pid="$3"
  sc_so_depth=0
  sc_is_fake_sleep "${sc_so_pid}" || return 1
  while [ "${sc_so_depth}" -lt 4 ]; do
    sc_so_pid="$(ps -p "${sc_so_pid}" -o ppid= 2> /dev/null | tr -d '[:space:]')" || return 1
    [ -n "${sc_so_pid}" ] || return 1
    if [ "${sc_so_pid}" = "$2" ]; then
      kill -"$1" "$3" 2> /dev/null
      return $?
    fi
    sc_so_depth=$((sc_so_depth + 1))
  done
  return 1
}

# sc_kill_job <信号> <pid>: この試験のシェルのジョブ表に、生きた（Running・Stopped）ジョブとして残る pid にだけ信号を送る。
# ジョブ表は 1 回の `jobs -l` で取る（bash が回収済みの pid は再利用されうるため、起動時に控えた pid へ無条件に送らない）。確かめられなければ送らず 1 を返す。
sc_kill_job() {
  # 状態名は bash が翻訳するため、関数の局所の LC_ALL=C で ASCII の名前にして照合する（git-hooks.md の (b)）
  local LC_ALL=C
  sc_kj_file="${work_dir}/kj-jobs"
  sc_kj_ok=1
  jobs -l >| "${sc_kj_file}" 2> /dev/null || return 1
  while read -r _kj_n sc_kj_pid sc_kj_state _kj_rest; do
    if [ "${sc_kj_pid}" = "$2" ]; then
      case "${sc_kj_state}" in
        Running | Stopped*) sc_kj_ok=0 ;;
      esac
    fi
  done < "${sc_kj_file}"
  rm -f "${sc_kj_file}"
  [ "${sc_kj_ok}" -eq 0 ] || return 1
  kill -"$1" "$2" 2> /dev/null
}

# sc_reap <pid>: 試験が起動したスクリプトを強制停止して回収する。消滅を確かめられたときだけ wait し（0）、
# 確かめられなければ wait せず 1 を返す（残り続ける対象を無期限に待たない）。既に回収済みで一覧に無い場合も、消滅していれば 0。
sc_reap() {
  sc_kj_note=""
  sc_kill_job KILL "$1" || true
  sc_rp_waited=0
  while kill -0 "$1" 2> /dev/null && [ "${sc_rp_waited}" -lt 50 ]; do
    sleep 0.1
    sc_rp_waited=$((sc_rp_waited + 1))
  done
  if kill -0 "$1" 2> /dev/null; then
    sc_kj_note="; could not confirm the forced stop of pid ${1} (not waiting)"
    return 1
  fi
  wait "$1" 2> /dev/null || true
  return 0
}

# sc_orphan_signal <信号> <pid> <起動時の開始時刻>: 孤児の偽物へ、送るたびに開始時刻の一致と sleep であることを確かめてから送る
# （CONT の後に終わって pid が再利用されても、TERM が別のプロセスへ届かない）。
sc_orphan_signal() {
  [ -n "$3" ] || return 1
  [ "$(ps -p "$2" -o lstart= 2> /dev/null || true)" = "$3" ] || return 1
  sc_is_fake_sleep "$2" || return 1
  kill -"$1" "$2" 2> /dev/null
}

# 中断（TERM・HUP）では、起動済みのバッチの shellcheck まで止め、2 で終わる（止めずに孫の shellcheck を残さない）。
# 偽の shellcheck が pid を書いて眠っている間に親へ信号を送り、終わった後にその pid のプロセスが残っていないことを確かめる
# INT は試験しない（背景で起動したシェルでは INT が無視された状態で始まり、trap できない）。
# HUP を trap できないシェルでは、30 秒待たずに注記を出して飛ばす。
for sc_sig in TERM HUP TERM-STOPPED; do
  sc_stop_child=0
  if [ "${sc_sig}" = "TERM-STOPPED" ]; then
    sc_sig=TERM
    sc_stop_child=1
    sc_label="a TERM with a stopped shellcheck"
  else
    sc_label="a ${sc_sig}"
  fi
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
  # 偽物が exec して sleep になるまで待ってから信号を送る（exec 前に送ると、停止の対象が sleep でなくシェルになり、確認が曖昧になる）
  # 起動・sleep への exec・SIGSTOP の送信・停止状態のどれかを確認できなければ、信号を送らず試験の失敗として扱う（退行を見逃さない）。
  sc_setup_err=""
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  if [ -z "${sc_fake_pid}" ]; then
    sc_setup_err="the fake shellcheck did not start"
  else
    sc_waited=0
    while ! sc_is_fake_sleep "${sc_fake_pid}" && [ "${sc_waited}" -lt 50 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if ! sc_is_fake_sleep "${sc_fake_pid}"; then
      sc_setup_err="the fake shellcheck did not become sleep"
    fi
  fi
  sc_fake_start=""
  if [ -z "${sc_setup_err}" ]; then
    sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
  fi
  if [ -z "${sc_setup_err}" ] && [ "${sc_stop_child}" -eq 1 ]; then
    # 一時停止中の子を持つ中断（停止中の子は CONT が無いと TERM を受け取れない）
    if ! sc_signal_owned STOP "${sc_script_pid}" "${sc_fake_pid}"; then
      sc_setup_err="SIGSTOP could not be sent to the fake shellcheck"
    else
      sc_waited=0
      sc_state=""
      while [ "${sc_waited}" -lt 30 ]; do
        sc_state="$(ps -p "${sc_fake_pid}" -o stat= 2> /dev/null || true)"
        sc_state="${sc_state#"${sc_state%%[![:space:]]*}"}"
        case "${sc_state}" in
          T*) break ;;
        esac
        sleep 0.1
        sc_waited=$((sc_waited + 1))
      done
      case "${sc_state}" in
        T*) ;;
        *) sc_setup_err="the fake shellcheck did not reach the stopped state" ;;
      esac
    fi
  fi
  if [ -n "${sc_setup_err}" ]; then
    # 管理下のプロセスだけを後始末する（偽物は自分で起動した子孫。眠りは 30 秒で終わる）
    if [ -n "${sc_fake_pid}" ]; then
      sc_signal_owned CONT "${sc_script_pid}" "${sc_fake_pid}" || true
    fi
    sc_reap "${sc_script_pid}" || true
    fail "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2" "setup failed: ${sc_setup_err}${sc_kj_note}" "output: $(cat "${work_dir}/out")"
    continue
  fi
  sc_t0="${SECONDS}"
  sc_kill_job "${sc_sig}" "${sc_script_pid}" || true
  # 期限（20 秒）付きで終了を待つ。無期限の wait にすると、停止が伝わらない退行で試験が止まる。
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sc_timed_out=0
  sc_kj_note=""
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_timed_out=1
    # 期限切れ: 試験が管理するプロセスだけを後始末して失敗にする
    sc_signal_owned CONT "${sc_script_pid}" "${sc_fake_pid}" || true
    sc_reap "${sc_script_pid}" || true
  fi
  if [ "${sc_timed_out}" -eq 1 ]; then
    # 回収は sc_reap が済ませた（確かめられなければ wait していない）
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_stop_secs=$((SECONDS - sc_t0))
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  # 停止の確認: 消滅（kill -0 の失敗）を確かめられたときだけ停止済みとする。ps の失敗・別名は停止の証拠にしない。
  # 強制終了は送らない（pid の再利用で無関係のプロセスを落とさない）。
  # 消滅の判定は kill -0 だけで行う（ps の失敗・名前の不一致を消滅と取り違えない。ps は起動時の待ち合わせにだけ使う）。
  sc_alive=1
  if [ -n "${sc_fake_pid}" ]; then
    sc_waited=0
    while kill -0 "${sc_fake_pid}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_fake_pid}" 2> /dev/null; then
      # 親が先に終わった孤児（SIGSTOP のまま残る等）は、起動時に控えた開始時刻と一致し、まだ sleep であるときだけ回収する
      # （pid の再利用で無関係のプロセスへ送らない）。ps で確かめられなければ送らず、偽物は 30 秒で自然に終わる。
      sc_now_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
      if [ -n "${sc_fake_start}" ] && [ "${sc_now_start}" = "${sc_fake_start}" ] && sc_is_fake_sleep "${sc_fake_pid}"; then
        sc_orphan_signal CONT "${sc_fake_pid}" "${sc_fake_start}" || true
        sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
      fi
    else
      sc_alive=0
    fi
  fi
  # 偽物は 30 秒眠るため、止まるまでに 10 秒以上かかったなら、停止が伝わらず眠りの終わりを待っただけ
  if [ "${sc_rc}" -eq 2 ] && [ -n "${sc_fake_pid}" ] && [ "${sc_alive}" -eq 0 ] && [ "${sc_stop_secs}" -lt 10 ]; then
    pass "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2"
  else
    fail "shellcheck-staged: ${sc_label} stops the running shellcheck and exits 2" "exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_alive}, seconds to stop: ${sc_stop_secs}${sc_kj_note}" "output: $(cat "${work_dir}/out")"
  fi
done

# job_state の状態語の解釈（ジョブ表を偽物に差し替えて確かめる。本体の関数をそのまま取り出す）。
# 信号で死んだ子の状態語（Hangup・Interrupt・Segmentation fault 等）は終了済み（none）で、誤った診断・wait の省略を起こさない。
# 一致した pid の状態語が空・ジョブ表を取れないときだけ unknown（信号を送らない）。
sc_js_fn="$(awk '/^job_state\(\) \{/ {p=1} p {print} p && /^\}/ {exit}' "${SC_STAGED}")"
sc_js_check() {
  # sc_js_check <期待> <説明> <jobs の出力の 1 行。"FAIL" なら jobs が失敗する> [読み取りの失敗: first=最初の read から失敗する / second=2 回目の read から失敗する]
  # 読み取りの失敗は、read を差し替えて起こす（読み取りエラーは EOF と同じ非 0 で終わるため、一覧を最後まで読めたかを終端の印で確かめる）。
  sc_js_got="$(
    # shellcheck disable=SC2034 # eval で取り出した job_state が使う
    tmp_dir="${work_dir}"
    SC_FAKE_LINE="$3"
    sc_js_reads=0
    sc_js_readfail="${4:-}"
    # shellcheck disable=SC2329 # eval で取り出した job_state が呼ぶ
    read() {
      sc_js_reads=$((sc_js_reads + 1))
      case "${sc_js_readfail}" in
        first) return 1 ;;
        second) [ "${sc_js_reads}" -lt 2 ] || return 1 ;;
      esac
      builtin read -r "${@:2}"
    }
    # shellcheck disable=SC2329 # eval で取り出した job_state が呼ぶ
    jobs() {
      [ "${SC_FAKE_LINE}" != "FAIL" ] || return 1
      printf '%s\n' "${SC_FAKE_LINE}"
    }
    eval "${sc_js_fn}"
    job_state 4242
    # shellcheck disable=SC2154 # eval で取り出した job_state が設定する
    echo "${job_state_result}"
  )"
  if [ "${sc_js_got}" = "$1" ]; then
    pass "shellcheck-staged: job_state treats ${2} as ${1}"
  else
    fail "shellcheck-staged: job_state treats ${2} as ${1}" "got: ${sc_js_got:-none}" "line: $3"
  fi
}
sc_js_check owned "Running" "[1]+ 4242 Running                    sleep 30 &"
sc_js_check owned "Stopped" "[1]+ 4242 Stopped                    sleep 30"
sc_js_check owned "Stopped(SIGTSTP)" "[1]+ 4242 Stopped(SIGTSTP)           sleep 30"
sc_js_check owned "Stopped (signal)" "[1]+ 4242 Stopped (signal)           sleep 30"
sc_js_check owned "Suspended" "[1]+ 4242 Suspended                  sleep 30"
sc_js_check owned "Suspended (signal)" "[1]+ 4242 Suspended (signal)         sleep 30"
sc_js_check none "Done" "[1]+ 4242 Done                       sleep 30"
sc_js_check none "Exit 3" "[1]+ 4242 Exit 3                     sleep 30"
sc_js_check none "Terminated" "[1]+ 4242 Terminated                 sleep 30"
sc_js_check none "Interrupt (a child killed by SIGINT)" "[1]+ 4242 Interrupt                  sleep 30"
sc_js_check none "Hangup (a child killed by SIGHUP)" "[1]+ 4242 Hangup                     sleep 30"
sc_js_check none "Segmentation fault" "[1]+ 4242 Segmentation fault        sleep 30"
sc_js_check none "a pid not in the table" "[1]+ 9999 Running                    sleep 30 &"
sc_js_check unknown "an empty state word" "[1]+ 4242"
sc_js_check unknown "a failing jobs" "FAIL"
# 退行を捕まえる（#6136）: 読み取りエラーを EOF と同じにすると、載っているジョブの一覧を読めないまま none（停止済み）になる
sc_js_check unknown "a read error before the listed child (an incomplete table)" "[1]+ 4242 Running                    sleep 30 &" first
sc_js_check unknown "a read error after the listed child (the end mark is not reached)" "[1]+ 4242 Running                    sleep 30 &" second
sc_js_check unknown "a read error on a table without the child" "[1]+ 9999 Running                    sleep 30 &" first

# sc_klog_read <記録>: 差し込んだ kill の記録（1 行 = 信号と宛先の pid）から、信号の並び（sc_kr_sigs）と宛先の pid の並び（sc_kr_pids）を空白区切りで取り出す。
sc_klog_read() {
  sc_kr_sigs="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $1 }' "$1" 2> /dev/null || true)"
  sc_kr_pids="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $2 }' "$1" 2> /dev/null || true)"
  # 3 列目: 送った時点の宛先の親の pid（取れなければ空）
  sc_kr_ppids="$(awk '{ printf "%s%s", (NR > 1 ? " " : ""), $3 }' "$1" 2> /dev/null || true)"
}

# 子への信号の送り先を固定する補助（pid の再利用・短く終わる別の子へ信号を送らない。sc_kill_job・sc_orphan_signal と同じ規律）。
# sc_same_args_children <親>: 親の直接の子のうち、引数が親と同じ bash（`&` で fork した subshell）の pid を出す。
# 本体はバッチの前に、引数の違う短命の子（getconf・mktemp・wc・sort の偽物や本物）も作るため、引数で区別する。
sc_same_args_children() {
  sc_sa_args="$(ps -p "$1" -o args= 2> /dev/null | sed 's/^ *//; s/ *$//')"
  [ -n "${sc_sa_args}" ] || return 1
  ps -A -o pid= -o ppid= -o args= 2> /dev/null | awk -v p="$1" -v a="${sc_sa_args}" '$2 == p { r = $0; sub(/^ *[0-9]+ +[0-9]+ +/, "", r); sub(/ *$/, "", r); if (r == a) print $1 }'
}
# sc_find_batch <親>: 親の直接の子が 1 つだけで、それが引数が親と同じ子のときだけ、その pid を sc_batch_pid に入れる（短命の別の子が残っている間・0 件・2 件以上なら空）。
sc_find_batch() {
  sc_batch_pid=""
  sc_fb_list="$(sc_same_args_children "$1" || true)"
  sc_fb_all="$(ps -A -o pid= -o ppid= 2> /dev/null | awk -v p="$1" '$2 == p { n++ } END { print n + 0 }')"
  sc_fb_n="$(printf '%s\n' "${sc_fb_list}" | grep -c . || true)"
  if [ "${sc_fb_n}" = "1" ] && [ "${sc_fb_all}" = "1" ]; then
    sc_batch_pid="${sc_fb_list}"
  fi
}
# sc_wait_batch <親>: 間を置いた 2 回続けて同じ pid が見つかるまで待つ（最大 10 秒）。見つからなければ sc_batch_pid は空。時間の経過だけには頼らない。
sc_wait_batch() {
  sc_wb_prev=""
  sc_wb_n=0
  while [ "${sc_wb_n}" -lt 50 ]; do
    sc_find_batch "$1"
    if [ -n "${sc_batch_pid}" ] && [ "${sc_batch_pid}" = "${sc_wb_prev}" ]; then
      return 0
    fi
    sc_wb_prev="${sc_batch_pid}"
    sleep 0.2
    sc_wb_n=$((sc_wb_n + 1))
  done
  sc_batch_pid=""
  return 1
}
# sc_write_kill_env <ファイル> <差し込む jobs の定義（空なら差し込まない）> <kill を失敗させる条件（空なら失敗させない）>:
# BASH_ENV で読ませるファイルを書く。差し込む kill は、本体が渡した最初の引数（信号）を SC_KLOG へ記録してから、条件に当たれば失敗し、そうでなければ本物の kill を呼ぶ。
sc_write_kill_env() {
  {
    printf '%s\n' "$2"
    printf '%s\n' "kill() { printf '%s %s %s\\n' \"\$1\" \"\${2:-}\" \"\$(ps -p \"\${2:-0}\" -o ppid= 2> /dev/null | tr -d ' ')\" >> \"\${SC_KLOG}\"; ${3:-true}; builtin kill \"\$@\"; }"
  } > "$1"
}
# sc_signal_child <信号> <pid> <親>: 送る直前に、pid が今も親の直接の子で、引数が親と同じことを確かめ直してから送る。
sc_signal_child() {
  sc_same_args_children "$3" | grep -qx "$2" || return 1
  kill -"$1" "$2" 2> /dev/null
}

# 信号を送れない・所有を確かめられない中断では、信号を送らず（または送れず）、固定の診断を出し、wait せずに 2 で終わる。
# BASH_ENV で差し込む関数で `jobs` か `kill` を失敗させる。偽の shellcheck は CONT・TERM を受けても生き続け、受けた信号を SC_SIGLOG へ記録する
# （sleep のままでは CONT を誤って送っても生きているため、合格してしまう）。wait へ進めば偽物の眠りが終わるまで戻らないため、10 秒未満で終わることで wait の省略を確かめる。
# 親が送る CONT・TERM の宛先はバッチの子（偽の shellcheck の 1 つ上）で、偽物には直接届かない。そのため、本体が送ろうとした信号は、差し込む kill が SC_KLOG へ記録して確かめる
# （偽物の記録では、バッチの子へ誤って送られた CONT・TERM を見落とす。バッチの子が TERM を受ければ batch_stop が偽物を止めるため、偽物が生きていることは TERM の不達の証拠になる）。
# sc_unk_check <名前> <差し込む jobs の定義（空なら差し込まない）> <差し込む kill の失敗の条件（空なら失敗させない）> <診断の断片> <信号の送り先: script か batch> <本体が送ろうとした信号の記録>
#   script: スクリプト本体（親）へ TERM を送る。batch: バッチの子（引数が親と同じ子がちょうど 1 つ）へ TERM を直接送る（batch_stop の経路）。
sc_unk_check() {
  sc_unk_env="${work_dir}/sc-unknown-env.sh"
  sc_write_kill_env "${sc_unk_env}" "$2" "$3"
  sc_klog="${work_dir}/sc.klog"
  rm -f "${sc_klog}"
  : > "${sc_klog}"
  sc_unk_dest=""
  sc_pidfile="${work_dir}/sc.pid"
  sc_siglog="${work_dir}/sc.siglog"
  rm -f "${sc_pidfile}" "${sc_siglog}"
  : > "${sc_siglog}"
  # 出力先は起動の前に作る（読み出しが set -e で試験を終わらせず、無いことも失敗として数える）
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_unk_env}" SC_KLOG="${sc_klog}" SC_SIGLOG="${sc_siglog}" SC_PIDFILE="${sc_pidfile}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" sigtrap.sh a.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_waited=0
  while [ ! -s "${sc_pidfile}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  sc_fake_start=""
  if [ -n "${sc_fake_pid}" ]; then
    sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
  fi
  sc_kj_note=""
  sc_unk_err=""
  if [ -z "${sc_fake_start}" ]; then
    sc_unk_err="the fake shellcheck did not start"
  fi
  if ! "${HOOK_BASH}" -n "${sc_unk_env}" 2> /dev/null; then
    sc_unk_err="the injected functions have a syntax error"
  fi
  sc_t0="${SECONDS}"
  if [ -z "${sc_unk_err}" ]; then
    if [ "$5" = "batch" ]; then
      # 他の直接の子が消え、バッチの子が 2 回続けて同じ pid で見つかるのを待ってから送る
      sc_wait_batch "${sc_script_pid}" || true
      if [ -z "${sc_batch_pid}" ]; then
        sc_unk_err="the batch child could not be identified"
      elif ! sc_signal_child TERM "${sc_batch_pid}" "${sc_script_pid}"; then
        sc_unk_err="the TERM could not be sent to the batch child"
      fi
      sc_unk_dest="${sc_fake_pid}"
    else
      # 親が信号を送る宛先はバッチの子。送る前に、他の直接の子が消えて一意に定まるのを待って控える
      if sc_wait_batch "${sc_script_pid}"; then
        sc_unk_dest="${sc_batch_pid}"
      else
        sc_unk_err="the batch child could not be identified"
      fi
      sc_kill_job TERM "${sc_script_pid}" || sc_unk_err="the TERM could not be sent to the script"
    fi
  fi
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_unk_secs=$((SECONDS - sc_t0))
  sc_unk_alive=0
  if [ -n "${sc_fake_pid}" ] && kill -0 "${sc_fake_pid}" 2> /dev/null; then
    sc_unk_alive=1
  fi
  # 偽物の trap は sleep の終わりまで遅れて走るため、少し待ってから記録を読む（誤って送られた信号を取りこぼさない）
  sleep 0.4
  sc_unk_sigs="$(tr '\n' ' ' < "${sc_siglog}" 2> /dev/null | sed 's/ *$//' || true)"
  sc_klog_read "${sc_klog}"
  sc_unk_sent="${sc_kr_sigs}"
  # 宛先: 記録された pid が全て、期待した pid（script 対象はバッチの子、batch 対象は偽物）と一致する。記録が無ければ照合しない。
  sc_unk_dest_ok=1
  for sc_kd in ${sc_kr_pids}; do
    [ "${sc_kd}" = "${sc_unk_dest}" ] || sc_unk_dest_ok=0
  done
  # 後始末: 偽物は止まらず生き続けるので、起動時の開始時刻が一致し、引数が sigtrap.sh を含むときだけ TERM を送る（pid の再利用で別のプロセスへ送らない）
  if [ -n "${sc_fake_start}" ] && [ "$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)" = "${sc_fake_start}" ] && [[ "$(ps -p "${sc_fake_pid}" -o args= 2> /dev/null || true)" == *sigtrap.sh* ]]; then
    kill -TERM "${sc_fake_pid}" 2> /dev/null || true
    # 偽物の trap の記録が次の確認へ混ざらないよう、終わるのを待つ
    sc_waited=0
    while kill -0 "${sc_fake_pid}" 2> /dev/null && [ "${sc_waited}" -lt 50 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
  fi
  sc_unk_out="$(cat "${work_dir}/out" 2> /dev/null || true)"
  if [ -z "${sc_unk_err}" ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_unk_alive}" -eq 1 ] && [ "${sc_unk_secs}" -lt 10 ] && [ -z "${sc_unk_sigs}" ] && [ "${sc_unk_sent}" = "$6" ] && [ "${sc_unk_dest_ok}" -eq 1 ] && [[ "${sc_unk_out}" == *"$4"* ]]; then
    pass "shellcheck-staged: ${1}"
  else
    fail "shellcheck-staged: ${1}" "${sc_unk_err:+setup failed: ${sc_unk_err}; }exit ${sc_rc}, fake still running: ${sc_unk_alive}, seconds: ${sc_unk_secs}, signals received by the fake: ${sc_unk_sigs:-none}, signals the script tried to send: ${sc_unk_sent:-none} to pids [${sc_kr_pids:-none}] (expected: ${6:-none} to ${sc_unk_dest:-no pid})${sc_kj_note}" "output: ${sc_unk_out}"
  fi
}
# 差し込む関数は bash 3.2 でも動く書き方にする（`builtin`・`${var:-}`・case のみ）。sc_pid は run_batch の局所変数で、親では未設定（バッチの子の中だけ設定される）。
# 記録される信号は、本体が kill に渡した最初の引数（-CONT・-TERM）。TERM の前に CONT が先に送られること、所有を確かめられなければどちらも送られないことを確かめる。
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM with an unreadable job table sends no signal and does not wait" 'jobs() { return 1; }' '' "cannot confirm ownership of child" script ""
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM with a job table that only the parent cannot read sends no signal and does not wait" 'jobs() { [ -n "${sc_pid:-}" ] || return 1; builtin jobs "$@"; }' '' "cannot confirm ownership of child" script ""
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM whose signals cannot be sent to a live child does not wait" '' 'return 1' "cannot stop child" script "-CONT"
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM that cannot be sent after a CONT does not wait" '' 'case "$1" in -TERM) return 1 ;; esac' "cannot stop child" script "-CONT -TERM"
# shellcheck disable=SC2016 # 差し込む関数の定義は、展開せずにそのまま BASH_ENV のファイルへ書く
sc_unk_check "a TERM to a batch child whose kill fails does not wait" '' '[ -z "${sc_pid:-}" ] || return 1' "cannot stop child" batch "-CONT"

# 起動より前に張る早い trap（sc_pending=1）と、停止の trap への差し替え（batch_stop）の間に届いた中断は、保留の旗（sc_pending）で扱う（孫の shellcheck を残さず、2 で終わる）。
# FIFO の open を止め所にする方式は、bash の版・OS によって中断された open をやり直さない（macOS の bash 3.2 は EINTR で失敗する）ため使わない。
# 代わりに、BASH_ENV で差し込む `trap` 関数が、本体の 2 か所の `trap` の呼び出しで、go の印のファイルが現れるまで待つ（外部の sleep の繰り返し。信号は早い trap が受けて旗を立て、待ちを続ける）。
#   1. 起動の前: 早い trap（`sc_pending=1`）を張った直後。shellcheck はまだ起動していない（バッチの子に shellcheck の子が無く、pidfile も無い）ことを確かめてから TERM を送る。
#      早い trap が起動より前に張られていることを見る（起動と pid の記録の後へ動かす変異は、この時点で偽物が起動済みになり落ちる）。
#   2. pid の記録の後・停止の trap への差し替えの前: 差し替え（`batch_stop`）の直前。shellcheck の pid は記録済みで、早い trap が張られたまま。
#      旗の確認が差し替えの後に在ること（確認を差し替えの前へ動かす変異は、旗が立つ前に確認が済み、落ちる）を見る。
# 確かめる範囲（どちらも）:
#   - TERM を送る前に、待ちに入った印（held）があること。TERM の後もバッチの子が生きていること（既定の動作で死んでいない）。
#   - 保留の旗の経路を通った正の証拠: 差し込んだ kill の記録が -CONT -TERM で、宛先が 2 回とも同じ pid で、送った時点の親がバッチの子であり、
#     その pid が残っていないこと（バッチが記録した $! の宛先と消滅）。pidfile が現れた場合は、それとも一致すること。
#   - 2 で終わること、偽物が残らないこと、10 秒未満で終わること。親の pending_signal の区間は対象外（同期できる口が無い）。
# 飛ばす分岐は持たない（どの環境でも同じ経路を試す）。
# sc_hold_case <試験名> <止める trap の第 1 引数> <1=その trap を張った後で止める / 0=張る前で止める>
sc_hold_case() {
  local sc_hold_name="$1" sc_hold_arg="$2" sc_hold_first="$3"
  sc_pidfile="${work_dir}/sc.pid"
  sc_klog="${work_dir}/sc.klog"
  sc_hold_env="${work_dir}/sc-hold-env.sh"
  sc_hold_held="${work_dir}/sc.held"
  sc_hold_go="${work_dir}/sc.go"
  # shellcheck disable=SC2016 # BASH_ENV のファイルへ、展開せずにそのまま書く
  sc_write_kill_env "${sc_hold_env}" 'trap() { if [ "${1:-}" = "${SC_HOLD_ARG}" ]; then [ "${SC_HOLD_INSTALL_FIRST}" != 1 ] || builtin trap "$@"; : > "${SC_HELD}"; _sc_hw=0; while [ ! -e "${SC_GO}" ] && [ "${_sc_hw}" -lt 150 ]; do sleep 0.1; _sc_hw=$((_sc_hw + 1)); done; [ "${SC_HOLD_INSTALL_FIRST}" = 1 ] || builtin trap "$@"; return 0; fi; builtin trap "$@"; }' ""
  rm -f "${sc_pidfile}" "${sc_klog}" "${sc_hold_held}" "${sc_hold_go}"
  : > "${sc_klog}"
  : > "${work_dir}/out"
  sc_reset
  BASH_ENV="${sc_hold_env}" SC_HOLD_ARG="${sc_hold_arg}" SC_HOLD_INSTALL_FIRST="${sc_hold_first}" SC_HELD="${sc_hold_held}" SC_GO="${sc_hold_go}" SC_KLOG="${sc_klog}" TMPDIR="${work_dir}" SC_PIDFILE="${sc_pidfile}" SC_JOBS=8 SC_LOG="${sc_log}" PATH="${sc_shim_dir}:${PATH}" "${HOOK_BASH}" "${SC_STAGED}" hang.sh > "${work_dir}/out" 2>&1 &
  sc_script_pid=$!
  sc_early_err=""
  sc_t0="${SECONDS}"
  sc_kj_note=""
  # 待ちに入った印（pid の記録の後）が現れるまで待ち、他の直接の子が消えてバッチの子が間を置いた 2 回続けて同じ pid で見つかるまで待つ
  sc_waited=0
  while [ ! -e "${sc_hold_held}" ] && [ "${sc_waited}" -lt 100 ]; do
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if [ ! -e "${sc_hold_held}" ]; then
    sc_early_err="the batch child never reached the point before the trap swap"
  elif ! sc_wait_batch "${sc_script_pid}"; then
    sc_early_err="the batch child could not be identified"
  elif [ "${sc_hold_first}" = 1 ] && { [ -s "${sc_pidfile}" ] || ps -A -o ppid= -o args= 2> /dev/null | awk -v p="${sc_batch_pid}" '$1 == p && $0 ~ /shellcheck/ { f = 1 } END { exit f ? 0 : 1 }'; }; then
    # 早い trap の直後に止めた場合は、この時点で shellcheck がまだ起動していないこと（起動より前に trap が張られている証拠）
    sc_early_err="the fake shellcheck had already started when the early trap was in place"
  elif ! sc_signal_child TERM "${sc_batch_pid}" "${sc_script_pid}"; then
    sc_early_err="the TERM could not be sent to the batch child"
  else
    # 信号の処理が済むのを待ち、バッチの子がまだ生きている（早い trap が旗を立て、待ちを続けている）ことを確かめる
    sleep 0.3
    if ! sc_same_args_children "${sc_script_pid}" | grep -qx "${sc_batch_pid}"; then
      sc_early_err="the batch child did not survive the TERM (it died by the default action)"
    fi
  fi
  # 失敗の経路でも、待ちを解いて回収する
  : > "${sc_hold_go}"
  # 偽物の開始時刻は、pidfile が現れた時点（待つ間の最初の観測）で控える。現れなければ控えず、残っても止めない（偽物の眠りは 30 秒で終わる）。
  sc_fake_pid=""
  sc_fake_start=""
  sc_waited=0
  while kill -0 "${sc_script_pid}" 2> /dev/null && [ "${sc_waited}" -lt 200 ]; do
    if [ -z "${sc_fake_start}" ] && [ -s "${sc_pidfile}" ]; then
      sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
      if [ -n "${sc_fake_pid}" ]; then
        sc_fake_start="$(ps -p "${sc_fake_pid}" -o lstart= 2> /dev/null || true)"
      fi
    fi
    sleep 0.1
    sc_waited=$((sc_waited + 1))
  done
  if kill -0 "${sc_script_pid}" 2> /dev/null; then
    sc_reap "${sc_script_pid}" || true
    sc_rc=124
  elif wait "${sc_script_pid}"; then
    sc_rc=0
  else
    sc_rc=$?
  fi
  sc_pend_secs=$((SECONDS - sc_t0))
  if [ -z "${sc_fake_pid}" ]; then
    sc_fake_pid="$(cat "${sc_pidfile}" 2> /dev/null || true)"
  fi
  sc_klog_read "${sc_klog}"
  sc_pend_sent="${sc_kr_sigs}"
  # 宛先: バッチが記録した shellcheck の pid（$!）は、差し込んだ kill の記録の宛先として試験側で取れる。偽物が書く pidfile には頼らない。
  # 記録の宛先が 2 回（CONT・TERM）とも同じ数字の pid で、送った時点の親が TERM の前に見つけたバッチの子であること。
  # pidfile が現れた場合は、それとも一致すること（偽物が書く pid と、バッチが記録した pid が別なら失敗）。
  sc_pend_dest_ok=0
  sc_pend_target=""
  case "${sc_kr_pids}" in
    [0-9]*" "[0-9]*)
      sc_pend_first="${sc_kr_pids%% *}"
      sc_pend_second="${sc_kr_pids#* }"
      if [ "${sc_pend_first}" = "${sc_pend_second}" ] && [ "${sc_kr_ppids}" = "${sc_batch_pid} ${sc_batch_pid}" ] \
        && { [ -z "${sc_fake_pid}" ] || [ "${sc_fake_pid}" = "${sc_pend_first}" ]; }; then
        sc_pend_dest_ok=1
        sc_pend_target="${sc_pend_first}"
      fi
      ;;
  esac
  # 子の消滅: 宛先（pidfile があればそれも）が残っていないこと
  sc_pend_alive=0
  for sc_pend_p in ${sc_pend_target} ${sc_fake_pid}; do
    sc_waited=0
    while kill -0 "${sc_pend_p}" 2> /dev/null && [ "${sc_waited}" -lt 30 ]; do
      sleep 0.1
      sc_waited=$((sc_waited + 1))
    done
    if kill -0 "${sc_pend_p}" 2> /dev/null; then
      sc_pend_alive=1
      # 残った偽物は、pidfile が現れた時点で控えた開始時刻が一致し、まだ sleep であるときだけ止める
      if [ "${sc_pend_p}" = "${sc_fake_pid}" ]; then
        sc_orphan_signal TERM "${sc_fake_pid}" "${sc_fake_start}" || true
      fi
    fi
  done
  sc_pend_out="$(cat "${work_dir}/out" 2> /dev/null || true)"
  if [ -z "${sc_early_err}" ] && [ "${sc_pend_sent}" = "-CONT -TERM" ] && [ "${sc_pend_dest_ok}" -eq 1 ] && [ "${sc_rc}" -eq 2 ] && [ "${sc_pend_alive}" -eq 0 ] && [ "${sc_pend_secs}" -lt 10 ] \
    && [[ "${sc_pend_out}" == *"no valid result for hang.sh (the check did not finish)"* ]] && [[ "${sc_pend_out}" != *"cannot open the output file of batch 0"* ]]; then
    pass "${sc_hold_name}"
  else
    fail "${sc_hold_name}" "${sc_early_err:+setup failed: ${sc_early_err}; }exit ${sc_rc}, fake pid: ${sc_fake_pid:-none}, still running: ${sc_pend_alive}, seconds: ${sc_pend_secs}, kill calls of the batch child: ${sc_pend_sent:-none} to pids [${sc_kr_pids:-none}] with parents [${sc_kr_ppids:-none}] (expected: -CONT -TERM to one pid, whose parent is the batch child ${sc_batch_pid:-none}, which is gone afterwards)${sc_kj_note}" "output: ${sc_pend_out}"
  fi
}

sc_hold_case "shellcheck-staged: a TERM before the batch starts its shellcheck exits 2 and leaves no process" "sc_pending=1" 1
sc_hold_case "shellcheck-staged: a TERM after the batch records its shellcheck pid and before it swaps its trap exits 2 and leaves no process" batch_stop 0

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
