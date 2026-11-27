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
  mark-final:*/index-final)
    # 複写だけ行い、完了の印だけ残す（付け替えは git の shim が、最後の HEAD の再照合の呼び出しで行う）。
    "${REAL_CP}" "$@" || exit $?
    : > "${CP8_FLAG}.final"
    exit 0
    ;;
  link-final:*/index-final)
    # index-final の複写の直後に、親ディレクトリのリンクを別の（同じ内容の index を持つ）ディレクトリへ付け替える。
    # 一覧は一致するため、複写の後の経路の取り直し（verify_index_unchanged の末尾・最後の HEAD の再照合の後）だけがこれを検出する。
    # 前者で止まることは T14 が git の shim で確かめる（後者だけを固定するのは mark-final と T15）。
    "${REAL_CP}" "$@" || exit $?
    ln -sfn "${CP8_TO}" "${CP8_LINK}" || exit 1
    : > "${CP8_FLAG}.final"
    exit 0
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

# T10: index のパスが有効な /dev/tcp 風でも、不在の確認で開かない（接続の試行を待ち受け側で観測する）（#6047）
# 127.0.0.1 の待ち受けを立て、その実ポートを /dev/tcp のパスに入れる。ファイルを開く（Bash の TCP リダイレクト）実装だと待ち受けが接続を受け、記録が残る。
# 待ち受けは時間に頼らず同期する: 走査の後に番兵の接続（先頭の 1 バイトが S）を送り、待ち受けが番兵を受けて正常終了するまで待つ。
# 接続は受け付けの待ち行列に順に溜まるため、番兵より前の接続はすべて先に記録される（溜まった接続の確認が済んだことが、番兵の受領で分かる）。
# 前提: python3 で待ち受けを立てる。CI（CI または GITHUB_ACTIONS が設定）では python3 が無ければ skip せず失敗にする
# （検出の試験が黙って消えないため）。ローカルでは理由を出して飛ばす。
t10_prereq="ok"
if ! command -v python3 > /dev/null 2>&1 || ! python3 -I -c 'import socket' > /dev/null 2>&1; then
  t10_prereq="missing"
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "secret-scan: a valid /dev/tcp-like index path is never opened (python3 is required in CI)" "missing prerequisite: python3" ""
  else
    echo "ok - secret-scan: /dev/tcp-like index path test skipped (python3 not found)"
  fi
fi
# 陽性対照は bash の /dev/tcp のリダイレクト（ビルド時に有効化される機能）で接続する。無効な bash はパスをファイルとして開こうとして
# "No such file or directory" で失敗する（有効なら閉じたポートで "Connection refused"）。この違いで機能の有無を明示的に判定する。
if [ "${t10_prereq}" = "ok" ]; then
  t10_tcp_probe="$( ( exec 3<> /dev/tcp/127.0.0.1/1 ) 2>&1 || true)"
  case "${t10_tcp_probe}" in
    *"No such file or directory"*)
      t10_prereq="missing"
      if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
        fail "secret-scan: a valid /dev/tcp-like index path is never opened (bash /dev/tcp support is required in CI)" "missing prerequisite: bash /dev/tcp redirection" ""
      else
        echo "ok - secret-scan: /dev/tcp-like index path test skipped (this bash was built without /dev/tcp redirection)"
      fi
      ;;
  esac
fi
if [ "${t10_prereq}" = "ok" ]; then
new_repo; stage src/app.txt "hello"
t10_listen="${work_dir}/t10-listen.py"
cat > "${t10_listen}" <<'EOF10'
import socket, sys
# 引数: ポートの書き出し先・接続の記録先。接続ごとに記録を 1 行足す（先頭の 1 バイトが S なら S、それ以外は C）。S を受けたら 0 で終わる。
s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(16)
s.settimeout(120)
with open(sys.argv[1], "w") as f:
    f.write(str(s.getsockname()[1]))
while True:
    c, _ = s.accept()
    c.settimeout(5)
    try:
        first = c.recv(1)
    except socket.timeout:
        first = b""
    c.close()
    kind = "S" if first == b"S" else "C"
    with open(sys.argv[2], "a") as f:
        f.write(kind + "\n")
    if kind == "S":
        sys.exit(0)
EOF10
rm -f "${work_dir}/t10-port" "${work_dir}/t10-conn"
python3 -I "${t10_listen}" "${work_dir}/t10-port" "${work_dir}/t10-conn" &
t10_pid=$!
for _ in $(seq 1 100); do
  [ -s "${work_dir}/t10-port" ] && break
  sleep 0.1
done
t10_port="$(cat "${work_dir}/t10-port" 2> /dev/null || true)"
# 陽性対照: 待ち受けが接続を記録できることを、/dev/tcp への 1 回の接続で先に確かめる（記録できない待ち受けでは T10 が何も検出できない）。
t10_ctl=0
if [ -n "${t10_port}" ]; then
  # bash の /dev/tcp への接続を、サブシェルで 1 回だけ開いて閉じる
  ( exec 3<> "/dev/tcp/127.0.0.1/${t10_port}" ) 2> /dev/null || true
  for _ in $(seq 1 100); do
    [ -s "${work_dir}/t10-conn" ] && break
    sleep 0.1
  done
  if [ "$(cat "${work_dir}/t10-conn" 2> /dev/null || true)" = "C" ]; then
    t10_ctl=1
  fi
  : > "${work_dir}/t10-conn"
fi
rc=0
t10_sentinel=0
if [ "${t10_ctl}" -eq 1 ]; then
  GIT_INDEX_FILE="/dev/tcp/127.0.0.1/${t10_port}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  # 番兵: 走査の前に溜まった接続はこれより先に記録される。待ち受けが番兵を受けて 0 で終わるまで待つ（待ちは上限付きで、時間経過を合格の根拠にしない）
  if ( exec 3<> "/dev/tcp/127.0.0.1/${t10_port}" && printf 'S' >&3 ) 2> /dev/null; then
    t10_sentinel=1
  fi
fi
t10_lrc=1
if [ "${t10_sentinel}" -eq 1 ]; then
  for _ in $(seq 1 100); do
    kill -0 "${t10_pid}" 2> /dev/null || break
    sleep 0.1
  done
  if kill -0 "${t10_pid}" 2> /dev/null; then
    kill "${t10_pid}" 2> /dev/null || true
  fi
  t10_lrc=0
  wait "${t10_pid}" 2> /dev/null || t10_lrc=$?
else
  kill "${t10_pid}" 2> /dev/null || true
  wait "${t10_pid}" 2> /dev/null || true
fi
t10_log="$(cat "${work_dir}/t10-conn" 2> /dev/null || true)"
# 合格: 陽性対照が済み・走査が 0・番兵を受けて待ち受けが正常終了し・記録が番兵の 1 行だけ（走査からの接続が無い）
if [ "${t10_ctl}" -eq 1 ] && [ "${t10_sentinel}" -eq 1 ] && [ "${t10_lrc}" -eq 0 ] && [ "${rc}" -eq 0 ] && [ "${t10_log}" = "S" ]; then
  pass "secret-scan: a valid /dev/tcp-like index path is never opened (no connection reaches the listener)"
else
  fail "secret-scan: a valid /dev/tcp-like index path is never opened (no connection reaches the listener)" "port ${t10_port:-none} positive-control ${t10_ctl} sentinel ${t10_sentinel} listener-rc ${t10_lrc} rc ${rc} log $(printf '%s' "${t10_log}" | tr '\n' ',')" "output: $(cat "${work_dir}/out" 2> /dev/null)"
fi
fi

# T10b: 未作成の代替 index "-"（作業ツリーのルート基準）は標準入力と取り違えず、空の index として扱う（#6047）
new_repo; stage src/app.txt "hello"
rc=0
GIT_INDEX_FILE="-" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 < /dev/null || rc=$?
if [ "${rc}" -eq 0 ]; then
  pass "secret-scan: a missing alternate index named '-' is treated as empty, not stdin"
else
  fail "secret-scan: a missing alternate index named '-' is treated as empty, not stdin" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# run_watched <秒数> <コマンド...>: コマンドを背景で実行し、時間内に終わらなければ kill して 124 を返す（timeout コマンドは macOS 標準に無い）。
# 終了コードは rc に入れる。標準出力・標準エラーは呼び出し側のリダイレクトを受け継ぐ。
run_watched() {
  local limit="$1" pid i=0
  shift
  # 監視対象を独立したプロセスグループで起動し（set -m）、打ち切りと終了後の取り残しの掃除をグループ全体へ行う。
  set -m
  # 標準入力は明示して /dev/null にする（独立したグループの背景ジョブが端末を読んで SIGTTIN で止まらないように）。
  "$@" < /dev/null &
  pid=$!
  set +m
  while kill -0 "${pid}" 2> /dev/null; do
    if [ "${i}" -ge $((limit * 10)) ]; then
      kill -9 -- "-${pid}" 2> /dev/null || true
      kill -9 "${pid}" 2> /dev/null || true
      wait "${pid}" 2> /dev/null || true
      rc=124
      return 0
    fi
    sleep 0.1
    i=$((i + 1))
  done
  rc=0
  wait "${pid}" || rc=$?
  # 取り残しがあるときだけグループを終了させる（無ければ何もしない）。
  if kill -0 -- "-${pid}" 2> /dev/null; then
    kill -9 -- "-${pid}" 2> /dev/null || true
  fi
}

# T10c: run_watched は打ち切り時に孫プロセスも含めて終了させる（フック本体の PID だけを kill すると子が残る）
t10c_pidfile="${work_dir}/t10c-pid"
rm -f "${t10c_pidfile}"
rc=0
# shellcheck disable=SC2016 # 子の bash に展開させる（意図）
run_watched 1 bash -c 'sleep 30 & echo $! > "$1"; wait' _ "${t10c_pidfile}" > /dev/null 2>&1
sleep 0.3
t10c_child="$(cat "${t10c_pidfile}" 2> /dev/null || true)"
# zombie（終了済みで回収待ち）は生存と数えない
t10c_alive=0
if [ -n "${t10c_child}" ] && kill -0 "${t10c_child}" 2> /dev/null; then
  case "$(ps -o stat= -p "${t10c_child}" 2> /dev/null || true)" in
    Z*) ;;
    *) t10c_alive=1 ;;
  esac
fi
if [ "${rc}" -eq 124 ] && [ -n "${t10c_child}" ] && [ "${t10c_alive}" -eq 0 ]; then
  pass "run_watched: a timeout also ends the child processes of the watched command"
else
  [ -z "${t10c_child}" ] || kill -9 "${t10c_child}" 2> /dev/null || true
  fail "run_watched: a timeout also ends the child processes of the watched command" "rc ${rc} child ${t10c_child:-none}"
fi

# T11: FIFO を index にしても止まらず 2 で終わる（通常ファイルの確認で止まる経路）。打ち切りは run_watched で行う
new_repo; stage src/app.txt "hello"
t11_fifo="${work_dir}/t11-fifo"
rm -f "${t11_fifo}"; mkfifo "${t11_fifo}"
rc=0
GIT_INDEX_FILE="${t11_fifo}" LC_ALL="${utf8_locale}" run_watched 20 "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1
if [ "${rc}" -eq 2 ]; then
  pass "secret-scan: a FIFO index path does not hang the hook (2)"
else
  fail "secret-scan: a FIFO index path does not hang the hook (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f "${t11_fifo}"

# T12: 不在と読んだ後、stat の照会までの間に index の場所が FIFO に変わっても、開かずに待たず 2（index_absent_confirmed の経路）
new_repo; stage src/app.txt "hello"
stat12_dir="${work_dir}/stat12-shim"
mkdir -p "${stat12_dir}"
t12_path="${work_dir}/t12-index"
rm -f "${t12_path}" "${work_dir}/t12-flag"
REAL_STAT="$(command -v stat)"
export REAL_STAT
cat > "${stat12_dir}/stat" <<'EOF12'
#!/bin/sh
# 最初の照会の直前に、不在だった index の場所を FIFO に替える（一度だけ）
if [ ! -e "${T12_FLAG}" ]; then
  : > "${T12_FLAG}"
  mkfifo "${T12_PATH}"
fi
exec "${REAL_STAT}" "$@"
EOF12
chmod +x "${stat12_dir}/stat"
rc=0
GIT_INDEX_FILE="${t12_path}" T12_FLAG="${work_dir}/t12-flag" T12_PATH="${t12_path}" PATH="${stat12_dir}:${PATH}" LC_ALL="${utf8_locale}" \
  run_watched 20 "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1
if [ -e "${work_dir}/t12-flag" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an absent index that becomes a FIFO before the stat check is a scan failure (2), not a hang"
else
  fail "secret-scan: an absent index that becomes a FIFO before the stat check is a scan failure (2), not a hang" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi
rm -f "${t12_path}"

# T13: 親ディレクトリのリンクの参照先が走査中に変わったら、開始時に index があっても 2（index_paths_unchanged。#6047）
new_repo; stage src/app.txt "hello"
t13_dir="${work_dir}/t13"
rm -rf "${t13_dir}"
mkdir -p "${t13_dir}/real1" "${t13_dir}/real2"
cp "$(git rev-parse --git-path index)" "${t13_dir}/real1/index"
cp "$(git rev-parse --git-path index)" "${t13_dir}/real2/index"
ln -s "${t13_dir}/real1" "${t13_dir}/lnk"
rm -f "${work_dir}/t13-flag"
sed -e 's/T8_FLAG/T13_FLAG/g' -e 's/T8_TO/T13_TO/g' -e 's/T8_LINK/T13_LINK/g' "${git8b_dir}/git" > "${git8b_dir}/git13"
mkdir -p "${work_dir}/git13-shim"
mv "${git8b_dir}/git13" "${work_dir}/git13-shim/git"
chmod +x "${work_dir}/git13-shim/git"
rc=0
GIT_INDEX_FILE="${t13_dir}/lnk/index" T13_FLAG="${work_dir}/t13-flag" T13_TO="${t13_dir}/real2" T13_LINK="${t13_dir}/lnk" PATH="${work_dir}/git13-shim:${PATH}" \
  LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/t13-flag" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: an existing index whose parent link was retargeted during the scan is a scan failure (2)"
else
  fail "secret-scan: an existing index whose parent link was retargeted during the scan is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T14: 2 回目の複写の直後に親ディレクトリのリンクが付け替わったら、一覧が一致しても 2（複写の後の経路の取り直し。#6047）
# 複写の前の経路の確認だけでは、前の確認の後・複写の最中の付け替えを見落とし、古い実体を読んで 0 を返す。
# 付け替えは複写の後の取り直し（verify_index_unchanged の末尾）で検出され、最後の HEAD の再照合には進まないことも確かめる（末尾の取り直しを外すと落ちる。最後の取り直しだけの固定は T15）。
new_repo; stage src/app.txt "hello"
t14_dir="${work_dir}/t14"
rm -rf "${t14_dir}"
mkdir -p "${t14_dir}/real1" "${t14_dir}/real2"
cp "$(git rev-parse --git-path index)" "${t14_dir}/real1/index"
cp "$(git rev-parse --git-path index)" "${t14_dir}/real2/index"
ln -s "${t14_dir}/real1" "${t14_dir}/lnk"
rm -f "${work_dir}/t14-flag.final" "${work_dir}/t14-headchk"
# 複写の後の取り直しで止まったことも確かめる: index-final の複写の完了の後に最後の HEAD の再照合（symbolic-ref）まで進んだら印を残す。
# 末尾の取り直しを外すと、付け替えは最後の取り直し（HEAD の再照合の後）まで検出されず、印が残って落ちる。
mkdir -p "${work_dir}/git14-shim"
cat > "${work_dir}/git14-shim/git" <<'EOF14'
#!/bin/sh
if [ -e "${T14_MARK}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "symbolic-ref" ]; then
      : > "${T14_HEADCHK}"
      break
    fi
  done
fi
exec "${REAL_GIT}" "$@"
EOF14
chmod +x "${work_dir}/git14-shim/git"
rc=0
GIT_INDEX_FILE="${t14_dir}/lnk/index" CP8_MODE=link-final CP8_FLAG="${work_dir}/t14-flag" CP8_TO="${t14_dir}/real2" CP8_LINK="${t14_dir}/lnk" \
  T14_MARK="${work_dir}/t14-flag.final" T14_HEADCHK="${work_dir}/t14-headchk" \
  PATH="${work_dir}/git14-shim:${cp8_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/t14-flag.final" ] && [ ! -e "${work_dir}/t14-headchk" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: a parent link retargeted right after the final index copy is a scan failure (2)"
else
  fail "secret-scan: a parent link retargeted right after the final index copy is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T15: 最後の HEAD の再照合と集計の間に親ディレクトリのリンクが付け替わったら、一覧が一致しても 2（最後の経路の取り直しの固定。#6047）
# 2 回目の複写（index-final）の後に走る git は、最後の HEAD の再照合の symbolic-ref だけ。その呼び出しでリンクを付け替える。
# 複写の後の取り直し（verify_index_unchanged の末尾）は付け替えの前に終わっているため、最後の取り直しを外すと 0 を返し、この試験が落ちる。
new_repo; stage src/app.txt "hello"
t15_dir="${work_dir}/t15"
rm -rf "${t15_dir}"
mkdir -p "${t15_dir}/real1" "${t15_dir}/real2" "${work_dir}/git15-shim"
cp "$(git rev-parse --git-path index)" "${t15_dir}/real1/index"
cp "$(git rev-parse --git-path index)" "${t15_dir}/real2/index"
ln -s "${t15_dir}/real1" "${t15_dir}/lnk"
rm -f "${work_dir}/t15-flag.final" "${work_dir}/t15-swapped"
cat > "${work_dir}/git15-shim/git" <<'EOF15'
#!/bin/sh
# index-final の複写の完了の印があるとき、最初の symbolic-ref（最後の HEAD の再照合）の前に一度だけ、リンクの参照先を差し替える
if [ -e "${T15_MARK}" ] && [ ! -e "${T15_SWAPPED}" ]; then
  for arg in "$@"; do
    if [ "${arg}" = "symbolic-ref" ]; then
      : > "${T15_SWAPPED}"
      ln -sfn "${T15_TO}" "${T15_LINK}"
      break
    fi
  done
fi
exec "${REAL_GIT}" "$@"
EOF15
chmod +x "${work_dir}/git15-shim/git"
rc=0
GIT_INDEX_FILE="${t15_dir}/lnk/index" CP8_MODE=mark-final CP8_FLAG="${work_dir}/t15-flag" \
  T15_MARK="${work_dir}/t15-flag.final" T15_SWAPPED="${work_dir}/t15-swapped" T15_TO="${t15_dir}/real2" T15_LINK="${t15_dir}/lnk" \
  PATH="${work_dir}/git15-shim:${cp8_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/t15-swapped" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: the index changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: a parent link retargeted during the final HEAD re-check is a scan failure (2)"
else
  fail "secret-scan: a parent link retargeted during the final HEAD re-check is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T16: 最後の経路の取り直しの間に HEAD が動いたら 2（集計の直前の HEAD の照合し直しの固定。#6047）
# 最後の経路の取り直し（git rev-parse を 2 回呼ぶ）は、最後の HEAD の再照合（symbolic-ref）の後に走る。その最初の呼び出しで HEAD を動かす。
# 経路の取り直しの後の HEAD の照合し直しを外すと、HEAD の移動を見落として 0 を返し、この試験が落ちる。
new_repo; stage src/app.txt "hello"
t16_first="$(git rev-parse HEAD)"
git commit -q --allow-empty -m "chore: second"
t16_second="$(git rev-parse HEAD)"
git reset -q --hard "${t16_first}"
mkdir -p "${work_dir}/git16-shim"
rm -f "${work_dir}/t16-flag.final" "${work_dir}/t16-seen" "${work_dir}/t16-moved"
cat > "${work_dir}/git16-shim/git" <<'EOF16'
#!/bin/sh
# index-final の複写の完了の後、最初の symbolic-ref（最後の HEAD の再照合）より後の最初の経路の取得（rev-parse --git-path）の前に、一度だけ HEAD を動かす
if [ -e "${T16_MARK}" ]; then
  for arg in "$@"; do
    case "${arg}" in
      symbolic-ref) : > "${T16_SEEN}" ;;
      --git-path)
        if [ -e "${T16_SEEN}" ] && [ ! -e "${T16_MOVED}" ]; then
          : > "${T16_MOVED}"
          (unset GIT_INDEX_FILE; "${REAL_GIT}" update-ref HEAD "${T16_TO}") || exit 1
        fi
        ;;
    esac
  done
fi
exec "${REAL_GIT}" "$@"
EOF16
chmod +x "${work_dir}/git16-shim/git"
rc=0
CP8_MODE=mark-final CP8_FLAG="${work_dir}/t16-flag" T16_MARK="${work_dir}/t16-flag.final" T16_SEEN="${work_dir}/t16-seen" T16_MOVED="${work_dir}/t16-moved" T16_TO="${t16_second}" \
  PATH="${work_dir}/git16-shim:${cp8_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ -e "${work_dir}/t16-moved" ] && [ "${rc}" -eq 2 ] \
  && grep -qxF "secret-scan: HEAD changed during the scan; aborting" "${work_dir}/out"; then
  pass "secret-scan: HEAD moved during the last index path re-check is a scan failure (2)"
else
  fail "secret-scan: HEAD moved during the last index path re-check is a scan failure (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
fi

# T17: stat の起動の失敗（診断が ENOENT で終わっても、終了コードが 126・127）・接頭辞 "stat: " の無い診断・末尾が ENOENT でない診断・改行で終わらない診断・空の診断・上限を超える診断（終了コード 1）は「不在」と確かめられたとは言えず 2（path_enoent_confirmed。#6047）
# 起動の失敗は、不在の index を空として 0 を返す経路になり得る（fail-open）。直しを外すと 0 を返し、この試験が落ちる。
t17_i=0
for t17_kind in bad-interpreter exit-127 exit-2 no-prefix multi-line empty-second-line unterminated unterminated-tail enoent-not-at-end nul empty oversize; do
  t17_i=$((t17_i + 1))
  new_repo; stage src/app.txt "hello"
  stat17_dir="${work_dir}/stat17-shim-${t17_i}"
  mkdir -p "${stat17_dir}"
  case "${t17_kind}" in
    bad-interpreter)
      printf '#!/nonexistent-interpreter-t17\n' > "${stat17_dir}/stat"
      ;;
    exit-127)
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\nexit 127\n' > "${stat17_dir}/stat"
      ;;
    exit-2)
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\nexit 2\n' > "${stat17_dir}/stat"
      ;;
    no-prefix)
      # 終了コードは 1 だが、診断に接頭辞 "stat: " が無い（stat 自身の診断とは言えない形）
      printf '#!/bin/sh\necho "x: No such file or directory" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    multi-line)
      # 終了コード 1・最初の行は ENOENT の形だが、末尾の行が ENOENT でない複数行（先頭と末尾の両方を見る）
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\necho "extra" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    empty-second-line)
      # 最初の行は ENOENT の形で、末尾の行が空行（末尾が ENOENT でない）
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\necho "" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    unterminated-tail)
      # 先頭の行は完結しているが、末尾の行が改行で終わらない（末尾が ENOENT の形でも、途中で切れた診断として 1）
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\nprintf "%%s" "stat: cannot statx y: No such file or directory" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    enoent-not-at-end)
      # 末尾の行に ENOENT の文言を含むが、末尾ではない
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory (extra)" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    nul)
      # ENOENT の形の行の後ろに NUL が残る（bash 4.3 以降の read は NUL を読み飛ばすため、バイトで確かめないと通る）
      printf '#!/bin/sh\necho "stat: cannot statx x: No such file or directory" >&2\nprintf "\\000" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    empty)
      # 終了コード 1 だが診断が空
      printf '#!/bin/sh\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    oversize)
      # 接頭辞と末尾は正しいが、診断が上限（64 KiB）を超える
      printf '#!/bin/sh\nprintf "stat: "  >&2\nhead -c 70000 /dev/zero | tr "\\0" a >&2\nprintf ": No such file or directory\\n" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
    unterminated)
      # 1 行の ENOENT の形だが、改行で終わらない（途中で切れた診断）
      printf '#!/bin/sh\nprintf "%%s" "stat: cannot statx x: No such file or directory" >&2\nexit 1\n' > "${stat17_dir}/stat"
      ;;
  esac
  case "${t17_kind}" in
    multi-line) t17_msg="a multi-line stat diagnostic not ending with ENOENT" ;;
    unterminated-tail) t17_msg="a stat diagnostic whose last line is unterminated" ;;
    enoent-not-at-end) t17_msg="a stat diagnostic with the ENOENT text not at the end" ;;
    nul) t17_msg="a stat diagnostic containing NUL" ;;
    empty) t17_msg="an empty stat diagnostic" ;;
    oversize) t17_msg="an oversize stat diagnostic" ;;
    empty-second-line) t17_msg="a stat diagnostic with a trailing empty line" ;;
    unterminated) t17_msg="an unterminated stat diagnostic" ;;
    no-prefix) t17_msg="a stat diagnostic without the stat: prefix" ;;
    *) t17_msg="a stat that fails to run (${t17_kind})" ;;
  esac
  chmod +x "${stat17_dir}/stat"
  rc=0
  GIT_INDEX_FILE="${work_dir}/t17-absent-index-${t17_i}" PATH="${stat17_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: ${t17_msg} does not confirm an absent index (2)"
  else
    fail "secret-scan: ${t17_msg} does not confirm an absent index (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done

# T18: 診断の受け皿を開けず stat が起動されなかった呼び出しで、前回の ENOENT の診断を読まない（path_enoent_confirmed。#6047）
# 1 回目の照会の後に受け皿を読み取り専用にして、2 回目のリダイレクトの open を失敗させる（bash は stat を起動せず 1 を返す）。
# 受け皿を消す処理（照会の前・読んだ後のどちらか一方があれば防げる）があれば、2 回目は新しく作られ、stat（shim）が起動されて
# Permission denied を返し、2 で止まる。両方を外すと、前回の ENOENT の診断が残ったまま読まれ、不在と確かめたことになって 0 を返し、この試験が落ちる。
# 読み取り専用になったこと（shim が chmod の後に [ ! -w ] で確かめた印）が無ければ、何も確かめていないため合格にせず、理由を付けて skip する。
# ルートは権限を迂回するため、一般ユーザーのときだけ行い、ルートでは skip を明示する。
if [ "$(id -u)" -ne 0 ]; then
  new_repo; stage src/app.txt "hello"
  stat18_dir="${work_dir}/stat18-shim"
  t18_tmp="${work_dir}/t18-tmp"
  rm -rf "${stat18_dir}" "${t18_tmp}" "${work_dir}/t18-n" "${work_dir}/t18-ro" "${work_dir}/t18-second" "${work_dir}/t18-found" "${work_dir}/t18-chmod"
  mkdir -p "${stat18_dir}" "${t18_tmp}"
  cat > "${stat18_dir}/stat" <<'EOF18'
#!/bin/sh
# 1 回目: 診断の受け皿を読み取り専用にしてから、本物の stat（ENOENT）を流す。2 回目以降（受け皿が新しく作られたときだけ届く）: 印を残して別の誤りを返す
if [ ! -e "${T18_N}" ]; then
  : > "${T18_N}"
  for f in "${T18_TMP}"/secret-scan.*/stat-err; do
    # 3 つを別々に記録する: ファイルを特定できたこと（glob の一致）・chmod の成功・読み取り専用になったこと
    if [ -e "${f}" ]; then
      : > "${T18_FOUND}"
      if chmod 444 "${f}"; then
        : > "${T18_CHMOD}"
      fi
      if [ ! -w "${f}" ]; then
        : > "${T18_RO}"
      fi
    fi
  done
  exec "${REAL_STAT}" "$@"
fi
: > "${T18_SECOND}"
echo "stat: cannot statx x: Permission denied" >&2
exit 1
EOF18
  chmod +x "${stat18_dir}/stat"
  rc=0
  GIT_INDEX_FILE="${work_dir}/t18-absent-index" TMPDIR="${t18_tmp}" T18_TMP="${t18_tmp}" T18_N="${work_dir}/t18-n" T18_RO="${work_dir}/t18-ro" T18_FOUND="${work_dir}/t18-found" T18_CHMOD="${work_dir}/t18-chmod" T18_SECOND="${work_dir}/t18-second" \
    PATH="${stat18_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ ! -e "${work_dir}/t18-n" ]; then
    # shim が一度も起動されていない（前提が崩れた）。skip にせず失敗にする
    fail "secret-scan: a stale ENOENT diagnostic is not read when the diagnostic file cannot be opened (2)" "the stat shim was never started" "got ${rc}" "output: $(cat "${work_dir}/out")"
  elif [ ! -e "${work_dir}/t18-found" ]; then
    # 受け皿のファイルを特定できていない（glob の不一致等）。環境の都合ではないため skip にせず失敗にする
    fail "secret-scan: a stale ENOENT diagnostic is not read when the diagnostic file cannot be opened (2)" "the diagnostic file was not found by the shim" "got ${rc}" "output: $(cat "${work_dir}/out")"
  elif [ ! -e "${work_dir}/t18-chmod" ]; then
    # chmod が失敗した。環境が権限を強制しないこととは別で、準備の失敗のため失敗にする
    fail "secret-scan: a stale ENOENT diagnostic is not read when the diagnostic file cannot be opened (2)" "chmod on the diagnostic file failed" "got ${rc}" "output: $(cat "${work_dir}/out")"
  elif [ ! -e "${work_dir}/t18-ro" ]; then
    # ファイルを特定でき chmod も成功したが、書き込み権限が残る（権限を強制しない環境）。何も確かめていないため、合格とは数えず理由付きで skip する
    echo "ok - secret-scan: stale diagnostic test skipped (write permission remained after a successful chmod; nothing was checked)"
  elif [ -e "${work_dir}/t18-second" ] && [ "${rc}" -eq 2 ] \
    && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: a stale ENOENT diagnostic is not read when the diagnostic file cannot be opened (2)"
  else
    fail "secret-scan: a stale ENOENT diagnostic is not read when the diagnostic file cannot be opened (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
else
  echo "ok - secret-scan: stale diagnostic test skipped (running as root bypasses permissions)"
fi

# T19: 改行を含む不在の index のパスは、診断の全体の先頭と末尾（strerror）で ENOENT と確かめて 0 になる（path_enoent_confirmed。#6047）
# BSD（macOS）の stat・busybox の stat はパスの改行をそのまま診断に出し、複数行になる。GNU 形・BSD 形の shim と実物の stat の 3 つで確かめる。
new_repo; stage src/app.txt "hello"
t19_nl=$'t19-dir\nwith-newline'
mkdir -p "${work_dir}/${t19_nl}"
for t19_kind in gnu bsd real; do
  stat19_dir="${work_dir}/stat19-shim-${t19_kind}"
  mkdir -p "${stat19_dir}"
  case "${t19_kind}" in
    gnu)
      # GNU 形（パスを生のまま入れた形にして、改行が診断の途中に入る）
      # shellcheck disable=SC2016 # shim の本文。展開させず、shim の実行時に評価させる（意図）
      printf '#!/bin/sh\nfor last in "$@"; do :; done\nprintf "stat: cannot statx %%s: No such file or directory\\n" "${last}" >&2\nexit 1\n' > "${stat19_dir}/stat"
      ;;
    bsd)
      # shellcheck disable=SC2016 # shim の本文。展開させず、shim の実行時に評価させる（意図）
      printf '#!/bin/sh\nfor last in "$@"; do :; done\nprintf "stat: %%s: stat: No such file or directory\\n" "${last}" >&2\nexit 1\n' > "${stat19_dir}/stat"
      ;;
    real)
      # 実物の stat（shim なし）
      rm -rf "${stat19_dir}"
      stat19_dir="${work_dir}/stat19-shim-empty"
      mkdir -p "${stat19_dir}"
      ;;
  esac
  [ ! -f "${stat19_dir}/stat" ] || chmod +x "${stat19_dir}/stat"
  rc=0
  GIT_INDEX_FILE="${work_dir}/${t19_nl}/absent-index" PATH="${stat19_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 0 ]; then
    pass "secret-scan: an absent index path containing a newline is confirmed absent (${t19_kind} stat) (0)"
  else
    fail "secret-scan: an absent index path containing a newline is confirmed absent (${t19_kind} stat) (0)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done

# T19b: 実際に照会されるパス（": No such file or directory" と改行の並びを含まない改行入りのパス）で、診断の末尾が ENOENT でない（Permission denied）なら、
# 診断の途中の行が ENOENT の文言で終わっていても不在と確かめず 2。stat が起動されたこと（起動の印）も確かめる（照会の前に拒否されていないこと）
new_repo; stage src/app.txt "hello"
t19_spoof=$'t19-spoof\nx: No such file or directory'
mkdir -p "${work_dir}/${t19_spoof}"
stat19s_dir="${work_dir}/stat19-shim-spoof"
stat19s_mark="${work_dir}/stat19-spoof-called"
mkdir -p "${stat19s_dir}"
rm -f "${stat19s_mark}"
# shellcheck disable=SC2016 # shim の本文。展開させず、shim の実行時に評価させる（意図）
printf '#!/bin/sh\n: > "%s"\nfor last in "$@"; do :; done\nprintf "stat: cannot statx %%s: Permission denied\\n" "${last}" >&2\nexit 1\n' "${stat19s_mark}" > "${stat19s_dir}/stat"
chmod +x "${stat19s_dir}/stat"
rc=0
GIT_INDEX_FILE="${work_dir}/${t19_spoof}/absent-index" PATH="${stat19s_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && [ -e "${stat19s_mark}" ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: a queried newline path with the ENOENT text in a middle line is not confirmed absent when the diagnostic ends with another error (stat ran) (2)"
else
  fail "secret-scan: a queried newline path with the ENOENT text in a middle line is not confirmed absent when the diagnostic ends with another error (stat ran) (2)" "got ${rc}, stat called: $([ -e "${stat19s_mark}" ] && echo yes || echo no)" "output: $(cat "${work_dir}/out")"
fi

# T19c: 照会の前の拒否の試験。パス自身が ": No such file or directory" と改行の並びを含むときは、path_enoent_confirmed が stat を起動する前に 1 を返し、
# index の確認は 2（診断が並びの直後で切れても完結した ENOENT と区別できないため。T19 の普通の改行入りパスは 0 のまま）。stat が起動されないことも確かめる
new_repo; stage src/app.txt "hello"
t19c_dir=$'t19c\nx: No such file or directory\n'
mkdir -p "${work_dir}/${t19c_dir}"
stat19c_dir="${work_dir}/stat19-shim-cut"
stat19c_mark="${work_dir}/stat19c-called"
mkdir -p "${stat19c_dir}"
rm -f "${stat19c_mark}"
# shellcheck disable=SC2016 # shim の本文。展開させず、shim の実行時に評価させる（意図）
printf '#!/bin/sh\n: > "%s"\nfor last in "$@"; do :; done\nprintf "stat: %%s" "${last%%/absent-index}" >&2\nexit 1\n' "${stat19c_mark}" > "${stat19c_dir}/stat"
chmod +x "${stat19c_dir}/stat"
rc=0
GIT_INDEX_FILE="${work_dir}/${t19c_dir}/absent-index" PATH="${stat19c_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
if [ "${rc}" -eq 2 ] && [ ! -e "${stat19c_mark}" ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
  pass "secret-scan: a path containing the ENOENT text and a newline is rejected before stat runs (pre-query rejection) (2)"
else
  fail "secret-scan: a path containing the ENOENT text and a newline is rejected before stat runs (pre-query rejection) (2)" "got ${rc}, stat called: $([ -e "${stat19c_mark}" ] && echo yes || echo no)" "output: $(cat "${work_dir}/out")"
fi

# T20: 診断の大きさの境界（ちょうど 64 KiB は 0、1 バイト超過は 2）。接頭辞 "stat: " と末尾 ": No such file or directory\n"（28 バイト）の間を詰めて合わせる
new_repo; stage src/app.txt "hello"
for t20_total in 65536 65537; do
  stat20_dir="${work_dir}/stat20-shim-${t20_total}"
  mkdir -p "${stat20_dir}"
  # shellcheck disable=SC2016 # shim の本文。展開させず、shim の実行時に評価させる（意図）
  printf '#!/bin/sh\nprintf "stat: " >&2\nhead -c %s /dev/zero | tr "\\000" a >&2\nprintf ": No such file or directory\\n" >&2\nexit 1\n' "$((t20_total - 6 - 28))" > "${stat20_dir}/stat"
  chmod +x "${stat20_dir}/stat"
  rc=0
  GIT_INDEX_FILE="${work_dir}/t20-absent-index" PATH="${stat20_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${t20_total}" -eq 65536 ]; then t20_want=0; else t20_want=2; fi
  if [ "${rc}" -eq "${t20_want}" ]; then
    pass "secret-scan: a stat diagnostic of ${t20_total} bytes gives ${t20_want} (limit boundary)"
  else
    fail "secret-scan: a stat diagnostic of ${t20_total} bytes gives ${t20_want} (limit boundary)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done

# T21: 診断の読み取りに使う head・cmp・tr が失敗したら、不在と確かめず 2（失敗を無視して通さない）
# head は本物の結果を作ってから失敗を返す（失敗を無視すると、複写が一致して 0 になる）。cmp は診断の受け皿の読み取りのとき、tr は NUL の除去（-d）のときだけ失敗させる。
real_head21="$(command -v head)"; real_cmp21="$(command -v cmp)"; real_tr21="$(command -v tr)"
export real_head21 real_cmp21 real_tr21
for t21_cmd in head cmp tr; do
  new_repo; stage src/app.txt "hello"
  shim21_dir="${work_dir}/shim21-${t21_cmd}"
  mkdir -p "${shim21_dir}"
  cat > "${shim21_dir}/${t21_cmd}" <<'EOF21'
#!/bin/sh
case "$*" in
  *stat-err*|*"-d "*)
    case "$(basename "$0")" in
      head) "${real_head21}" "$@"; exit 1 ;;
      cmp) "${real_cmp21}" "$@"; exit 2 ;;
      tr) "${real_tr21}" "$@"; exit 1 ;;
    esac
    ;;
esac
case "$(basename "$0")" in
  head) exec "${real_head21}" "$@" ;;
  cmp) exec "${real_cmp21}" "$@" ;;
  tr) exec "${real_tr21}" "$@" ;;
esac
EOF21
  chmod +x "${shim21_dir}/${t21_cmd}"
  rc=0
  GIT_INDEX_FILE="${work_dir}/t21-absent-index" PATH="${shim21_dir}:${PATH}" LC_ALL="${utf8_locale}" "${HOOK_BASH}" "${SECRET_SCAN}" > "${work_dir}/out" 2>&1 || rc=$?
  if [ "${rc}" -eq 2 ] && grep -qxF "secret-scan: cannot snapshot the index; aborting" "${work_dir}/out"; then
    pass "secret-scan: a failing ${t21_cmd} while reading the stat diagnostic does not confirm an absent index (2)"
  else
    fail "secret-scan: a failing ${t21_cmd} while reading the stat diagnostic does not confirm an absent index (2)" "got ${rc}" "output: $(cat "${work_dir}/out")"
  fi
done

finish "part 8"
