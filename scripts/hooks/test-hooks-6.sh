#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 6（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

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

# secret-scan-canary.sh: 陰性対照の結果はリポジトリの内容に依らない。作業ツリー・HEAD の skills-lock.json が壊れていても、
# 検出器の複製を一覧の無い場所から動かすため、ダミーの検出（終了コード 0）を確かめられる（#6011）。
# 検出器とスクリプトの複製を一時 repo に置き、壊れた一覧をコミットした上で作業ツリーも壊す
canary_root="${work_dir}/canary-root"
mkdir -p "${canary_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${canary_root}/scripts/"
cp "${SECRET_SCAN}" "${canary_root}/scripts/hooks/secret-scan.sh"
(
  cd "${canary_root}"
  git -c init.defaultBranch=main init -q
  printf '%s\n' '{ "version": 2, "skills": {' > skills-lock.json
  git add -A
  git commit -q -m "chore: broken lock"
  printf '%s\n' 'not json' > skills-lock.json
)
rc=0
(cd "${canary_root}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" scripts/secret-scan-canary.sh) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "scan-canary: a broken skills-lock.json in the repository does not affect the canary"
else
  fail "scan-canary: a broken skills-lock.json in the repository does not affect the canary" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 検出器の実行ビットが無ければ、陰性対照は 2 で終わる（CI の本走査は検出器を直接起動する。#6011）
chmod -x "${canary_root}/scripts/hooks/secret-scan.sh"
rc=0
(cd "${canary_root}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" scripts/secret-scan-canary.sh) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qF "scan-canary: the scanner is not executable" "${work_dir}/out"; then
  pass "scan-canary: a scanner without the execute bit makes the canary exit 2"
else
  fail "scan-canary: a scanner without the execute bit makes the canary exit 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
chmod +x "${canary_root}/scripts/hooks/secret-scan.sh"

# 検出器の 1 行目が `#!/usr/bin/env bash` に完全一致しなければ、実行ビットがあっても陰性対照は 2 で終わる
# （CI の本走査と make secret-scan は検出器を直接起動し、shebang が壊れていると起動できない。陰性対照は複製を bash に
# 読み込ませるため shebang に依らず、確かめないと通ってしまう。#6011）。
# 壊し方は bash として読めば動くもの（解釈系の名前の誤り・CR の付加・別の解釈系のパス）にし、確かめなければ 0 で通る形にする。
# NUL は変数に持てないため、1 行目は printf の書式で直接書く（bash 4 以降の read は NUL を落とし、文字列の比較だけでは通る）。
# 判定は固定の診断と、ERR trap（同じく 2）ではないことの両方で行う
for broken_kind in misspelled cr other-interpreter nul; do
  case "${broken_kind}" in
    misspelled) shebang_fmt='#!/usr/bin/env bsah\n' ;;
    cr) shebang_fmt='#!/usr/bin/env bash\r\n' ;;
    other-interpreter) shebang_fmt='#!/bin/bash\n' ;;
    nul) shebang_fmt='#!/usr/bin/env b\000ash\n' ;;
  esac
  {
    # shellcheck disable=SC2059 # 書式は上の固定の値（NUL・CR を書くため書式として渡す）
    printf "${shebang_fmt}"
    tail -n +2 "${SECRET_SCAN}"
  } > "${canary_root}/scripts/hooks/secret-scan.sh"
  chmod +x "${canary_root}/scripts/hooks/secret-scan.sh"
  rc=0
  (cd "${canary_root}" && LC_ALL="${utf8_locale}" "${HOOK_BASH}" scripts/secret-scan-canary.sh) > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qF "scan-canary: the scanner's first line is not the expected shebang" "${work_dir}/out" \
    && ! grep -qF "internal error" "${work_dir}/out"; then
    pass "scan-canary: a scanner with a broken shebang (${broken_kind}) makes the canary exit 2"
  else
    fail "scan-canary: a scanner with a broken shebang (${broken_kind}) makes the canary exit 2" "exit ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done
unset broken_kind shebang_fmt
cp "${SECRET_SCAN}" "${canary_root}/scripts/hooks/secret-scan.sh"
chmod +x "${canary_root}/scripts/hooks/secret-scan.sh"

# 一時領域が noexec のマウントでも、陰性対照は検出器の複製を起動できる（複製は bash に読み込ませる。#6011）。
# noexec を mount なしで再現するため、複製先の実行ビットを落とす cp の shim を使う（直接起動なら同じく EACCES で起動できない）。
# shim は実行ビットを落とした後に、複製先に実行ビットが無いことを確かめてそのパスを印のファイルへ書く。
# 印で shim が実際に呼ばれ、検出器の複製の実行ビットが落ちた状態で起動されたことを確かめる（shim が効かずに通る形を防ぐ）
noexec_shim_dir="${work_dir}/noexec-cp-shim"
mkdir -p "${noexec_shim_dir}"
real_cp="$(command -v cp)"
# shellcheck disable=SC2016 # 展開させない固定のコード
printf '%s\n' '#!/usr/bin/env bash' \
  '"${REAL_CP}" "$@" || exit $?' \
  'for _last in "$@"; do :; done' \
  'chmod a-x -- "${_last}"' \
  'if [ ! -x "${_last}" ]; then printf "%s\n" "${_last}" >> "${NOEXEC_MARK}"; fi' > "${noexec_shim_dir}/cp"
chmod +x "${noexec_shim_dir}/cp"
noexec_mark="${work_dir}/noexec-mark"
rm -f "${noexec_mark}"
rc=0
(cd "${canary_root}" && NOEXEC_MARK="${noexec_mark}" REAL_CP="${real_cp}" PATH="${noexec_shim_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" scripts/secret-scan-canary.sh) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ -f "${noexec_mark}" ] && grep -qE '/scripts/hooks/secret-scan[.]sh$' "${noexec_mark}"; then
  pass "scan-canary: a copy of the scanner without the execute bit (noexec temp) still runs"
else
  fail "scan-canary: a copy of the scanner without the execute bit (noexec temp) still runs" "exit ${rc}" \
    "shim mark: $(cat "${noexec_mark}" 2> /dev/null || echo none)" "output: $(cat "${work_dir}/out")"
fi

# 陰性対照は検出器の複製を PATH の bash のパス（`type -P`）で起動し、環境から取り込まれた関数 bash（BASH_FUNC_bash%%）に差し替えられない（#6011）。
# 関数は印を残して 0 で返す（関数が使われれば、陰性対照は検出の欠落として 1 で終わる）
bash_fn_mark="${work_dir}/bash-fn-mark"
rm -f "${bash_fn_mark}"
rc=0
(
  cd "${canary_root}"
  # shellcheck disable=SC2329,SC2153 # 子の陰性対照が呼び得る関数（ここでは呼ばない）。印のパスは呼び出し時の環境で渡す
  bash() { : > "${BASH_FN_MARK}"; return 0; }
  export -f bash
  BASH_FN_MARK="${bash_fn_mark}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" scripts/secret-scan-canary.sh
) > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ ! -e "${bash_fn_mark}" ]; then
  pass "scan-canary: an exported bash function does not replace the scanner run"
else
  fail "scan-canary: an exported bash function does not replace the scanner run" "exit ${rc}" \
    "function ran: $([ -e "${bash_fn_mark}" ] && echo yes || echo no)" "output: $(cat "${work_dir}/out")"
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
head_want="$(git -C "${ci_src}" rev-parse HEAD)"
# 案内の試験は、この repo の作業ツリーの状態に左右されないよう、検出器をコミットした複製（det_root）の入口で流す
det_root="${work_dir}/det-root"
mkdir -p "${det_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${det_root}/scripts/"
cp "${SECRET_SCAN}" "${det_root}/scripts/hooks/secret-scan.sh"
if [ -f "${HOOKS_DIR}/../../skills-lock.json" ]; then cp "${HOOKS_DIR}/../../skills-lock.json" "${det_root}/skills-lock.json"; else printf '{}\n' > "${det_root}/skills-lock.json"; fi
git -C "${det_root}" -c init.defaultBranch=main init -q
git -C "${det_root}" add -A
git -C "${det_root}" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q -m "chore: detector copy"
det_sha="$(git -C "${det_root}" rev-parse HEAD)"
make_ci_target "${ci_src}" "${ci_target}"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_root}/scripts/secret-scan-ci.sh" "${ci_target}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ] && grep -qxF -- "secret-scan: total: 1 finding(s) in 1 file(s)" "${work_dir}/out" \
  && grep -qF -- "check out the trusted detector and exclusion list at the commit this job used (${det_sha})" "${work_dir}/out" \
  && grep -qF -- "only in a private local terminal" "${work_dir}/out" \
  && grep -qF -- "never paste or upload its output to a public log, PR or issue" "${work_dir}/out" \
  && grep -qF -- "not the main tip" "${work_dir}/out" \
  && grep -qF -- "GIT_NO_REPLACE_OBJECTS=1 GIT_INDEX_FILE=<new file> git -c core.hooksPath=/dev/null -c core.fsmonitor=false read-tree ${head_want}" "${work_dir}/out" \
  && grep -qF -- "every existing GIT_* variable unset for both commands below" "${work_dir}/out" \
  && grep -qF -- "env -u GIT_DIR -u GIT_OBJECT_DIRECTORY" "${work_dir}/out" \
  && grep -qF -- "do not run make or any script from the scanned commit" "${work_dir}/out" \
  && ! grep -qF -- "check out the scanned commit" "${work_dir}/out" && ! grep -qF -- "run: make secret-scan" "${work_dir}/out" \
  && ! grep -qF -- "${token}" "${work_dir}/out" && ! grep -qF -- "leaked.txt" "${work_dir}/out"; then
  pass "secret-scan-ci: a target without a working tree reports a planted dummy (exit 1, no value or path)"
else
  fail "secret-scan-ci: a target without a working tree reports a planted dummy (exit 1, no value or path)" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi

# 検出器が未コミットの変更を持つとき、案内は sha の取得を勧めず、作業ツリーの版が要ると伝える（sha では再現できない）
printf '\n# local edit\n' >> "${det_root}/scripts/hooks/secret-scan.sh"
rc=0
LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${det_root}/scripts/secret-scan-ci.sh" "${ci_target}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 1 ] && grep -qF -- "use the detector and exclusion list exactly as this run used them" "${work_dir}/out" \
  && grep -qF -- "does not reproduce them" "${work_dir}/out" \
  && ! grep -qF -- "check out the trusted detector" "${work_dir}/out" \
  && grep -qF -- "only in a private local terminal" "${work_dir}/out" \
  && ! grep -qF -- "${token}" "${work_dir}/out" && ! grep -qF -- "leaked.txt" "${work_dir}/out"; then
  pass "secret-scan-ci: an uncommitted detector change switches the rescan hint to the working-tree version"
else
  fail "secret-scan-ci: an uncommitted detector change switches the rescan hint to the working-tree version" "exit ${rc}" "output: $(cat "${work_dir}/out")"
fi
git -C "${det_root}" checkout -q -- scripts/hooks/secret-scan.sh

# 判定が作業ツリーの版へ倒れる残りの経路も、sha の取得を案内しないことを確かめる（期待は緩めない）
assert_worktree_hint() {
  # 引数: 試験名・入口のスクリプト。出力は ${work_dir}/out。
  local name="$1" entry="$2"
  rc=0
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${entry}" "${ci_target}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 1 ] && grep -qF -- "use the detector and exclusion list exactly as this run used them" "${work_dir}/out" \
    && ! grep -qF -- "check out the trusted detector" "${work_dir}/out" \
    && grep -qF -- "only in a private local terminal" "${work_dir}/out" \
    && ! grep -qF -- "${token}" "${work_dir}/out" && ! grep -qF -- "leaked.txt" "${work_dir}/out"; then
    pass "${name}"
  else
    fail "${name}" "exit ${rc}" "output: $(cat "${work_dir}/out")"
  fi
}
# (1) 入口のあるディレクトリが git のリポジトリでなく、HEAD を解決できない（trusted_sha=unknown）
nogit_root="${work_dir}/nogit-root"
mkdir -p "${nogit_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${nogit_root}/scripts/"
cp "${SECRET_SCAN}" "${nogit_root}/scripts/hooks/secret-scan.sh"
if git -C "${nogit_root}" rev-parse --git-dir > /dev/null 2>&1; then
  fail "secret-scan-ci: setup for the unresolvable-HEAD hint" "nogit_root is inside a git repository"
else
  assert_worktree_hint "secret-scan-ci: an unresolvable detector HEAD (unknown) does not suggest checking out a commit" "${nogit_root}/scripts/secret-scan-ci.sh"
  grep -qF -- "(unknown)" "${work_dir}/out" || fail "secret-scan-ci: an unresolvable detector HEAD reports unknown" "output: $(cat "${work_dir}/out")"
fi
# (2) skills-lock.json がコミットと違う
printf '\n' >> "${det_root}/skills-lock.json"
assert_worktree_hint "secret-scan-ci: an uncommitted skills-lock.json change switches the rescan hint to the working-tree version" "${det_root}/scripts/secret-scan-ci.sh"
git -C "${det_root}" checkout -q -- skills-lock.json
# (3) skills-lock.json が未追跡（コミットに無い）
git -C "${det_root}" rm -q --cached skills-lock.json
git -C "${det_root}" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q -m "chore: untrack lock"
assert_worktree_hint "secret-scan-ci: an untracked skills-lock.json switches the rescan hint to the working-tree version" "${det_root}/scripts/secret-scan-ci.sh"

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
# 差し替えの 1 行目は検出器の起動の契約（`#!/usr/bin/env bash`）に合わせる（陰性対照が 1 行目を確かめるため。#6011）。
stub_root="${work_dir}/stub-root"
mkdir -p "${stub_root}/scripts/hooks"
cp "${HOOKS_DIR}/../lib.sh" "${HOOKS_DIR}/../secret-scan-ci.sh" "${HOOKS_DIR}/../secret-scan-canary.sh" "${stub_root}/scripts/"
cat > "${stub_root}/scripts/hooks/secret-scan.sh" <<'EOF'
#!/usr/bin/env bash
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
for missing in git mktemp mkdir rm sort cmp grep cp; do
  run_without_cmd "${missing}" "${HOOKS_DIR}/../secret-scan-canary.sh" git mktemp mkdir rm sort cmp grep cp
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



finish "part 6"
