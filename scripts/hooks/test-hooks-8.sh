#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 8（TASK-6）。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。単体でも実行できる。
# 準備・helper・例示値は test-hooks-lib.sh が持つ。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-lib.sh"

# ---------------------------------------------------------------- secret-scan: 終了時の index の 2 回目の確認（#6047）
# 終了時の確認は 2 回（HEAD の取り直しの前の index-after と、集計の直前の index-final）。
# 1 回目の複写の後に元の index が置き換わっても、2 回目の確認で検出して 2 にする。
# 内容（一覧）で比べるため、stat 情報だけの更新（バイトは変わるが一覧は変わらない）では 0 のまま。
REAL_CP="$(command -v cp)"
export REAL_CP
cp8_dir="${work_dir}/cp8-shim"
mkdir -p "${cp8_dir}"
# CP8_MODE: mutate=index-after の複写の直後に元の index へ別のファイルを add する（一度だけ）。
# stat=同じ場所で追跡ファイルの mtime だけ変えて update-index --refresh する（一度だけ。複写とのバイト差を記録する）。
# fail-final=index-final への複写だけ失敗させる（index-after は通し、通ったことを記録する）。
cat > "${cp8_dir}/cp" <<'EOF8'
#!/bin/sh
for last in "$@"; do :; done
case "${CP8_MODE:-}:${last}" in
  fail-final:*/index-final)
    : > "${CP8_FLAG}.final"
    exit 1
    ;;
  head-final:*/index-final)
    "${REAL_CP}" "$@" || exit $?
    # 事前に用意したコミットへ update-ref で切り替える。失敗したら完了の印を作らず、原因を標準エラーに残す
    if ! (unset GIT_INDEX_FILE; "${REAL_GIT}" update-ref HEAD "${CP8_HEAD_TO}"); then
      echo "cp8 shim: update-ref failed" >&2
      exit 1
    fi
    : > "${CP8_FLAG}.final"
    exit 0
    ;;
esac
"${REAL_CP}" "$@" || exit $?
case "${last}" in
  */index-after)
    : > "${CP8_FLAG}.first"
    if [ ! -e "${CP8_FLAG}" ]; then
      case "${CP8_MODE:-}" in
        mutate)
          : > "${CP8_FLAG}"
          (unset GIT_INDEX_FILE; "${REAL_GIT}" add -- "${CP8_ADD}")
          ;;
        stat)
          : > "${CP8_FLAG}"
          # touch と update-index --refresh の成功を確かめる。失敗したら前提崩れの印を残し、cmp の判定に進まない
          if ! touch -t 200101010000 "${CP8_TOUCH}" \
            || ! (unset GIT_INDEX_FILE; "${REAL_GIT}" update-index --refresh > /dev/null 2>&1); then
            : > "${CP8_FLAG}.setuperr"
            exit 0
          fi
          orig="$(unset GIT_INDEX_FILE; "${REAL_GIT}" rev-parse --path-format=absolute --git-path index)"
          cmp_rc=0
          cmp -s "${orig}" "${last}" || cmp_rc=$?
          case "${cmp_rc}" in
            0) ;;
            1) : > "${CP8_FLAG}.differs" ;;
            *) : > "${CP8_FLAG}.cmperr" ;;
          esac
          ;;
      esac
    fi
    ;;
esac
EOF8
chmod +x "${cp8_dir}/cp"

# T1: 1 回目の複写の後に元の index が置き換わったら 2（修正前は 0 を返していた）
new_repo; stage src/app.txt "hello"
printf '%s\n' "other" > extra.txt
rm -f "${work_dir}/cp8-flag" "${work_dir}/cp8-flag.first"
rc=0
CP8_MODE=mutate CP8_FLAG="${work_dir}/cp8-flag" CP8_ADD=extra.txt PATH="${cp8_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/cp8-flag" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index replaced after the first end-of-scan copy is a scan failure (2)"
else
  fail "secret-scan: an index replaced after the first end-of-scan copy is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T2: stat 情報だけの更新（バイトは変わるが一覧は変わらない）では誤検出しない（0）
new_repo; stage src/app.txt "hello"
rm -f "${work_dir}/cp8-flag" "${work_dir}/cp8-flag.first" "${work_dir}/cp8-flag.differs" "${work_dir}/cp8-flag.cmperr" "${work_dir}/cp8-flag.setuperr"
rc=0
CP8_MODE=stat CP8_FLAG="${work_dir}/cp8-flag" CP8_TOUCH=src/app.txt PATH="${cp8_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/cp8-flag.setuperr" ]; then
  fail "secret-scan: a stat-only index update after the first copy is not a scan failure" "premise broken: touch or update-index --refresh failed" "output: $(cat "${work_dir}/out")"
elif [ -e "${work_dir}/cp8-flag.cmperr" ]; then
  fail "secret-scan: a stat-only index update after the first copy is not a scan failure" "premise broken: cmp could not compare" "output: $(cat "${work_dir}/out")"
elif [ ! -e "${work_dir}/cp8-flag.differs" ]; then
  fail "secret-scan: a stat-only index update after the first copy is not a scan failure" "premise broken: the index bytes did not differ" "output: $(cat "${work_dir}/out")"
elif [ "${rc}" -eq 0 ]; then
  pass "secret-scan: a stat-only index update after the first copy is not a scan failure"
else
  fail "secret-scan: a stat-only index update after the first copy is not a scan failure" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T3: 集計の直前の複写（index-final）の失敗は 2。1 回目の複写は通る
new_repo; stage src/app.txt "hello"
rm -f "${work_dir}/cp8-flag" "${work_dir}/cp8-flag.first" "${work_dir}/cp8-flag.final"
rc=0
CP8_MODE=fail-final CP8_FLAG="${work_dir}/cp8-flag" PATH="${cp8_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/cp8-flag.first" ] && [ -e "${work_dir}/cp8-flag.final" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: a failure of the final pre-verdict index copy is a scan failure (2)"
else
  fail "secret-scan: a failure of the final pre-verdict index copy is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T4: 開始時に無かった index の親ディレクトリが走査中に検索できなくなったら、空として扱わず 2（#6047）。
# ルートは権限を迂回するため、一般ユーザーのときだけ行い、ルートでは skip を明示する。
if [ "$(id -u)" -ne 0 ]; then
  git8_dir="${work_dir}/git8-shim"
  mkdir -p "${git8_dir}"
  cat > "${git8_dir}/git" <<'EOF9'
#!/bin/sh
# 本文の diff（-U0）の呼び出しの前に一度だけ、T4_DIR の検索権限を外す
if [ ! -e "${T4_FLAG}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "-U0" ]; then
      : > "${T4_FLAG}"
      chmod 000 "${T4_DIR}"
      break
    fi
  done
fi
exec "${REAL_GIT}" "$@"
EOF9
  chmod +x "${git8_dir}/git"
  new_repo; stage src/app.txt "hello"
  t4_dir="${work_dir}/t4-index-dir"
  mkdir -p "${t4_dir}"
  rm -f "${work_dir}/t4-flag"
  rc=0
  GIT_INDEX_FILE="${t4_dir}/index" T4_DIR="${t4_dir}" T4_FLAG="${work_dir}/t4-flag" PATH="${git8_dir}:${PATH}" \
    LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  chmod 755 "${t4_dir}"
  if [ -e "${work_dir}/t4-flag" ] && [ "${rc}" -eq 2 ] \
    && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: a missing index whose parent became unsearchable is a scan failure (2), not an empty index"
  else
    fail "secret-scan: a missing index whose parent became unsearchable is a scan failure (2), not an empty index" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
  # T7: 親の経路にリンクがある代替 index。解いた先の祖先は検索できるまま、リンクに至るディレクトリの検索権限が走査中に外れたら 2（#6047）
  new_repo; stage src/app.txt "hello"
  t7_outer="${work_dir}/t7-outer"
  t7_real="${work_dir}/t7-real"
  mkdir -p "${t7_outer}" "${t7_real}"
  ln -s "${t7_real}" "${t7_outer}/lnk"
  rm -f "${work_dir}/t7-flag"
  rc=0
  GIT_INDEX_FILE="${t7_outer}/lnk/index" T4_DIR="${t7_outer}" T4_FLAG="${work_dir}/t7-flag" PATH="${git8_dir}:${PATH}" \
    LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  chmod 755 "${t7_outer}"
  if [ -e "${work_dir}/t7-flag" ] && [ "${rc}" -eq 2 ] \
    && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: a missing index reached through a link whose parent became unsearchable is a scan failure (2)"
  else
    fail "secret-scan: a missing index reached through a link whose parent became unsearchable is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
else
  echo "ok - secret-scan: a missing index whose parent became unsearchable (also via a link) is a scan failure (2): skipped (running as root bypasses permissions)"
fi

# T5: 2 回目の index 検証の途中に HEAD が動いたら 2（検証の後にも HEAD を照合する）
new_repo; stage src/app.txt "hello"
t5_first="$(git rev-parse HEAD)"
git commit -q --allow-empty -m "chore: second"
t5_second="$(git rev-parse HEAD)"
git reset -q --hard "${t5_first}"
rm -f "${work_dir}/cp8-flag" "${work_dir}/cp8-flag.first" "${work_dir}/cp8-flag.final"
rc=0
CP8_MODE=head-final CP8_HEAD_TO="${t5_second}" CP8_FLAG="${work_dir}/cp8-flag" PATH="${cp8_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/cp8-flag.final" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: HEAD changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: HEAD moved during the final index check is a scan failure (2)"
else
  fail "secret-scan: HEAD moved during the final index check is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T6: 親ディレクトリが未作成の GIT_INDEX_FILE は空の index として扱う（検査不能にしない）
new_repo
rc=0
GIT_INDEX_FILE="${work_dir}/no-such-dir/sub/index" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: an index whose parent directory does not exist is an empty index"
else
  fail "secret-scan: an index whose parent directory does not exist is an empty index" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T8: 開始時に無かった index の解く前の経路が、走査中に別の（index を持つ）ディレクトリを指すよう替わったら 2（#6047）
new_repo; stage src/app.txt "hello"
t8_dir="${work_dir}/t8"
mkdir -p "${t8_dir}/real1" "${t8_dir}/real2"
cp "$(git rev-parse --git-path index)" "${t8_dir}/real2/index"
ln -s "${t8_dir}/real1" "${t8_dir}/lnk"
git8b_dir="${work_dir}/git8b-shim"
mkdir -p "${git8b_dir}"
cat > "${git8b_dir}/git" <<'EOF9'
#!/bin/sh
# 本文の diff（-U0）の呼び出しの前に一度だけ、リンクの参照先を差し替える
if [ ! -e "${T8_FLAG}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "-U0" ]; then
      : > "${T8_FLAG}"
      ln -sfn "${T8_TO}" "${T8_LINK}"
      break
    fi
  done
fi
exec "${REAL_GIT}" "$@"
EOF9
chmod +x "${git8b_dir}/git"
rm -f "${work_dir}/t8-flag"
rc=0
GIT_INDEX_FILE="${t8_dir}/lnk/index" T8_FLAG="${work_dir}/t8-flag" T8_TO="${t8_dir}/real2" T8_LINK="${t8_dir}/lnk" PATH="${git8b_dir}:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/t8-flag" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: a missing index whose reference path was retargeted during the scan is a scan failure (2)"
else
  fail "secret-scan: a missing index whose reference path was retargeted during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T9: 名前が長すぎる index（ENAMETOOLONG）は、祖先が検索できても ENOENT ではないため空として扱わず 2（#6047）
new_repo; stage src/app.txt "hello"
t9_name="$(printf 'a%.0s' $(seq 1 300))"
rc=0
GIT_INDEX_FILE="${work_dir}/${t9_name}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an index with a too-long name (not ENOENT) is a scan failure (2)"
else
  fail "secret-scan: an index with a too-long name (not ENOENT) is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T10: index のパスが /dev/tcp 風でも、不在の確認で開かない（外向き通信・接続の誤りを出さない）（#6047）
new_repo; stage src/app.txt "hello"
rc=0
GIT_INDEX_FILE="/dev/tcp/127.0.0.1/9/index" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if ! grep -qiE "connect|refused|/dev/tcp" "${work_dir}/out" && [ "${rc}" -ne 1 ]; then
  pass "secret-scan: a /dev/tcp-like index path is never opened (no connection attempt)"
else
  fail "secret-scan: a /dev/tcp-like index path is never opened (no connection attempt)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T11: 不在判定の経路で開かない（FIFO が現れても止まらない）。stat の shim は使わず、FIFO を index にして 2 で即座に終わることを確かめる
new_repo; stage src/app.txt "hello"
t11_fifo="${work_dir}/t11-fifo"
rm -f "${t11_fifo}"; mkfifo "${t11_fifo}"
rc=0
GIT_INDEX_FILE="${t11_fifo}" LC_ALL="${utf8_locale}" timeout 20 "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan: a FIFO index path does not hang the hook (2)"
else
  fail "secret-scan: a FIFO index path does not hang the hook (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f "${t11_fifo}"

finish "part 8"
