#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 3（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

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
# 作業ツリーと index は有効な一覧にして、拒否の原因を HEAD に限る
printf '%s\n' "${lock_ok}" > skills-lock.json
git add skills-lock.json
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
# 読み取りの監視（TERM で止める子シェル）は、EXIT の trap（一時ディレクトリの削除）を外した状態で fork し、fork の直後に戻す。
# 起動直後の子に TERM が届くと、子が親の EXIT の trap を実行して走査中の一時ディレクトリを消し、有効な一覧が 2 になる（#6011）。
# 競合は負荷に依って稀にしか起きず実行では安定して再現できないため、形で確かめる（外す → fork → 戻すの順で各 1 か所）。
# 外す行の次の行は子の開き `(` の行に限る（子の中で外す形では、外す前の子が TERM を受けて親の trap を実行し得る）
wd_clear="$(grep -nxE '[[:space:]]*trap - EXIT' "${SECRET_SCAN}" | cut -d: -f1 | tr '\n' ' ' || true)"
wd_open=""
if [[ "${wd_clear}" =~ ^[0-9]+\ $ ]]; then
  wd_open="$(sed -n "$((${wd_clear% } + 1))p" "${SECRET_SCAN}")"
fi
wd_fork="$(grep -nxF '    ) > /dev/null 2>&1 < /dev/null &' "${SECRET_SCAN}" | cut -d: -f1 | tr '\n' ' ' || true)"
wd_restore="$(grep -nxF '    trap cleanup_tmp EXIT' "${SECRET_SCAN}" | cut -d: -f1 | tr '\n' ' ' || true)"
wd_pid="$(grep -nxF '    lock_wd=$!' "${SECRET_SCAN}" | cut -d: -f1 | tr '\n' ' ' || true)"
if [[ "${wd_clear}" =~ ^[0-9]+\ $ ]] && [[ "${wd_fork}" =~ ^[0-9]+\ $ ]] && [[ "${wd_restore}" =~ ^[0-9]+\ $ ]] \
  && [[ "${wd_pid}" =~ ^[0-9]+\ $ ]] && [ "${wd_open}" = "    (" ] && [ "${wd_clear% }" -lt "${wd_fork% }" ] \
  && [ "$((${wd_fork% } + 1))" -eq "${wd_pid% }" ] && [ "$((${wd_pid% } + 1))" -eq "${wd_restore% }" ]; then
  pass "secret-scan: the read watchdog is forked without the EXIT trap and the trap is restored right after"
else
  fail "secret-scan: the read watchdog is forked without the EXIT trap and the trap is restored right after" \
    "clear: ${wd_clear}, next: ${wd_open}, fork: ${wd_fork}, pid: ${wd_pid}, restore: ${wd_restore}"
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
# 走査後の比較は 2 回（#6047）。どちらも元の index を 1 回だけ複写し、ls-files -s と intent-to-add の印の 2 つの一覧を同じ複写から取る（#6011）。
# 1 回目は index-after（HEAD の取り直しの前）、2 回目は index-final（集計の直前）。元の index を別々に開くと、
# 2 つの呼び出しの間の更新で一覧が別の版を見る。本文の diff（-U0）より後の 4 つの呼び出し（2 組）が、各組で同じ index のファイルを読み、
# それが元の index でも固定時の複写（本文の diff が読むもの）でもないことを確かめる
# 検出器の一時ディレクトリの親（TMPDIR）を既知の場所にし、パスは実パス（pwd -P）に直して比べる（macOS の /var と /private/var）
new_repo; stage src/app.txt "hello"
scan_tmp="${work_dir}/scan-tmp"
mkdir -p "${scan_tmp}"
scan_tmp_real="$(cd "${scan_tmp}" && pwd -P)"
orig_idx="$(cd "$(git rev-parse --absolute-git-dir)" && pwd -P)/index"
: > "${work_dir}/gitindex"
TMPDIR="${scan_tmp}" LOG_GIT_INDEX="${work_dir}/gitindex" PATH="${shim_dir}:${PATH}" check_scan "the post-scan index comparison runs on fresh copies" 0
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
first_one="${after_ls%%$'\n'*}"
second_one="${after_ls#*$'\n'}"
second_one="${second_one%$'\n'}"
first_real="${first_one}"
second_real="${second_one}"
case "${first_one}" in
  "${scan_tmp}"/*) first_real="${scan_tmp_real}/${first_one#"${scan_tmp}"/}" ;;
esac
case "${second_one}" in
  "${scan_tmp}"/*) second_real="${scan_tmp_real}/${second_one#"${scan_tmp}"/}" ;;
esac
body_dir="${body_idx%/*}"
if [ -n "${body_idx}" ] && [ "${after_ls}" = "${after_ita}" ] && [ "$(printf '%s' "${after_ls}" | wc -l | tr -d ' ')" = 2 ] \
  && [ "${body_idx}" = "${body_dir}/index-snap" ] && [ "${first_one}" = "${body_dir}/index-after" ] && [ "${second_one}" = "${body_dir}/index-final" ] \
  && [[ "${first_real}" == "${scan_tmp_real}"/secret-scan.*/index-after ]] && [[ "${second_real}" == "${scan_tmp_real}"/secret-scan.*/index-final ]] \
  && [ "${first_real}" != "${orig_idx}" ] && [ "${second_real}" != "${orig_idx}" ] \
  && [ "${first_one}" != "${body_idx}" ] && [ "${second_one}" != "${body_idx}" ]; then
  pass "secret-scan: each pair of post-scan listings reads the same fresh copy of the index"
else
  fail "secret-scan: each pair of post-scan listings reads the same fresh copy of the index" "git calls: $(cat "${work_dir}/gitindex")"
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

# 終了時の index の検証の途中（元の index の複写の直後）に HEAD が動いても、古い基準の結果で合格させない（2）。
# HEAD の取り直しは集計の直前に置く（.claude/rules/git-hooks.md の (f)。index の検証より前に置くと、その後の動きを見落とす。#6011）
cp_head_shim_dir="${work_dir}/cp-head-shim"
mkdir -p "${cp_head_shim_dir}"
cat > "${cp_head_shim_dir}/cp" <<'EOF3'
#!/bin/sh
for last in "$@"; do :; done
"${REAL_CP}" "$@" || exit $?
case "${last}" in
  */index-after)
    if [ ! -e "${CP_HEAD_FLAG}" ]; then
      : > "${CP_HEAD_FLAG}"
      "${REAL_GIT}" update-ref HEAD "${CP_HEAD_TO}"
    fi
    ;;
esac
EOF3
chmod +x "${cp_head_shim_dir}/cp"
same_checkout_repo "${lock_ok}"
git commit -q --allow-empty -m "chore: second"
second_sha="$(git rev-parse HEAD)"
git reset -q --hard HEAD~1
stage src/app.txt "x = placeholder"
rm -f "${work_dir}/head-moved"
rc=0
CP_HEAD_FLAG="${work_dir}/head-moved" CP_HEAD_TO="${second_sha}" PATH="${cp_head_shim_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${same_bin}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/head-moved" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: HEAD changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: HEAD moving during the end-of-scan index check is a scan failure (2)"
else
  fail "secret-scan: HEAD moving during the end-of-scan index check is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# index の親ディレクトリに検索権限が無いときは、未作成（空の index）として扱わず 2（git 自身の解決が Permission denied で失敗するため、開始時に止まる。#6011）。
# ルートは権限を迂回するため、この試験は一般ユーザーのときだけ行い、ルートでは skip を明示する。
if [ "$(id -u)" -ne 0 ]; then
  new_repo; stage src/app.txt "x = ${token}"
  noperm_dir="${work_dir}/noperm-index"
  mkdir -p "${noperm_dir}"
  cp "$(git rev-parse --path-format=absolute --git-path index)" "${noperm_dir}/index"
  chmod 000 "${noperm_dir}"
  rc=0
  GIT_INDEX_FILE="${noperm_dir}/index" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  chmod 755 "${noperm_dir}"
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: an index in an unsearchable directory is a scan failure (2), not an empty index"
  else
    fail "secret-scan: an index in an unsearchable directory is a scan failure (2), not an empty index" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
else
  echo "ok - secret-scan: an index in an unsearchable directory is a scan failure (2): skipped (running as root bypasses permissions)"
fi
# 親が検索可能で index が無いだけなら、従来どおり空として扱う（走査は続く）。権限に関係しないため root でも行う。
# 本物の index には検出対象を stage しておく。git が使うのは GIT_INDEX_FILE の指す（無い）index なので、それでも 0 になる
new_repo; stage src/app.txt "x = ${token}"
rc=0
GIT_INDEX_FILE="${work_dir}/no-such-index" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: a missing index in a searchable directory is still treated as empty"
else
  fail "secret-scan: a missing index in a searchable directory is still treated as empty" "got ${rc}" "output: $(cat "${work_dir}/out")"
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


finish "part 3"
