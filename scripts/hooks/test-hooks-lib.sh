#!/usr/bin/env bash
# test-hooks.sh の各 part（test-hooks-N.sh）が共有する準備・helper・例示値（TASK-6）。
#
# 単体では実行しない。各 part が先頭で source する。使い捨ての作業領域（work_dir）・git の隔離・
# git シム・UTF-8 ロケールの解決・pass / fail の計数・検出器の複製と、例示値（実行時に組み立てる）を持つ。
# 例示値は、本ファイル自体のコミットが secret-scan に検知されないよう、すべて実行時に組み立てる。
# 各 part は別の bash として起動され、状態（変数・関数・cwd）は part の間で引き継がない。
# 計数は finish が TEST_HOOKS_COUNTS_FILE へ書き、入口（test-hooks.sh）が合算する。
#
# 変数・関数は source した側の part が参照するため、単独で未使用に見える定義には、宣言ごとに理由付きの disable を置く。
set -euo pipefail
HOOKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib.sh
source "${HOOKS_DIR}/../lib.sh"

SECRET_SCAN="${HOOKS_DIR}/secret-scan.sh"
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
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
# 権限を落とした試験用ディレクトリ（chmod 000）がシグナルで止まった後も残らないよう、消す前に権限を戻す。
trap 'chmod -R u+rwx "${work_dir}" 2> /dev/null; rm -rf "${work_dir}"' EXIT

# 開発者・CI ランナーの git 設定（署名・色・フック・テンプレート）から隔離する。
# lefthook 経由で起動された場合に備え、親リポジトリを指す環境変数も外す。
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG_PARAMETERS GIT_CONFIG_COUNT
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
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
# MAX_GIT_ARG_BYTES: 各引数の長さ + 1 + ポインタ 8 バイトの合計が上限を超える呼び出しを、macOS の E2BIG の代わりに失敗させる（#6053）
if [ -n "${MAX_GIT_ARG_BYTES:-}" ]; then
  arg_total=0
  for arg in "$@"; do
    arg_total=$((arg_total + ${#arg} + 9))
  done
  if [ "${arg_total}" -gt "${MAX_GIT_ARG_BYTES}" ]; then
    echo "git shim: argument list too long" >&2
    exit 126
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
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
password_line="password = ${q}${password_value}${q}"
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
dsn_line="db: postgres://app:s3cr3tpass${at}db.internal:5432/app"
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
key_line="${dash}BEGIN OPENSSH PRIVATE"" KEY${dash}"
# 出力に値そのものが含まれていないことを確認する対象
leak_needles="${token}
${password_value}
s3cr3tpass"

# 規則表（TASK-6）の例示値。すべての part の check_scan が、これらも出力に漏れていないことを確かめる（実行時に組み立てる）
jwt_value="eyJ""hbGciOiJFUzI1NiJ9.eyJ""zdWIiOiJ1c2VyMSJ9.abcdefghij"
jwk_value="Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij"
recovery_value="Abcd1234-Efgh5678-Ijkl9012"
leak_needles="${leak_needles}
${jwt_value}
${jwk_value}
${recovery_value}"

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
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
det_ok="${det_bin}"
make_detector nolock
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
det_nolock="${det_bin}"


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
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
same_bin="scripts/hooks/secret-scan.sh"

# ---- 複数の part が使う helper（元は各節の中にあった。part の間で関数を引き継がないため、ここに置く）
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

# 1 つの結果を記録する補助（引数: 名前・期待する終了状態・実際の終了状態）
expect_rc() {
  local name="$1" want="$2" got="$3"
  if [ "${got}" -eq "${want}" ]; then
    pass "${name}"
  else
    fail "${name}" "exit ${got} (expected ${want})" "output: $(cat "${work_dir}/out")"
  fi
}

# secret-scan-ci.sh（CI 向けの入口）
# shellcheck disable=SC2034 # part から参照する（本ファイルだけを検査すると未使用に見える）
CI_ENTRY="${HOOKS_DIR}/../secret-scan-ci.sh"

# print_header: 検査対象の bash とロケールを 1 行で出す（part 1 だけが呼ぶ）。
print_header() {
  echo "# hooks under test run with: ${HOOK_BASH} (${BASH_VERSION}), locale ${utf8_locale}"
}

# finish <part の名前>: part の計数を出し、入口へ渡す。失敗があれば 1 で終わる。
# 入口は TEST_HOOKS_COUNTS_FILE に計数が書かれていない part を失敗として扱う（途中で止まった part を成功にしない）。
finish() {
  echo "# $1: passed ${passed}, failed ${failed}"
  if [ -n "${TEST_HOOKS_COUNTS_FILE:-}" ]; then
    echo "${passed} ${failed}" >> "${TEST_HOOKS_COUNTS_FILE}"
  fi
  if [ "${failed}" -ne 0 ]; then
    exit 1
  fi
}
