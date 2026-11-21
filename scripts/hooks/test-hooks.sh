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
# LOG_GIT_FILE があれば、呼び出しの引数を 1 行ずつ記録する（HEAD の ID が同じ値で渡ることの確認用）
if [ -n "${LOG_GIT_FILE:-}" ]; then
  printf '%s\n' "$*" >> "${LOG_GIT_FILE}"
fi
# LOG_GIT_INDEX があれば、呼び出しが読む index（GIT_INDEX_FILE。未設定は -）と引数をタブ区切りで記録する
if [ -n "${LOG_GIT_INDEX:-}" ]; then
  printf '%s\t%s\n' "${GIT_INDEX_FILE:--}" "$*" >> "${LOG_GIT_INDEX}"
fi
# MOVE_HEAD_ON_ARG を含む呼び出しの前に一度だけ、HEAD の指す参照を MOVE_HEAD_TO へ動かす（走査中の HEAD の変化の再現用）
if [ -n "${MOVE_HEAD_ON_ARG:-}" ] && [ ! -e "${MOVE_HEAD_FLAG:-/nonexistent}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "${MOVE_HEAD_ON_ARG}" ]; then
      : > "${MOVE_HEAD_FLAG}"
      "${REAL_GIT}" update-ref HEAD "${MOVE_HEAD_TO}"
      break
    fi
  done
fi
# DELETE_INDEX_ON_ARG を含む呼び出しの前に一度だけ、DELETE_INDEX_PATH（元の index）を消す（走査中に index が消える場合の再現用）
if [ -n "${DELETE_INDEX_ON_ARG:-}" ] && [ ! -e "${DELETE_INDEX_FLAG:-/nonexistent}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "${DELETE_INDEX_ON_ARG}" ]; then
      : > "${DELETE_INDEX_FLAG}"
      rm -f -- "${DELETE_INDEX_PATH}"
      # DELETE_INDEX_MKFIFO があれば、消した後の同じパスに FIFO を置く（走査中の通常ファイル以外への差し替えの再現用）
      if [ -n "${DELETE_INDEX_MKFIFO:-}" ]; then
        mkfifo "${DELETE_INDEX_PATH}"
      fi
      # DELETE_INDEX_SYMLINK_TO があれば、消した後の同じパスにそのパスへのリンクを置く（走査中のリンクへの付け替えの再現用）
      if [ -n "${DELETE_INDEX_SYMLINK_TO:-}" ]; then
        ln -s "${DELETE_INDEX_SYMLINK_TO}" "${DELETE_INDEX_PATH}"
      fi
      break
    fi
  done
fi
# MUTATE_INDEX_ON_ARG を含む呼び出しの前に一度だけ、元の index へ MUTATE_INDEX_ADD のパスを git add する（走査中の index の変化の再現用）。
# 検出器は固定した複写を GIT_INDEX_FILE で指して git を呼ぶため、それを外して既定の index を書き換える。
if [ -n "${MUTATE_INDEX_ON_ARG:-}" ] && [ ! -e "${MUTATE_INDEX_FLAG:-/nonexistent}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "${MUTATE_INDEX_ON_ARG}" ]; then
      : > "${MUTATE_INDEX_FLAG}"
      (unset GIT_INDEX_FILE; "${REAL_GIT}" add -- "${MUTATE_INDEX_ADD}")
      break
    fi
  done
fi
# FAIL_GIT_BOTH1・FAIL_GIT_BOTH2 の両方を引数に含む呼び出しだけを失敗させる（cat-file -s は通し cat-file blob だけを失敗させる再現用）
if [ -n "${FAIL_GIT_BOTH1:-}" ]; then
  both1=0
  both2=0
  for arg in "$@"; do
    [ "${arg}" = "${FAIL_GIT_BOTH1}" ] && both1=1
    [ "${arg}" = "${FAIL_GIT_BOTH2:-}" ] && both2=1
  done
  if [ "${both1}" -eq 1 ] && [ "${both2}" -eq 1 ]; then
    echo "git shim: forced failure on a pair" >&2
    exit 128
  fi
fi
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
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SCAN_BIN:-${SECRET_SCAN}}" ${SCAN_ARG:+"${SCAN_ARG}"} > "${work_dir}/out" 2>&1 || rc=$?
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

# stage_lock: 走査対象の index に、検出器と同じ内容の skills-lock.json（lock_ok）を登録する。
# 除外は検出器の一覧と対象の index の一覧の共通部分だけに効く（#6000）。lock_ok は後ろで定義されるため実行時に参照する。
stage_lock() {
  printf '%s\n' "${lock_ok}" > skills-lock.json
  git add -- skills-lock.json
}

# stage <path> <内容（printf の書式ではなくそのまま書き込む）>
stage() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" > "$1"
  git add -- "$1"
}

# make_detector <名前> [skills-lock.json の内容]: 検出器の複製を ${work_dir}/det-<名前>/ に作り、
# スクリプトのパスを det_bin に入れる。内容を省くと skills-lock.json を置かない（除外なし）。
# 除外の一覧は検出器の repo の skills-lock.json から読む（走査対象の repo のものは読まない。#6000）。
det_bin=""
make_detector() {
  local d="${work_dir}/det-$1"
  mkdir -p "${d}/scripts/hooks"
  cp "${SECRET_SCAN}" "${d}/scripts/hooks/secret-scan.sh"
  if [ "$#" -ge 2 ]; then
    printf '%s\n' "$2" > "${d}/skills-lock.json"
  fi
  det_bin="${d}/scripts/hooks/secret-scan.sh"
}
lock_ok='{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "example/skills",
      "skillPath": "skills/vendored-a/SKILL.md"
    },
    "vendored-b": {
      "source": "example/skills"
    }
  }
}'
make_detector ok "${lock_ok}"
det_ok="${det_bin}"
make_detector nolock
det_nolock="${det_bin}"

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

# 検出器の skills-lock.json が壊れていれば検査不能（fail-closed）。診断に名前・パスを出さない
bad_n=0
# grep_none <パターン> <ファイル>: 不一致（終了コード 1）のときだけ成功。読み取り失敗等（2 以上）は失敗にする。
grep_none() {
  local grc=0
  grep -q "$1" "$2" || grc=$?
  [ "${grc}" -eq 1 ]
}
# run_bad_lock <説明>: 準備済みの検出器（det_bin）が、固定の文言・終了コード 2 で止まること
run_bad_lock() {
  local desc="$1" rc=0
  new_repo; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_bin}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting" "${work_dir}/out" \
    && grep_none "vendored\|det-bad\|/" "${work_dir}/out"; then
    pass "secret-scan: ${desc}"
  else
    fail "secret-scan: ${desc}" "expected exit 2 with a fixed message, got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
}
check_bad_lock() {
  bad_n=$((bad_n + 1))
  make_detector "bad${bad_n}" "$2"
  run_bad_lock "$1"
}
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

check_bad_lock "skills-lock.json with a missing comma between members is rejected" '{
  "version": 2
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with a missing comma between skills is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
    "vendored-b": {
      "source": "y"
    }
  }
}'
check_bad_lock "skills-lock.json with a trailing comma is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    },
  }
}'
check_bad_lock "skills-lock.json with a broken skill body is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x",
      "hash":
    }
  }
}'
check_bad_lock "skills-lock.json with an unquoted skill key body is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      source: "x"
    }
  }
}'
check_bad_lock "skills-lock.json with skills as an array is rejected" '{
  "version": 2,
  "skills": ["vendored-a"]
}'
check_bad_lock "skills-lock.json with a string skill value is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": "x"
  }
}'
check_bad_lock "skills-lock.json with a string version is rejected" '{
  "version": "2",
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with trailing garbage is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
} x'
check_bad_lock "skills-lock.json with a second top-level value is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}
{}'
check_bad_lock "skills-lock.json with an escaped skill name is rejected" '{
  "version": 2,
  "skills": {
    "vendored\/a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with an invalid string escape is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "a\qb"
    }
  }
}'
check_bad_lock "skills-lock.json with an unterminated string is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x
    }
  }
}'
check_bad_lock "skills-lock.json with a duplicate version is rejected" '{
  "version": 2,
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json that is empty is rejected" ''

# 一覧の source 欄だけを差し替えた最小の一覧
lock_with_source() {
  printf '{\n  "version": 2,\n  "skills": {\n    "vendored-a": {\n      "source": "%s"\n    }\n  }\n}' "$1"
}
bs=$'\x5c'
check_bad_lock "skills-lock.json with an invalid UTF-8 byte is rejected" "$(lock_with_source "a"$'\xff'"b")"
check_bad_lock "skills-lock.json with an overlong UTF-8 sequence is rejected" "$(lock_with_source "a"$'\xc0\x80'"b")"
check_bad_lock "skills-lock.json with a UTF-8 surrogate is rejected" "$(lock_with_source "a"$'\xed\xa0\x80'"b")"
check_bad_lock "skills-lock.json with a truncated UTF-8 sequence is rejected" "$(lock_with_source "a"$'\xe3\x81'"b")"
# 不正なバイトの両側が、正しい列の除去で接して新しい列にならない（c2 + 正しい 3 バイト列 + 80）
check_bad_lock "skills-lock.json with a stray UTF-8 lead around a valid sequence is rejected" "$(lock_with_source "a"$'\xc2\xe2\x82\xac\x80'"b")"
check_bad_lock "skills-lock.json with a raw U+0001 in a string is rejected" "$(lock_with_source "a"$'\x01'"b")"
check_bad_lock "skills-lock.json with a raw U+001F in a string is rejected" "$(lock_with_source "a"$'\x1f'"b")"
check_bad_lock "skills-lock.json with a raw tab in a string is rejected" "$(lock_with_source "a"$'\t'"b")"
check_bad_lock "skills-lock.json with a raw newline in a string is rejected" "$(lock_with_source "a"$'\n'"b")"
check_bad_lock "skills-lock.json without a version is rejected" '{
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  }
}'
check_bad_lock "skills-lock.json with a duplicate skills key is rejected" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    }
  },
  "skills": {
    "vendored-b": {
      "source": "y"
    }
  }
}'
check_bad_lock "skills-lock.json with a unicode-escaped skill name is rejected" "{
  \"version\": 2,
  \"skills\": {
    \"vendored${bs}u0061\": {
      \"source\": \"x\"
    }
  }
}"
deep_open="$(printf '[%.0s' {1..40})"
deep_close="$(printf ']%.0s' {1..40})"
check_bad_lock "skills-lock.json nested deeper than the limit is rejected" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"deep\": ${deep_open}${deep_close}
    }
  }
}"
# NUL を含む一覧（読み取りが NUL で止まり、後ろが未検証になる経路）
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
printf '{\n  "version": 2,\n  "skills": {\n    "vendored-a": {\n      "source": "x"\n    }\n  }\n}\n\0trailing' > "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json containing a NUL byte is rejected"
# 一覧が普通のファイルでない（行き先の無いシンボリックリンク・ディレクトリ・FIFO）
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
ln -s "no-such-target" "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a dangling symlink is rejected"
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
mkdir "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a directory is rejected"
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
rm -f "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
mkfifo "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a FIFO is rejected"

# 構造の記号を含む文字列値・1 行に詰めた JSON は正しく読める（行単位の深さの数え方では誤判定した形）
check_good_lock() {
  local desc="$1" content="$2"
  make_detector "good${bad_n}" "${content}"
  bad_n=$((bad_n + 1))
  new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
  SCAN_BIN="${det_bin}" check_scan "${desc}" 0
}
check_good_lock "skills-lock.json with braces inside strings is accepted" '{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "a{b}c } {",
      "note": "q\"{\\"
    }
  }
}'
check_good_lock "skills-lock.json packed on one line is accepted" '{"version":2,"skills":{"vendored-a":{"source":"x","n":[1,-2.5e3,true,null,{"k":[]}]}}}'

check_good_lock "skills-lock.json with valid multibyte UTF-8 is accepted" "$(lock_with_source "$(printf 'caf\xc3\xa9 \xe6\x97\xa5\xe6\x9c\xac \xf0\x9f\x98\x80')")"
check_good_lock "skills-lock.json with a raw U+0020 in a string is accepted" "$(lock_with_source "a b")"
check_good_lock "skills-lock.json with a raw U+007F in a string is accepted" "$(lock_with_source "a"$'\x7f'"b")"
check_good_lock "skills-lock.json with a 70-digit number is accepted" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"big\": $(printf '1%.0s' {1..70}),
      \"neg\": -$(printf '2%.0s' {1..70}).5e10
    }
  }
}"
check_good_lock "skills-lock.json nested within the depth limit is accepted" "{
  \"version\": 2,
  \"skills\": {
    \"vendored-a\": {
      \"deep\": $(printf '[%.0s' {1..20})$(printf ']%.0s' {1..20})
    }
  }
}"

# 検出器のあるディレクトリ名が改行で終わっていても、除外の一覧を読める（dirname のコマンド置換は末尾の改行を落とす）
nl_root="${work_dir}/det-nl"
nl_dir="${nl_root}/s/h"$'\n'
mkdir -p "${nl_dir}"
cp "${SECRET_SCAN}" "${nl_dir}/secret-scan.sh"
printf '%s\n' "${lock_ok}" > "${nl_root}/skills-lock.json"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${nl_dir}/secret-scan.sh" check_scan "detector in a directory whose name ends with a newline reads its list" 0

# scripts がシンボリックリンクでも、検出器の実体の場所から一覧を読む（論理的な `..` で別のルートを選ばない。#6000）
lnk_real="${work_dir}/det-real"
lnk_other="${work_dir}/det-other"
mkdir -p "${lnk_real}/scripts/hooks" "${lnk_other}"
cp "${SECRET_SCAN}" "${lnk_real}/scripts/hooks/secret-scan.sh"
printf '%s\n' "${lock_ok}" > "${lnk_real}/skills-lock.json"
ln -s "${lnk_real}/scripts" "${lnk_other}/scripts"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${lnk_other}/scripts/hooks/secret-scan.sh" check_scan "detector reached through a symlinked scripts directory reads the real list" 0

# ファイル名だけで起動した相対リンクの先が / を含んでも、リンクの親ディレクトリを基準に解決する（#6000）
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
ln -s "../det-real/scripts/hooks/secret-scan.sh" sslink
SCAN_BIN="sslink" check_scan "a bare-name symlink with a relative target containing a slash resolves from the link's directory" 0

# 同じ checkout（検出器と走査対象が同じ repo）では、除外の一覧はコミット済み（HEAD）の名前だけを使う（#6000）。
# 未コミットの作業ツリー・index の編集で除外を足せない。
same_lock_two='{
  "version": 2,
  "skills": {
    "vendored-a": {
      "source": "x"
    },
    "other": {
      "source": "y"
    }
  }
}'
# same_checkout_repo [skills-lock.json の内容]: 検出器の複製と一覧をコミットした repo を作る
same_checkout_repo() {
  new_repo
  mkdir -p scripts/hooks
  cp "${SECRET_SCAN}" scripts/hooks/secret-scan.sh
  if [ "$#" -ge 1 ]; then
    printf '%s\n' "$1" > skills-lock.json
  fi
  git add -A
  git commit -q -m "chore: add detector"
}
same_bin="scripts/hooks/secret-scan.sh"

same_checkout_repo "${lock_ok}"; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed name is excluded" 0

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name added in the working tree only is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
git add skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name staged but not committed is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' "${same_lock_two}" > skills-lock.json
stage .agents/skills/other/SKILL.md "x = ${token}"
SCAN_ARG=--tracked SCAN_BIN="${same_bin}" check_scan "same checkout (--tracked): a name added in the working tree only is scanned" 1 .agents/skills/other/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{"version":2,"skills":{"vendored-b":{"source":"x"}}}' > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed name removed in the working tree is scanned" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken working-tree list is rejected" 2

# 登録の削除をステージしたあと、作業ツリーだけへ登録を戻しても除外は残らない（index の一覧でも名前を確かめる。#6000）
same_checkout_repo "${lock_ok}"
printf '%s\n' '{"version":2,"skills":{"vendored-b":{"source":"x"}}}' > skills-lock.json
git add skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a name unregistered in the index is scanned even if restored in the working tree" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
git rm -q --cached skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a list deleted from the index excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
git add skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken staged list is rejected" 2

same_checkout_repo
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a list absent from HEAD excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 作業ツリーに一覧が無くても、同じ checkout では HEAD の一覧を検証する（壊れていれば 2。#6011）
same_checkout_repo '{ "version": 2, "skills": {'
rm -f skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken committed list is rejected even without a working-tree list" 2
expect_out "same checkout: a broken committed list without a working-tree list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# 作業ツリーに一覧が無く HEAD の一覧が有効なら、何も除外しない
same_checkout_repo "${lock_ok}"
rm -f skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a valid committed list without a working-tree list excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 作業ツリーの一覧がシンボリックリンクなら（リンク先が有効でも）拒否する
same_checkout_repo "${lock_ok}"
cp skills-lock.json real-lock.json
rm -f skills-lock.json
ln -s real-lock.json skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a working-tree list that is a symlink is rejected" 2
expect_out "same checkout: a symlinked working-tree list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# HEAD の一覧がシンボリックリンクなら（作業ツリーが通常ファイルでも）拒否する
new_repo
mkdir -p scripts/hooks
cp "${SECRET_SCAN}" scripts/hooks/secret-scan.sh
# リンク先の文字列そのものを有効な一覧にして、blob の中身の構文検査だけでは拒否されない形にする
ln -s '{"version":2,"skills":{"vendored-a":{}}}' skills-lock.json
git add -A
git commit -q -m "chore: add detector with a symlinked list"
rm -f skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed list that is a symlink is rejected" 2
expect_out "same checkout: a symlinked committed list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# 検出器の一覧がシンボリックリンク（リンク先は有効）なら拒否する
bad_n=$((bad_n + 1))
make_detector "bad${bad_n}" "${lock_ok}"
mv "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json" "${det_bin%/scripts/hooks/secret-scan.sh}/real-lock.json"
ln -s real-lock.json "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
run_bad_lock "skills-lock.json as a symlink to a valid file is rejected"

# HEAD の参照: 未作成ブランチは除外なしで走査する。参照があるのに解決できない場合は検査不能（2）
same_checkout_repo "${lock_ok}"
git symbolic-ref HEAD refs/heads/unborn
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: an unborn branch excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

same_checkout_repo "${lock_ok}"
head_sha="$(git rev-parse HEAD)"
rm -f ".git/objects/${head_sha:0:2}/${head_sha:2}"
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a HEAD whose commit object is missing exits 2" 2
expect_out "same checkout: an unresolvable HEAD reports a fixed message" "secret-scan: cannot read HEAD; aborting"

# 壊れた参照（内容が不正）は未作成ブランチと区別し、検査不能（2）にする（show-ref の終了コード 1 は両方で返る。#6011）
same_checkout_repo "${lock_ok}"
git symbolic-ref HEAD refs/heads/broken
printf '%s\n' 'not-a-valid-object-id' > .git/refs/heads/broken
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken loose ref is not treated as an unborn branch" 2
expect_out "same checkout: a broken ref reports a fixed message" "secret-scan: cannot read HEAD; aborting"

# 他のブランチが packed-refs にあっても、HEAD の指す参照が無ければ未作成ブランチとして除外なしで走査する
same_checkout_repo "${lock_ok}"
git pack-refs --all
git symbolic-ref HEAD refs/heads/unborn-packed
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: an unborn branch next to packed refs excludes nothing" 1 .agents/skills/vendored-a/SKILL.md

# 空の skills は、同じ checkout の HEAD の一覧でも固定の診断文で 2 にする（Bash 3.2 の空配列に依らない）
same_checkout_repo '{"version":2,"skills":{}}'
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a committed list with an empty skills map exits 2" 2
expect_out "same checkout: an empty committed skills map reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

# 実装の形の確認（macOS の bash 3.2 が無い環境向け）: 空になり得る配列の長さ参照・素の展開を使わない。
# 一覧の読み取りは一時ファイルへの複写 1 回だけにし、元のパスを read・wc で別々に開かない
bad_forms="$(grep -nE '[$]\{#parsed_names\[@\]\}|[^+]"[$]\{(parsed_names|work_names)\[@\]\}"' "${SECRET_SCAN}" | grep -v '^[0-9]*:[[:space:]]*#' | grep -vF '[@]+' || true)"
if [ -z "${bad_forms}" ]; then
  pass "secret-scan: no length reference or bare expansion of possibly-empty arrays (bash 3.2 set -u)"
else
  fail "secret-scan: no length reference or bare expansion of possibly-empty arrays (bash 3.2 set -u)" "found: ${bad_forms}"
fi
snap_reads="$(grep -cE '(js|wc -c) < "[$]1"' "${SECRET_SCAN}" || true)"
# shellcheck disable=SC2016 # 検索する文字列そのもの（展開しない）
snap_copies="$(grep -cF -e 'head -c $((lock_max_bytes + 1)) -- "$1" > "${snap}"' "${SECRET_SCAN}" || true)"
if [ "${snap_reads}" = 0 ] && [ "${snap_copies}" = 1 ]; then
  pass "secret-scan: the list is read from a single snapshot"
else
  fail "secret-scan: the list is read from a single snapshot" "direct reads: ${snap_reads}, copies: ${snap_copies}"
fi

# HEAD のコミット ID は最初に 1 回だけ確定し、ls-tree・cat-file・staged の差分の基準に同じ ID を渡す（#6011）。
# git の呼び出しの引数を記録し、`HEAD` の名前が残らず、3 つとも同じ ID であることを確かめる
same_checkout_repo "${lock_ok}"
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
stage src/app.txt "x = ${token}"
head_sha="$(git rev-parse HEAD)"
: > "${work_dir}/gitlog"
LOG_GIT_FILE="${work_dir}/gitlog" PATH="${shim_dir}:${PATH}" SCAN_BIN="${same_bin}" check_scan "same checkout: the pinned HEAD id does not disturb a scan" 1 src/app.txt
pin_ok=1
for sub in "ls-tree" "cat-file" "diff"; do
  grep -E "(^| )${sub}( |$)" "${work_dir}/gitlog" > "${work_dir}/gitlog-sub" || true
  # cat-file は index の一覧の blob（オブジェクト ID だけを渡す）も読むため、HEAD の一覧を読む呼び出し（`<ID>:skills-lock.json`）だけを見る
  if [ "${sub}" = cat-file ]; then
    grep -F ":skills-lock.json" "${work_dir}/gitlog-sub" > "${work_dir}/gitlog-sub2" || true
    mv "${work_dir}/gitlog-sub2" "${work_dir}/gitlog-sub"
  fi
  # index の比較・intent-to-add の判定の diff（--ita-*-in-index）は HEAD ではなく空の tree と比べる。空の tree の ID を
  # 渡していることを確かめてから、HEAD の ID の照合（走査の diff）から外す。index の比較は固定時と走査後の 2 回ある
  if [ "${sub}" = diff ]; then
    empty_sha="$(git hash-object -t tree /dev/null)"
    grep -E -- "--ita-(in)?visible-in-index" "${work_dir}/gitlog-sub" > "${work_dir}/gitlog-ita" || true
    ita_total="$(wc -l < "${work_dir}/gitlog-ita")"
    ita_total="${ita_total//[[:space:]]/}"
    ita_with_empty="$(grep -c "${empty_sha}" "${work_dir}/gitlog-ita" || true)"
    if [ "${ita_total}" -lt 2 ] || [ "${ita_total}" -ne "${ita_with_empty}" ]; then
      pin_ok=0
    fi
    grep -vE -- "--ita-(in)?visible-in-index" "${work_dir}/gitlog-sub" > "${work_dir}/gitlog-sub2" || true
    mv "${work_dir}/gitlog-sub2" "${work_dir}/gitlog-sub"
  fi
  sub_total="$(wc -l < "${work_dir}/gitlog-sub")"
  sub_total="${sub_total//[[:space:]]/}"
  sub_with_id="$(grep -c "${head_sha}" "${work_dir}/gitlog-sub" || true)"
  # 名前の一覧と本文の 2 回の diff を含め、すべての呼び出しが同じ ID を渡す（片方が基準を落とすと検出する）
  if [ "${sub_total}" -lt 1 ] || [ "${sub_total}" -ne "${sub_with_id}" ]; then
    pin_ok=0
  fi
  # どの呼び出しにも、解決し直される `HEAD` の名前が残らない
  if grep -Eq "(^| )HEAD(:| |$)" "${work_dir}/gitlog-sub"; then
    pin_ok=0
  fi
  if [ "${sub}" = diff ] && [ "${sub_total}" -lt 2 ]; then
    pin_ok=0
  fi
done
if [ "${pin_ok}" -eq 1 ]; then
  pass "secret-scan: ls-tree, cat-file and the staged diff all use the same pinned HEAD commit id"
else
  fail "secret-scan: ls-tree, cat-file and the staged diff all use the same pinned HEAD commit id" "git calls: $(cat "${work_dir}/gitlog")"
fi

# 走査中に HEAD が動いたら、古い基準の結果で合格させない（2）。本文の diff（-U0）の直前に動かす
same_checkout_repo "${lock_ok}"
git commit -q --allow-empty -m "chore: second"
second_sha="$(git rev-parse HEAD)"
git reset -q --hard HEAD~1
stage src/app.txt "x = placeholder"
rm -f "${work_dir}/moved"
move_rc=0
MOVE_HEAD_ON_ARG=-U0 MOVE_HEAD_TO="${second_sha}" MOVE_HEAD_FLAG="${work_dir}/moved" PATH="${shim_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || move_rc=$?
if [ "${move_rc}" -eq 2 ] && grep -qxF "secret-scan: HEAD changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: HEAD moving during a staged scan is a scan failure (2)"
else
  fail "secret-scan: HEAD moving during a staged scan is a scan failure (2)" "got ${move_rc}" "output: $(cat "${work_dir}/out")"
fi

# 未作成ブランチのまま始まり、走査中にブランチが作られた場合も 2
new_repo
git branch other
other_sha="$(git rev-parse other)"
git symbolic-ref HEAD refs/heads/unborn
stage src/app.txt "x = placeholder"
rm -f "${work_dir}/moved"
move_rc=0
MOVE_HEAD_ON_ARG=-U0 MOVE_HEAD_TO="${other_sha}" MOVE_HEAD_FLAG="${work_dir}/moved" PATH="${shim_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_ok}" > "${work_dir}/out" 2>&1 || move_rc=$?
if [ "${move_rc}" -eq 2 ] && grep -qxF "secret-scan: HEAD changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: an unborn branch created during a staged scan is a scan failure (2)"
else
  fail "secret-scan: an unborn branch created during a staged scan is a scan failure (2)" "got ${move_rc}" "output: $(cat "${work_dir}/out")"
fi

# 走査中に元の index が変わったら、固定した複写に基づく結果で合格させない（2）。本文の diff（-U0）の直前に git add する
# run_index_mutation <説明> <走査中に add するパス>
run_index_mutation() {
  local rc=0
  rm -f "${work_dir}/mutated"
  MUTATE_INDEX_ON_ARG=-U0 MUTATE_INDEX_ADD="$2" MUTATE_INDEX_FLAG="${work_dir}/mutated" PATH="${shim_dir}:${PATH}" \
    LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ -e "${work_dir}/mutated" ] && [ "${rc}" -eq 2 ] \
    && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
    pass "secret-scan: $1"
  else
    fail "secret-scan: $1" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
}
new_repo; stage src/app.txt "hello"; printf '%s\n' "x = placeholder" > other.txt
run_index_mutation "a file staged during the scan is a scan failure (2)" other.txt
# intent-to-add の空ファイルを走査中に実際にステージしても 2。ls-files -s では両者が同じ行になるため、印の変化も比べる（#6011）
new_repo; stage src/app.txt "hello"; : > .env; git add -N .env
check_scan "an intent-to-add .env is not part of the commit" 0
run_index_mutation "an intent-to-add file really staged during the scan is a scan failure (2)" .env
# 走査後の比較は、元の index を 1 回だけ複写し、ls-files -s と intent-to-add の印の 2 つの一覧を同じ複写から取る（#6011）。
# 元の index を別々に開くと、2 つの呼び出しの間の更新で一覧が別の版を見る。本文の diff（-U0）より後の 2 つの呼び出しが、
# 同じ index のファイルを読み、それが元の index でも固定時の複写（本文の diff が読むもの）でもないことを確かめる
# 検出器の一時ディレクトリの親（TMPDIR）を既知の場所にし、パスは実パス（pwd -P）に直して比べる（macOS の /var と /private/var）
new_repo; stage src/app.txt "hello"
scan_tmp="${work_dir}/scan-tmp"
mkdir -p "${scan_tmp}"
scan_tmp_real="$(cd "${scan_tmp}" && pwd -P)"
orig_idx="$(cd "$(git rev-parse --absolute-git-dir)" && pwd -P)/index"
: > "${work_dir}/gitindex"
TMPDIR="${scan_tmp}" LOG_GIT_INDEX="${work_dir}/gitindex" PATH="${shim_dir}:${PATH}" check_scan "the post-scan index comparison runs on a fresh copy" 0
body_idx=""
after_ls=""
after_ita=""
seen_body=0
while IFS=$'\t' read -r idx args; do
  if [ "${seen_body}" -eq 0 ]; then
    case " ${args} " in
      *" -U0 "*)
        seen_body=1
        body_idx="${idx}"
        ;;
    esac
    continue
  fi
  case " ${args} " in
    *" ls-files -s -z "*) after_ls="${after_ls}${idx}"$'\n' ;;
    *" --ita-invisible-in-index "*) after_ita="${after_ita}${idx}"$'\n' ;;
  esac
done < "${work_dir}/gitindex"
# 記録したパスの TMPDIR の部分を実パスに置き換える（一時ディレクトリは走査の後に消えるため、親だけを正規化する）
after_one="${after_ls%$'\n'}"
after_real="${after_one}"
case "${after_one}" in
  "${scan_tmp}"/*) after_real="${scan_tmp_real}/${after_one#"${scan_tmp}"/}" ;;
esac
body_dir="${body_idx%/*}"
if [ -n "${body_idx}" ] && [ "${after_ls}" = "${after_ita}" ] && [ "$(printf '%s' "${after_ls}" | wc -l | tr -d ' ')" = 1 ] \
  && [ "${body_idx}" = "${body_dir}/index-snap" ] && [ "${after_one}" = "${body_dir}/index-after" ] \
  && [[ "${after_real}" == "${scan_tmp_real}"/secret-scan.*/index-after ]] \
  && [ "${after_real}" != "${orig_idx}" ] && [ "${after_one}" != "${body_idx}" ]; then
  pass "secret-scan: both post-scan listings read the same fresh copy of the index"
else
  fail "secret-scan: both post-scan listings read the same fresh copy of the index" "git calls: $(cat "${work_dir}/gitindex")"
fi

# 固定時にあった元の index が走査中に消えたら、一覧の差か有無の記録（index_pinned）のどちらでも 2（#6011）
new_repo; stage src/app.txt "hello"
rm -f "${work_dir}/deleted"
rc=0
DELETE_INDEX_ON_ARG=-U0 DELETE_INDEX_PATH="$(git rev-parse --path-format=absolute --git-path index)" DELETE_INDEX_FLAG="${work_dir}/deleted" \
  PATH="${shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/deleted" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: the index removed during the scan is a scan failure (2)"
else
  fail "secret-scan: the index removed during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 終了時の元の index の複写（index-after への cp）が失敗したら 2。固定時の複写（index-snap）は通す（#6011）
cp_shim_dir="${work_dir}/cp-shim"
mkdir -p "${cp_shim_dir}"
REAL_CP="$(command -v cp)"
export REAL_CP
cat > "${cp_shim_dir}/cp" <<'EOF2'
#!/bin/sh
for last in "$@"; do :; done
case "${last}" in
  */index-after) : > "${CP_SHIM_FLAG}"; exit 1 ;;
esac
exec "${REAL_CP}" "$@"
EOF2
chmod +x "${cp_shim_dir}/cp"
new_repo; stage src/app.txt "hello"
rm -f "${work_dir}/cp-failed"
rc=0
CP_SHIM_FLAG="${work_dir}/cp-failed" PATH="${cp_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/cp-failed" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: a failure to copy the index at the end of the scan is a scan failure (2)"
else
  fail "secret-scan: a failure to copy the index at the end of the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 登録 0 件の index でも、固定時にあった index が終了時に無ければ 2（一覧の差だけで判定すると空のまま変わらず 0 になる。#6011）
new_repo
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
if [ -f "${idx_path}" ] && [ -z "$(git ls-files)" ]; then
  pass "secret-scan: precondition: the index exists and registers no file"
else
  fail "secret-scan: precondition: the index exists and registers no file"
fi
rm -f "${work_dir}/deleted"
rc=0
DELETE_INDEX_ON_ARG=-U0 DELETE_INDEX_PATH="${idx_path}" DELETE_INDEX_FLAG="${work_dir}/deleted" \
  PATH="${shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/deleted" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index with no registrations removed during the scan is a scan failure (2)"
else
  fail "secret-scan: an index with no registrations removed during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# index が通常ファイルでない（FIFO）ときは、複写せず（cp が読み手の到着待ちで止まらない）2。開始時・走査中の差し替えの両方。
# 退行すると止まり続けるため、背景で実行して 10 秒で打ち切り、打ち切りは失敗にする
# run_bounded <コマンド...>: 出力は ${work_dir}/out に残し、終了コードを返す（打ち切りは 99）
run_bounded() {
  local pid i
  "$@" > "${work_dir}/out" 2>&1 &
  pid=$!
  for ((i = 0; i < 100; i++)); do
    kill -0 "${pid}" 2> /dev/null || break
    sleep 0.1
  done
  if kill -0 "${pid}" 2> /dev/null; then
    # 子プロセス（FIFO の open で止まった cp 等）も止める。pkill が無い環境では親だけ（残りは終了時に手で確認する）
    if command -v pkill > /dev/null 2>&1; then
      pkill -KILL -P "${pid}" 2> /dev/null || true
    fi
    kill -9 "${pid}" 2> /dev/null || true
    wait "${pid}" 2> /dev/null || true
    return 99
  fi
  wait "${pid}"
}
new_repo; stage src/app.txt "hello"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
rm -f -- "${idx_path}"
mkfifo "${idx_path}"
rc=0
LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index that is a FIFO at the start is a scan failure (2) without blocking"
else
  fail "secret-scan: an index that is a FIFO at the start is a scan failure (2) without blocking" "got ${rc} (99 = timed out)" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}"

# 開始時の index がシンボリックリンクなら、辿らず 2（実体に解いて固定すると、走査中のリンクの付け替えに気づけない）。
# 以前は「実体が通常ファイルなら走査する（秘密値があれば 1）」を期待していた。リンクなら 2 へ強める向きの変更（AUDB-6 に当たらない）
new_repo; stage leak.txt "x = ${token}"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
mv -- "${idx_path}" "${idx_path}.real"
ln -s "${idx_path}.real" "${idx_path}"
rc=0
LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index that is a symlink (to a regular file) at the start is a scan failure (2)"
else
  fail "secret-scan: an index that is a symlink (to a regular file) at the start is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}" "${idx_path}.real"
# サブディレクトリから起動しても同じ（解く前のパスは作業ツリーのルート基準で見る）
new_repo; stage leak.txt "x = ${token}"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
mv -- "${idx_path}" "${idx_path}.real"
ln -s "${idx_path}.real" "${idx_path}"
mkdir -p sub
rc=0
(cd sub && LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}") || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index that is a symlink is a scan failure (2) from a subdirectory too"
else
  fail "secret-scan: an index that is a symlink is a scan failure (2) from a subdirectory too" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}" "${idx_path}.real"
# 走査中にリンクへ替わった場合も 2（終了時の確認）
new_repo; stage src/app.txt "hello"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
cp -- "${idx_path}" "${work_dir}/swap-target-index"
rm -f "${work_dir}/deleted"
rc=0
DELETE_INDEX_SYMLINK_TO="${work_dir}/swap-target-index" DELETE_INDEX_ON_ARG=-U0 DELETE_INDEX_PATH="${idx_path}" DELETE_INDEX_FLAG="${work_dir}/deleted" \
  PATH="${shim_dir}:${PATH}" LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
if [ -e "${work_dir}/deleted" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index replaced by a symlink during the scan is a scan failure (2)"
else
  fail "secret-scan: an index replaced by a symlink during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}"
new_repo; stage src/app.txt "hello"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
rm -f -- "${idx_path}"
mkfifo "${idx_path}.fifo"
ln -s "${idx_path}.fifo" "${idx_path}"
rc=0
LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index that is a symlink to a FIFO is a scan failure (2) without blocking"
else
  fail "secret-scan: an index that is a symlink to a FIFO is a scan failure (2) without blocking" "got ${rc} (99 = timed out)" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}" "${idx_path}.fifo"

new_repo; stage src/app.txt "hello"
idx_path="$(git rev-parse --path-format=absolute --git-path index)"
rm -f "${work_dir}/deleted"
rc=0
DELETE_INDEX_MKFIFO=1 DELETE_INDEX_ON_ARG=-U0 DELETE_INDEX_PATH="${idx_path}" DELETE_INDEX_FLAG="${work_dir}/deleted" \
  PATH="${shim_dir}:${PATH}" LC_ALL="${utf8_locale}" run_bounded "${HOOK_BASH}" "${SECRET_SCAN}" || rc=$?
if [ -e "${work_dir}/deleted" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index replaced by a FIFO during the scan is a scan failure (2) without blocking"
else
  fail "secret-scan: an index replaced by a FIFO during the scan is a scan failure (2) without blocking" "got ${rc} (99 = timed out)" "output: $(cat "${work_dir}/out")"
fi
rm -f -- "${idx_path}"

# 外部コマンドは使う前に確かめ、欠けていれば 2（固定の診断文）。使うコマンドの一覧は検出器の確認ループと同じ。
# 一覧にあるコマンドだけを置いた PATH で通常の走査が通る（= 一覧に漏れが無い。同じ checkout・リンク経由の起動で readlink・head・sleep も踏む）
req_cmds="git readlink mktemp rm cp head wc sleep tr cmp"
# make_minpath <ディレクトリ> [置かないコマンド]
make_minpath() {
  local c
  mkdir -p "$1"
  for c in ${req_cmds}; do
    [ "${c}" = "${2:-}" ] || ln -s "$(command -v "${c}")" "$1/${c}"
  done
}
same_checkout_repo "${lock_ok}"; stage src/app.txt "hello"
ln -s scripts/hooks/secret-scan.sh sslink-min
make_minpath "${work_dir}/minpath-all"
rc=0
PATH="${work_dir}/minpath-all" LC_ALL="${utf8_locale}" "${HOOK_BASH}" sslink-min > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: a PATH holding only the required commands passes a scan (the list is complete)"
else
  fail "secret-scan: a PATH holding only the required commands passes a scan (the list is complete)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
for missing_cmd in ${req_cmds}; do
  make_minpath "${work_dir}/minpath-no-${missing_cmd}" "${missing_cmd}"
  rc=0
  PATH="${work_dir}/minpath-no-${missing_cmd}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" sslink-min > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: required command not found on PATH: ${missing_cmd}; aborting" "${work_dir}/out"; then
    pass "secret-scan: a missing ${missing_cmd} is a scan failure (2) with a fixed message"
  else
    fail "secret-scan: a missing ${missing_cmd} is a scan failure (2) with a fixed message" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done
# --summary でも診断は出る（fd 4）
rc=0
PATH="${work_dir}/minpath-no-sleep" LC_ALL="${utf8_locale}" "${HOOK_BASH}" sslink-min --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: required command not found on PATH: sleep; aborting" "${work_dir}/out"; then
  pass "secret-scan: a missing command is reported under --summary"
else
  fail "secret-scan: a missing command is reported under --summary" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 一覧の読み取りの失敗は、どちらの一覧かで固定の文言を分ける（パス・オブジェクト指定は出さない）。
# index 側は blob の oid だけを渡して cat-file を失敗させ、HEAD 側は `<コミット>:skills-lock.json` の形で失敗させる
new_repo; stage_lock
lock_oid="$(git hash-object skills-lock.json)"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG="${lock_oid}" SCAN_BIN="${det_ok}" check_scan "a failure to read the staged list exits 2" 2
expect_out "a staged-list read failure reports the staged message" "secret-scan: cannot read the staged skills-lock.json; aborting"
if [[ "${last_out}" == *committed* ]]; then
  fail "secret-scan: a staged-list read failure does not mention the committed list" "output: ${last_out}"
else
  pass "secret-scan: a staged-list read failure does not mention the committed list"
fi
# cat-file blob（本文の取得）の失敗も、同じく側ごとの文言にする（大きさの取得 cat-file -s は通す）
new_repo; stage_lock
PATH="${shim_dir}:${PATH}" FAIL_GIT_BOTH1=blob FAIL_GIT_BOTH2="${lock_oid}" SCAN_BIN="${det_ok}" check_scan "a failure to fetch the staged list body exits 2" 2
expect_out "a staged-list body failure reports the staged message" "secret-scan: cannot read the staged skills-lock.json; aborting"
same_checkout_repo "${lock_ok}"; stage src/app.txt "hello"
PATH="${shim_dir}:${PATH}" FAIL_GIT_BOTH1=blob FAIL_GIT_BOTH2="$(git rev-parse HEAD):skills-lock.json" SCAN_BIN="${same_bin}" check_scan "a failure to fetch the committed list body exits 2" 2
expect_out "a committed-list body failure reports the committed message" "secret-scan: cannot read the committed skills-lock.json; aborting"
same_checkout_repo "${lock_ok}"; stage src/app.txt "hello"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG="$(git rev-parse HEAD):skills-lock.json" SCAN_BIN="${same_bin}" check_scan "a failure to read the committed list exits 2" 2
expect_out "a committed-list read failure reports the committed message" "secret-scan: cannot read the committed skills-lock.json; aborting"

# 相対パスの GIT_INDEX_FILE は、git と同じく作業ツリーのルートを基準に読む（起動時の作業ディレクトリではない。#6011）。
# サブディレクトリから GIT_INDEX_FILE=sub/alt で起動すると、git は <root>/sub/alt を読む。起動時の作業ディレクトリを基準にした
# <root>/sub/sub/alt には秘密値の無い index を置き、取り違えると 0 になる形にする
new_repo; stage src/app.txt "hello"; git commit -q -m "chore: base"
mkdir -p sub/sub
cp "$(git rev-parse --path-format=absolute --git-path index)" sub/sub/alt
cp "$(git rev-parse --path-format=absolute --git-path index)" sub/alt
printf '%s\n' "x = ${token}" > leak.txt
GIT_INDEX_FILE="${repo}/sub/alt" git add leak.txt
rm -f leak.txt
cd sub
git_view="$(GIT_INDEX_FILE=sub/alt git diff --cached --name-only HEAD)"
if [ "${git_view}" = "leak.txt" ]; then
  pass "secret-scan: git reads a relative GIT_INDEX_FILE from the work tree root"
else
  fail "secret-scan: git reads a relative GIT_INDEX_FILE from the work tree root" "git sees: ${git_view}"
fi
GIT_INDEX_FILE=sub/alt check_scan "a relative GIT_INDEX_FILE from a subdirectory pins the index git uses" 1 leak.txt
cd "${repo}"

# 一覧の大きさの上限（作業ツリー側・HEAD 側・境界）。上限は検出器の lock_max_bytes（暫定 64 KiB）
lock_max="$(sed -n 's/^lock_max_bytes=\([0-9][0-9]*\)$/\1/p' "${SECRET_SCAN}")"
# padded_lock <総バイト数>: 正しい JSON に末尾の空白を詰めて指定の大きさにする
padded_lock() {
  local base pad
  base='{"version":2,"skills":{"vendored-a":{"source":"x"}}}'
  pad=$(($1 - ${#base}))
  printf '%s' "${base}"
  head -c "${pad}" /dev/zero | tr '\000' ' '
}
run_big_lock() {
  local desc="$1" rc=0
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "$2" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: skills-lock.json is too large; aborting" "${work_dir}/out" \
    && grep_none "vendored\|/" "${work_dir}/out"; then
    pass "secret-scan: ${desc}"
  else
    fail "secret-scan: ${desc}" "expected exit 2 with a fixed message, got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
}
make_detector "big" "x"
padded_lock $((lock_max + 1)) > "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
new_repo; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
run_big_lock "skills-lock.json one byte over the size limit is rejected" "${det_bin}"
make_detector "edge" "x"
padded_lock "${lock_max}" > "${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_bin}" check_scan "skills-lock.json exactly at the size limit is accepted" 0
# 解析は小さな窓を足しながら読むため、窓（1024 バイト）をまたぐ長い文字列・数・空白・エスケープを受理・拒否することを確かめる（#6011）
# rep_chars <文字> <個数>: 指定の文字を個数だけ並べる
# 個数 0 は何も出さない（BSD の head は `-c 0` を "illegal byte count" で拒否する。macOS の CI で落ちた。#6011）。
# 負の個数・数字以外は、計算の誤りを見逃さないよう診断を出して終了コード 2 で戻す（呼び出しは代入のため set -e で止まる）。
rep_chars() {
  case "$2" in
    '' | *[!0-9]*)
      echo "test-hooks: rep_chars: invalid count: $2" >&2
      return 2
      ;;
  esac
  if [ "$2" -eq 0 ]; then
    return 0
  fi
  head -c "$2" /dev/zero | tr '\000' "$1"
}
long_a="$(rep_chars a 3000)"
long_d="$(rep_chars 1 200)"
long_ws="$(rep_chars ' ' 3000)"
window_lock="{${long_ws}\"version\":2,\"skills\":{\"vendored-a\":{\"k\":\"${long_a}\\n${long_a}\\u00e9${long_a}\",\"n\":-0.${long_d}e+${long_d},\"l\":[true,false,null,${long_d}]${long_ws}},${long_ws}\"vendored-b\":{}}}${long_ws}"
make_detector "window" "${window_lock}"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_bin}" check_scan "skills-lock.json with strings, numbers and spaces longer than the read window is accepted" 0
# 閉じていない長い文字列・不正なエスケープ・後ろに文字が続く長い数は、窓をまたいでも拒否する
check_bad_lock "skills-lock.json with a long unterminated string is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":\"${long_a}}}}"
check_bad_lock "skills-lock.json with an invalid escape after a long string is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":\"${long_a}\\x\"}}}"
check_bad_lock "skills-lock.json with a short unicode escape after a long string is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":\"${long_a}\\u12\"}}}"
check_bad_lock "skills-lock.json with a long number followed by a letter is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":${long_d}x}}}"
check_bad_lock "skills-lock.json with a long number followed by a bare exponent is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":${long_d}e}}}"
check_bad_lock "skills-lock.json with a trailing garbage after long spaces is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{}}}${long_ws}x"
# 窓の境目の総当たり: 先頭の空白を 0〜70 バイトずつずらし、次の字句（先頭が 985〜1055 バイト目）が 1024 バイトの境目をまたぐ位置をすべて通す。
# 窓の大きさは変えず、入力の側をずらす。受理するはずのものは受理（0）、拒否するはずのものは固定の文言・終了コード 2 になること。
# 1 件ごとに検出器を 1 回走らせるため、1 つの検出器と 1 つの repo を使い回し、結果は字句ごとに 1 件の試験にまとめる。
make_detector "brute" "x"
brute_lock_path="${det_bin%/scripts/hooks/secret-scan.sh}/skills-lock.json"
new_repo; stage src/app.txt "hello"
brute_head='{"version":2,"skills":{"vendored-a":{"k":'
brute_total=0
# brute_sweep <説明> <期待する終了コード> <種別: str|raw> <字句> <字句の後ろ>
# str: "k":"<a を並べて字句を境目の付近に置く><字句><字句の後ろ>   raw: "k":<空白を並べて字句を境目の付近に置く><字句><字句の後ろ>
brute_sweep() {
  local desc="$1" want="$2" kind="$3" tok="$4" tail="$5" n rc fill_len bad="" cnt=0 lead body
  for n in $(seq 0 70); do
    lead="$(rep_chars ' ' "${n}")"
    if [ "${kind}" = str ]; then
      fill_len=$((985 - ${#brute_head} - 1))
      body="\"$(rep_chars a "${fill_len}")${tok}${tail}"
    else
      fill_len=$((985 - ${#brute_head}))
      body="$(rep_chars ' ' "${fill_len}")${tok}${tail}"
    fi
    printf '%s' "${lead}${brute_head}${body}}}}" > "${brute_lock_path}"
    rc=0
    LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_bin}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
    cnt=$((cnt + 1))
    brute_total=$((brute_total + 1))
    if [ "${rc}" -ne "${want}" ]; then
      bad="${bad} ${n}"
    elif [ "${want}" -eq 2 ] && ! grep -qxF "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting" "${work_dir}/out"; then
      bad="${bad} ${n}"
    fi
  done
  # 試行の回数（0〜70 の 71 位置）が欠けていないことを確かめる（ループの誤りで黙って件数が減るのを防ぐ）。
  if [ "${cnt}" -ne 71 ]; then
    fail "secret-scan: window boundary sweep: ${desc}" "expected 71 offsets, ran ${cnt}"
  elif [ -z "${bad}" ]; then
    pass "secret-scan: window boundary sweep (${cnt} offsets): ${desc}"
  else
    fail "secret-scan: window boundary sweep: ${desc}" "expected exit ${want} at leading-space counts:${bad}"
  fi
}
brute_start="${SECONDS}"
brute_sweep "an escaped quote is accepted" 0 str '\"x' '"'
brute_sweep "a multibyte character is accepted" 0 str 'éx' '"'
brute_sweep "a unicode escape is accepted" 0 str '\u00e9x' '"'
brute_sweep "a closing quote is accepted" 0 str '' '"'
brute_sweep "a number with a signed exponent is accepted" 0 raw '1e+5' ''
brute_sweep "a number with a fraction is accepted" 0 raw '1.5' ''
brute_sweep "a literal true is accepted" 0 raw 'true' ''
brute_sweep "an invalid escape is rejected" 2 str '\x' 'y"'
brute_sweep "a short unicode escape is rejected" 2 str '\u12' '"'
brute_sweep "a bare exponent is rejected" 2 raw '1e+' ''
brute_sweep "a fraction without digits is rejected" 2 raw '1.' ''
brute_sweep "a truncated literal is rejected" 2 raw 'tru' ''
# 総当たり全体の回数（12 字句 × 71 位置）も確かめる（字句の行が黙って欠けるのを防ぐ）。
if [ "${brute_total}" -ne 852 ]; then
  fail "secret-scan: window boundary sweep total" "expected 852 detector runs, ran ${brute_total}"
fi
printf '# window boundary sweep: %s detector runs, %s seconds\n' "${brute_total}" "$((SECONDS - brute_start))"
# 深さの上限: 配列で入れ子にした先の {"k":1} の深さが 31 なら受理、32 なら拒否（値は深さ + 1 で読むため。1 段の object の早道も同じ上限）
depth_lock() {
  printf '{"version":2,"skills":{"vendored-a":{"k":%s{"k":1}%s}}}' "$(rep_chars '[' "$1")" "$(rep_chars ']' "$1")"
}
make_detector "depth31" "$(depth_lock 28)"
new_repo; stage src/app.txt "hello"
SCAN_BIN="${det_bin}" SCAN_ARG=--tracked check_scan "skills-lock.json with an object at depth 31 is accepted" 0
check_bad_lock "skills-lock.json with an object at depth 32 is rejected" "$(depth_lock 29)"
# 1 段の object の早道に当たる形でも、不正な値は拒否する
check_bad_lock "skills-lock.json with an uppercase literal is rejected" '{"version":2,"skills":{"vendored-a":{"k":TRUE}}}'
check_bad_lock "skills-lock.json with a leading-zero number in a flat object is rejected" '{"version":2,"skills":{"vendored-a":{"k":01}}}'
check_bad_lock "skills-lock.json with a fraction without digits in a flat object is rejected" '{"version":2,"skills":{"vendored-a":{"k":1.}}}'
check_bad_lock "skills-lock.json with a bare minus in a flat object is rejected" '{"version":2,"skills":{"vendored-a":{"k":-}}}'
check_bad_lock "skills-lock.json with a bare exponent in a flat object is rejected" '{"version":2,"skills":{"vendored-a":{"k":1e}}}'
check_bad_lock "skills-lock.json with a missing colon in a flat object is rejected" '{"version":2,"skills":{"vendored-a":{"k" 1}}}'
# 作業ツリー・index・HEAD の一覧が同じ内容なら解析結果を再利用するが、内容が違えば版ごとに解析する（除外は共通部分だけ）
same_checkout_repo "${lock_ok}"
printf '%s\n' '{"version":2,"skills":{"vendored-a":{"source":"example/skills"}}}' > skills-lock.json
git add skills-lock.json
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
stage .agents/skills/vendored-b/sub/n.md "${dsn_line}"
SCAN_BIN="${same_bin}" check_scan "an index list that differs from the working tree list is parsed separately and narrows the exclusions" 1 .agents/skills/vendored-b/sub/n.md
# HEAD 側だけが上限を超える（同じ checkout。作業ツリーの一覧は小さい）
same_checkout_repo "${lock_ok}"
padded_lock $((lock_max + 1)) > skills-lock.json
git add skills-lock.json
git commit -q -m "chore: big lock"
printf '%s\n' "${lock_ok}" > skills-lock.json
stage src/app.txt "x = placeholder"
run_big_lock "the committed skills-lock.json over the size limit is rejected" "${same_bin}"

# 別の checkout の staged でも、差分の基準は確定した ID を使う
new_repo; stage src/app.txt "x = ${token}"
head_sha="$(git rev-parse HEAD)"
: > "${work_dir}/gitlog"
LOG_GIT_FILE="${work_dir}/gitlog" PATH="${shim_dir}:${PATH}" SCAN_BIN="${det_ok}" check_scan "staged: a scan from another checkout succeeds with the pinned id" 1 src/app.txt
if grep -E "(^| )diff( |$)" "${work_dir}/gitlog" | grep -q "${head_sha}"; then
  pass "secret-scan: the staged diff of another checkout uses the pinned HEAD commit id"
else
  fail "secret-scan: the staged diff of another checkout uses the pinned HEAD commit id" "git calls: $(cat "${work_dir}/gitlog")"
fi

# 未作成ブランチの staged の差分は、空の tree の ID を明示して固定する（名前の検査と本文の検査の 2 回とも）
new_repo
git symbolic-ref HEAD refs/heads/unborn
stage src/app.txt "x = ${token}"
: > "${work_dir}/gitlog"
LOG_GIT_FILE="${work_dir}/gitlog" PATH="${shim_dir}:${PATH}" SCAN_BIN="${det_ok}" check_scan "staged: an unborn branch is scanned against the empty tree" 1 src/app.txt
empty_id="$(git hash-object -t tree --stdin < /dev/null)"
# index の比較の diff（--ita-*-in-index。常に空の tree と比べる）は数えず、走査の 2 回の diff だけを見る
grep -E "(^| )diff( |$)" "${work_dir}/gitlog" | grep -vE -- "--ita-(in)?visible-in-index" > "${work_dir}/gitlog-sub" || true
sub_total="$(wc -l < "${work_dir}/gitlog-sub")"
sub_total="${sub_total//[[:space:]]/}"
sub_with_id="$(grep -c "${empty_id}" "${work_dir}/gitlog-sub" || true)"
if [ "${sub_total}" -ge 2 ] && [ "${sub_total}" -eq "${sub_with_id}" ]; then
  pass "secret-scan: both staged diffs of an unborn branch pass the empty tree id"
else
  fail "secret-scan: both staged diffs of an unborn branch pass the empty tree id" "git calls: $(cat "${work_dir}/gitlog")"
fi

# 一覧が通常ファイルでない（FIFO）・リンクのときは、止まらずに 2
same_checkout_repo "${lock_ok}"
rm -f skills-lock.json
stage src/app.txt "x = placeholder"
fifo_dir="."
rm -f "${fifo_dir}/skills-lock.json"
if mkfifo "${fifo_dir}/skills-lock.json" 2> /dev/null; then
  fifo_rc=0
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 < /dev/null || fifo_rc=$?
  if [ "${fifo_rc}" -eq 2 ]; then
    pass "secret-scan: a FIFO in place of the list is rejected without hanging (2)"
  else
    fail "secret-scan: a FIFO in place of the list is rejected without hanging (2)" "got ${fifo_rc}" "output: $(cat "${work_dir}/out")"
  fi
  rm -f "${fifo_dir}/skills-lock.json"
fi

# CDPATH で相対パスの cd が別のディレクトリ（一覧の無いおとり）へ向かっても、検出器の場所を取り違えない。
# repo 側にコミットした正しい一覧が使われれば除外され（0）、CDPATH を効かせると一覧を失って検出になる（1）。
decoy="${work_dir}/decoy"
mkdir -p "${decoy}/scripts/hooks"
same_checkout_repo "${lock_ok}"
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
CDPATH="${decoy}" SCAN_BIN="${same_bin}" check_scan "a CDPATH decoy does not displace the detector location" 0

# 登録名の下でも、env-file の名前の検査は全パスに掛かる
new_repo; stage_lock; stage .agents/skills/vendored-a/.env "KEY=value"
SCAN_BIN="${det_ok}" check_scan "an env file under a registered skill is still reported" 1 .agents/skills/vendored-a/.env

# 同じ checkout で HEAD の一覧が壊れている・読めないときは検査不能（2）。固定の文言だけを出す
same_checkout_repo '{ "version": 2, "skills": {'
printf '%s\n' "${lock_ok}" > skills-lock.json
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${same_bin}" check_scan "same checkout: a broken committed list is rejected" 2
expect_out "same checkout: a broken committed list reports a fixed message" "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting"

same_checkout_repo "${lock_ok}"
stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=ls-tree SCAN_BIN="${same_bin}" check_scan "same checkout: a failure to look up the committed list exits 2" 2
expect_out "same checkout: a failed lookup reports a fixed message" "secret-scan: cannot read the committed skills-lock.json; aborting"
# 固定した index の一覧が先に読まれるため、cat-file をすべて失敗させると最初に失敗するのは index 側（staged の文言。HEAD 側の文言は下の試験で確かめる）
PATH="${shim_dir}:${PATH}" FAIL_GIT_ARG=cat-file SCAN_BIN="${same_bin}" check_scan "same checkout: a failure to read the lists exits 2" 2
expect_out "same checkout: a failed read of the staged list reports a fixed message" "secret-scan: cannot read the staged skills-lock.json; aborting"

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
