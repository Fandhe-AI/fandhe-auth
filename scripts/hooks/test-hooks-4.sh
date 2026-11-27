#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 4（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

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
req_cmds="git readlink mktemp rm cp head wc sleep tr cmp stat awk cat"
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
# 窓（1024 バイト）を何度も補充しないと読み終えない長い数（#6011）
huge_d="$(rep_chars 1 5000)"
huge_f="$(rep_chars 2 3000)"
huge_e="$(rep_chars 3 3000)"
make_detector "hugenum" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":-${huge_d}.${huge_f}e+${huge_e}}}}"
new_repo; stage_lock; stage .agents/skills/vendored-a/SKILL.md "x = ${token}"
SCAN_BIN="${det_bin}" check_scan "skills-lock.json with a number needing many window refills is accepted" 0
check_bad_lock "skills-lock.json with a number needing many window refills followed by a letter is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":${huge_d}x}}}"
check_bad_lock "skills-lock.json with a long number and a bare exponent at the end is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":${huge_d}.${huge_f}e+}}}"
check_bad_lock "skills-lock.json with a long number and a trailing dot is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{\"k\":${huge_d}.}}}"
check_bad_lock "skills-lock.json with a trailing garbage after long spaces is rejected" "{\"version\":2,\"skills\":{\"vendored-a\":{}}}${long_ws}x"
# skills の名前の重複の確認に使う内部の変数と、名前・環境の変数が衝突しない（名前 vars・a など。#6011）
make_detector "seen1" '{"version":2,"skills":{"vars":{"k":1},"a":{},"b":{}}}'
new_repo; stage src/app.txt "hello"
SCAN_BIN="${det_bin}" SCAN_ARG=--tracked check_scan "a skill named vars first is accepted" 0
make_detector "seen2" '{"version":2,"skills":{"a":{},"vars":{},"b":{},"vars-x":{}}}'
SCAN_BIN="${det_bin}" SCAN_ARG=--tracked check_scan "a skill named vars not first is accepted" 0
make_detector "seen3" '{"version":2,"skills":{"a":{}}}'
JSEEN_a=1 JSEEN_vars=1 jseen_a=1 jseen_vars=x SCAN_BIN="${det_bin}" SCAN_ARG=--tracked check_scan "variables inherited from the environment do not affect the duplicate check" 0
# compgen を無効にした bash（programmable completion を外したビルドの再現）でも、継いだ JSEEN_a=1 に影響されない
nocompgen_env="${work_dir}/nocompgen-env.sh"
printf '%s\n' 'enable -n compgen' > "${nocompgen_env}"
JSEEN_a=1 BASH_ENV="${nocompgen_env}" SCAN_BIN="${det_bin}" SCAN_ARG=--tracked check_scan "a JSEEN_ variable inherited from the environment is cleared without compgen" 0
# 同じプロセスで続けて解析する（作業ツリー・index・HEAD の一覧が互いに違う）。前の解析の記録が次の解析に残らない
same_checkout_repo '{"version":2,"skills":{"vars":{},"a":{}}}'
printf '%s\n' '{"version":2,"skills":{"vars":{}}}' > skills-lock.json
git add -- skills-lock.json
printf '%s\n' '{"version":2,"skills":{"a":{},"vars":{},"b":{}}}' > skills-lock.json
stage src/app.txt "hello"
SCAN_BIN="${same_bin}" check_scan "lists parsed one after another in one process do not leak names into each other" 0
# 重複は拒否のまま（vars の重複）
check_bad_lock "skills-lock.json with a duplicate skill named vars is rejected" '{"version":2,"skills":{"vars":{},"a":{},"vars":{}}}'
# 読み取りの 5 秒の打ち切りは、解析の拒否とは別の固定の診断で 2 にする（拒否の試験が、打ち切りでも通ってしまわないように。#6011）。
# 読み取り用の head を、返ってこない shim に差し替えて打ち切りを起こす（5 秒掛かる）
timeout_shim_dir="${work_dir}/timeout-shim"
mkdir -p "${timeout_shim_dir}"
printf '%s\n' '#!/bin/sh' 'exec sleep 30' > "${timeout_shim_dir}/head"
chmod +x "${timeout_shim_dir}/head"
make_detector "timeout" "${lock_ok}"
new_repo; stage src/app.txt "hello"
rc=0
PATH="${timeout_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_bin}" --tracked --summary > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: reading skills-lock.json timed out; aborting" "${work_dir}/out" \
  && ! grep -qF "unreadable or has an unexpected shape" "${work_dir}/out"; then
  pass "secret-scan: a read that does not finish in time exits 2 with its own message (not the parse rejection)"
else
  fail "secret-scan: a read that does not finish in time exits 2 with its own message (not the parse rejection)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
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
  local desc="$1" want="$2" kind="$3" tok="$4" tail="$5" n rc fill_len bad="" bad_out="" cnt=0 lead body
  for ((n = 0; n <= 70; n++)); do
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
      [ -n "${bad_out}" ] || bad_out="exit ${rc}, output: $(cat "${work_dir}/out")"
    elif [ "${want}" -eq 2 ] && ! grep -qxF "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting" "${work_dir}/out"; then
      bad="${bad} ${n}"
      [ -n "${bad_out}" ] || bad_out="exit ${rc}, output: $(cat "${work_dir}/out")"
    fi
  done
  # 試行の回数（0〜70 の 71 位置）が欠けていないことを確かめる（ループの誤りで黙って件数が減るのを防ぐ）。
  if [ "${cnt}" -ne 71 ]; then
    fail "secret-scan: window boundary sweep: ${desc}" "expected 71 offsets, ran ${cnt}"
  elif [ -z "${bad}" ]; then
    pass "secret-scan: window boundary sweep (${cnt} offsets): ${desc}"
  else
    fail "secret-scan: window boundary sweep: ${desc}" "expected exit ${want} at leading-space counts:${bad}" "first failure: ${bad_out}"
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
# 小さい一覧を作業ツリーと index の両方へ戻す（上限を超える版を HEAD だけに残す）。index に大きい一覧が残ると、
# 検出器は HEAD より先に index の一覧を検証して同じ診断で止まり、HEAD 側の検査がなくても通ってしまう（#6011）
printf '%s\n' "${lock_ok}" > skills-lock.json
git add -- skills-lock.json
stage src/app.txt "x = placeholder"
# 大きさは終了コードを確かめた cat-file -s で取る（index に登録が無いと空の出力が 0 に読めて「小さい」が成り立ってしまう）
idx_size=""
head_size=""
if idx_size="$(git cat-file -s :skills-lock.json 2> /dev/null)" && head_size="$(git cat-file -s HEAD:skills-lock.json 2> /dev/null)" \
  && [ "${idx_size}" -gt 0 ] && [ "${idx_size}" -le "${lock_max}" ] && [ "${head_size}" -gt "${lock_max}" ]; then
  pass "secret-scan: precondition: only the committed skills-lock.json is over the size limit"
else
  fail "secret-scan: precondition: only the committed skills-lock.json is over the size limit" "index size: ${idx_size:-none}, HEAD size: ${head_size:-none}"
fi
run_big_lock "the committed skills-lock.json over the size limit is rejected" "${same_bin}"

# リビジョン（空の tree・HEAD のコミット ID）と同名のパスがあっても、引数をパスと取り違えて止まらない（`--` で区切る。#6011）
empty_tree_id="$(git hash-object -t tree --stdin < /dev/null)"
# 中身に検出対象を入れ、同名のパスが黙って走査から外れていないこと（検出して 1・ファイル名が報告に出る）も確かめる
new_repo; stage "${empty_tree_id}" "x = ${token}"
SCAN_ARG=--tracked check_scan "tracked: a path named like the empty tree id does not stop the scan and is still scanned" 1 "${empty_tree_id}"
new_repo; stage "${empty_tree_id}" "x = ${token}"
check_scan "staged: a path named like the empty tree id does not stop the scan and is still scanned" 1 "${empty_tree_id}"
new_repo; stage src/app.txt "hello"; git commit -q -m "chore: base"
stage "$(git rev-parse HEAD)" "x = ${token}"
check_scan "staged: a path named like the HEAD commit id is still scanned" 1 "$(git rev-parse HEAD)"

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
# 作業ツリーと index は有効な一覧にして、拒否の原因を HEAD に限る
printf '%s\n' "${lock_ok}" > skills-lock.json
git add skills-lock.json
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


finish "part 4"
