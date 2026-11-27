#!/usr/bin/env bash
# 秘密情報検査の CI 向け入口（TASK-6。CI の secret-scan ジョブから呼ぶ〔#51〕。ローカルからも同じ形で呼べる）。
#   scripts/secret-scan-ci.sh [<走査対象のリポジトリのパス>]   既定は本スクリプトのあるリポジトリ
# 検出器・陰性対照は本スクリプトと同じリポジトリ（CI では PR が書き換えられない版。PR では保護された main の先端）のものを使い、
# 走査対象はデータとして渡す。公開ログには規則 ID・件数・固定の診断文だけを出す。
# 検査不能（2）のときは、lib.sh の許可リストと行全体が完全一致する検出器の診断文も通す（#6036）。
# 走査するのは HEAD を sha に固定して作った専用の一時 index（作業ツリーの index は読まない）。
# 検出器の出力は一度ファイルへ受け、規則 ID・件数の行（--summary の出力形）だけを通す。ファイル名・値・
# git のエラー文は出さない（パスに値が含まれ得る。SECR-23）。
# 終了コード: 0 = 検出なし・陰性対照も期待どおり / 1 = 検出または陰性対照の失敗 / 2 = 検査を完了できない。
# 範囲外: コミット履歴・submodule の中。検出時は .claude/rules/security.md の重大度「高」に従う。
set -euo pipefail
# 相対パスの解決（cd）が CDPATH に左右されないようにする（以降の cd・lib.sh の解決より前に外す）。
unset CDPATH
# 想定外の失敗（set -e 相当）は検出（1）と取り違えないよう、すべて検査不能（2）として止める（fail-closed）。
# lib.sh の読み込み・DIR の解決の失敗も対象にするため、source より前に置く。
set -E
trap 'echo "secret-scan-ci: internal error; aborting" >&2; exit 2' ERR
# 起動時のパス解決は bash の組み込みだけで行う（依存の確認より前に外部コマンド dirname を呼ばない）。
_src="${BASH_SOURCE[0]}"
case "${_src}" in
  */*) _src_dir="${_src%/*}" ;;
  *) _src_dir="." ;;
esac
[ -n "${_src_dir}" ] || _src_dir="/"
DIR="$(cd -- "${_src_dir}" && pwd)"
unset _src _src_dir
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

# 外部コマンドは起動前に確かめる。欠けていれば検査不能（2）で止める（makefile.md の契約。fail-closed）。
for _c in git mktemp grep rm head sleep readlink; do
  require_cmd "${_c}" || exit 2
done
unset _c

# 呼び出し元の環境が git のリポジトリ・index・設定を差し替えないよう、GIT_* をすべて外す
# （GIT_INDEX_FILE・GIT_DIR・GIT_WORK_TREE 等。対象の確認と本走査を同じ環境で行う）。
# 列挙の失敗を「該当なし」と取り違えないよう、パイプを使わず bash の照合で絞り、失敗は検査不能にする。
if ! env_names="$(compgen -e 2> /dev/null)"; then
  echo "secret-scan-ci: failed to list environment variables" >&2
  exit 2
fi
for _v in ${env_names}; do
  case "${_v}" in
    GIT_*) unset "${_v}" ;;
  esac
done
unset _v env_names
# 置換参照（refs/replace/）でオブジェクトの中身が差し替わらないよう、対象の確定から検出器・陰性対照の実行まで
# 一貫して無効にする（子プロセスへ受け継ぐ）。
export GIT_NO_REPLACE_OBJECTS=1
# git の呼び出しは、設定で外部プログラムを起動し得る経路（hooks・fsmonitor）を無効にして通す。
g() {
  git -c core.hooksPath=/dev/null -c core.fsmonitor=false "$@"
}

if [ "$#" -gt 1 ]; then
  echo "usage: secret-scan-ci.sh [<target-repo-path>]" >&2
  exit 2
fi
# 既定の対象を使うのは引数を省略したときだけ。空文字が明示されたときは取り違えを避けて検査不能にする。
if [ "$#" -eq 0 ]; then
  target="${ROOT_DIR}"
else
  target="$1"
  if [ -z "${target}" ]; then
    echo "secret-scan-ci: target is empty" >&2
    exit 2
  fi
fi
# `-` や `-x` のような値を cd がオプション・OLDPWD として解釈しないよう、絶対パスでなければ `./` を前に付ける。
case "${target}" in
  /*) ;;
  *) target="./${target}" ;;
esac
if [ ! -d "${target}" ]; then
  echo "secret-scan-ci: target is not a directory" >&2
  exit 2
fi
# パスの取得は `$(...)` が末尾の改行を落とすため、番兵（x）を付けて受けてから外す（改行で終わるパスも保つ）。
if ! target="$(cd "${target}" 2> /dev/null && pwd -P && printf x)"; then
  echo "secret-scan-ci: cannot resolve the target directory" >&2
  exit 2
fi
target="${target%x}"
target="${target%$'\n'}"
if [ -z "${target}" ]; then
  echo "secret-scan-ci: cannot resolve the target directory" >&2
  exit 2
fi
# 親のリポジトリを誤って走査しないよう、対象がリポジトリのルートそのものであることを確かめる（fail-closed）。
top="$(g -C "${target}" rev-parse --show-toplevel 2> /dev/null && printf x)" || top=""
top="${top%x}"
top="${top%$'\n'}"
top_real=""
if [ -n "${top}" ]; then
  top_real="$(cd "${top}" 2> /dev/null && pwd -P && printf x)" || top_real=""
  top_real="${top_real%x}"
  top_real="${top_real%$'\n'}"
fi
if [ -z "${top_real}" ] || [ "${top_real}" != "${target}" ]; then
  echo "secret-scan-ci: target is not the root of a git repository" >&2
  exit 2
fi

# 走査対象は作業ツリーの index ではなく、HEAD を sha に固定してその tree から作る専用の一時 index にする。
# 照合（HEAD と index の比較）と走査が index を別々に開く隙間（TOCTOU）で index が書き換わっても、
# 取り違えて 0 を返さない。作業ツリーの index が HEAD と違っても、走査するのは HEAD の内容である。
# git の呼び出しは対象の hooks（post-index-change 等）を起動しない。
if ! head_sha="$(g -C "${target}" rev-parse --verify "HEAD^{commit}" 2> /dev/null)" || [ -z "${head_sha}" ]; then
  echo "secret-scan-ci: cannot resolve HEAD" >&2
  exit 2
fi
if ! work_dir="$(mktemp -d "${TMPDIR:-/tmp}/secret-scan-ci.XXXXXX" 2> /dev/null)" || [ -z "${work_dir}" ]; then
  echo "secret-scan-ci: mktemp failed" >&2
  exit 2
fi
trap 'rm -rf "${work_dir}"' EXIT
# 相対の TMPDIR でも、git は `-C` や cd で作業ディレクトリが変わる。一時領域は作った直後に絶対パスへ解決してから使う。
# work_dir は解決と検証が成功するまで mktemp の生のパスのままにする（EXIT トラップが作ったディレクトリを確実に消せるように）。
if ! resolved_dir="$(cd "${work_dir}" 2> /dev/null && pwd -P && printf x)"; then
  echo "secret-scan-ci: cannot resolve the temporary directory" >&2
  exit 2
fi
resolved_dir="${resolved_dir%x}"
resolved_dir="${resolved_dir%$'\n'}"
case "${resolved_dir}" in
  /*) ;;
  *)
    echo "secret-scan-ci: cannot resolve the temporary directory" >&2
    exit 2
    ;;
esac
work_dir="${resolved_dir}"
# 検出器の mktemp なども絶対パスの一時領域を使うよう、TMPDIR も解決済みの値に置き換える。
export TMPDIR="${work_dir}"
out="${work_dir}/out"
tmp_index="${work_dir}/index"
if ! GIT_INDEX_FILE="${tmp_index}" g -C "${target}" read-tree "${head_sha}" > /dev/null 2>&1; then
  echo "secret-scan-ci: failed to build the temporary index from HEAD" >&2
  exit 2
fi

# 再走査の案内を出す。案内する検出器・除外一覧は、この実行が使った信頼済みの版に揃える。
# 保護された main の先端ではない（trust-root-secret-scan.yml は組織の ruleset が固定した SHA の版を使うため）。
# secret-scan.sh --tracked はコミットを引数に取らず index を走査するので、走査したコミットから専用の一時 index を作って渡す。
# 検出器は、走査対象と同じ checkout にあるか（検出器の実体のルートが走査対象と一致）で除外の規則が変わる（同じ checkout は
# 作業ツリー・HEAD・index の共通部分、別の checkout は検出器側と対象 index の共通部分）。別の checkout へ複写して流すと
# 除外が変わり得るため、元の実行が同じ checkout だったかを区別して案内する。
# 検出器の実体のファイルとルートは、検出器と同じ規則（自身のリンクを辿り、親ディレクトリは cd -P で解決し、2 階層上）で求める。
# 同じ checkout の判定は、実体のルートと走査対象の一致だけで決める（入口のルートとの一致は問わない）。入口のルートとの一致と
# git のルートそのものであることは、コミットとの照合（sha の判定）の条件としてだけ使う（満たさなければ作業ツリーの案内に倒す）。
# 同じ checkout でも、起動した実体が対象の ./scripts/hooks/secret-scan.sh と別のファイルなら、そのファイルを勧めず、
# 同じ入口を通して流すよう案内する（パスは出さない）。起動前に検査不能になった（検出器が通常のファイルでない・実行できない・
# 対象に入れない）ときは、実際に起動した検出器が無いので案内を出さない（固定の文だけ。#6145）。
# 別の checkout のときは、検出器（scripts/hooks/secret-scan.sh）と除外一覧（skills-lock.json）の作業ツリーのファイルを、
# コミットの blob と直接照合する（通常のファイルか・実行者にとって実行できるかがコミットの mode（100755 は実行できる・100644 は実行できない）と合うか・内容が同じか。skip-worktree・assume-unchanged・
# ignore された未追跡も見落とさない。読む量は blob の大きさ + 1、一覧は 65,536 バイトまで、時間は読み取りごとに 5 秒で、検出器の読み取りと同じ上限）。
# 前後の照合は、走査がその版を読んだことまでは証明しない（前後の間に差し替えて戻せる）。そのため、一致しても commit の sha は
# 案内に出さず、「この実行が使ったとおりの版を使う」とだけ案内する。一致しない・照合できないときは作業ツリーの版を使う案内にする（fail-closed）。
# 案内は検出の結果を変えない（終了コードは呼び出し側が決める。案内の失敗は無視する）。
# 詳細付きの再走査はファイル名・git の生の診断を出すため、非公開の端末で行い出力を公開の場へ出さないよう明記する（SECR-23）。
# 出す値は公開してよい sha（走査した commit と、一致しなかった説明のときの検出器の HEAD）と固定の文だけ（秘密値・パスを含まない）。
# bounded_copy <元> <先> <上限バイト>: 元の先頭から上限バイトだけを先へ複写する。検出器の一覧の読み取りと同じく読み取りごとに 5 秒で打ち切る
# （FIFO への差し替えで止まり続けない）。打ち切り・失敗は 1。案内のための処理であり、検出の結果は変えない。
bounded_copy() {
  local src="$1" dst="$2" limit="$3" pid wd rc=0
  rm -f -- "${dst}.timeout" 2> /dev/null || true
  head -c "${limit}" 2> /dev/null < "${src}" > "${dst}" &
  pid=$!
  # 監視は、最初に TERM の trap を張ってから sleep を起こす（trap より前に TERM が届いても sleep はまだ無く、残らない）。
  # trap は jobs -p で起動済みの子をすべて終わらせる（sleep の PID を変数へ入れる前に届いても残らない）。
  (
    trap - ERR EXIT
    trap 'kill $(jobs -p) > /dev/null 2>&1 || true; exit 0' TERM
    sleep 5 &
    wait $! || true
    : 2> /dev/null > "${dst}.timeout" || true
    kill "${pid}" > /dev/null 2>&1 || true
  ) > /dev/null 2>&1 < /dev/null &
  wd=$!
  wait "${pid}" 2> /dev/null || rc=$?
  kill "${wd}" > /dev/null 2>&1 || true
  wait "${wd}" > /dev/null 2>&1 || true
  [ "${rc}" -eq 0 ] && [ ! -e "${dst}.timeout" ]
}
# matches_commit <ルート> <sha> <パス> <無くてよいか 0|1> <上限バイト 0 は無し>:
# ルートの作業ツリーのファイルがコミットの blob と同じ（検出器が読むものと同じ）なら 0。照合できない・違うなら 1。
# 検出器の規則に合わせる: コミットに無いものは、無くてよい（skills-lock.json）で作業ツリーにも無いときだけ一致。
# 通常のファイルのみ（シンボリックリンクは不一致）、実行者にとって実行できるかはコミットの mode と一致、読む量と読み取りごとの時間（5 秒）に上限を置く。
matches_commit() {
  local root="$1" sha="$2" path="$3" optional="$4" max_bytes="$5"
  local file="${root}/${path}" ls_out want_mode want_id have_id blob_size copy
  ls_out="$(g -C "${root}" ls-tree --full-tree "${sha}" -- "${path}" 2> /dev/null && printf x)" || return 1
  ls_out="${ls_out%x}"
  if [ -z "${ls_out}" ]; then
    [ "${optional}" = 1 ] && [ ! -e "${file}" ] && [ ! -L "${file}" ]
    return
  fi
  want_mode="${ls_out%% *}"
  case "${want_mode}" in
    100644 | 100755) ;;
    *) return 1 ;;
  esac
  { [ -f "${file}" ] && [ ! -L "${file}" ]; } || return 1
  if [ "${want_mode}" = 100755 ]; then
    [ -x "${file}" ] || return 1
  else
    [ ! -x "${file}" ] || return 1
  fi
  want_id="$(g -C "${root}" rev-parse --verify "${sha}:${path}" 2> /dev/null)" || return 1
  blob_size="$(g -C "${root}" cat-file -s "${want_id}" 2> /dev/null)" || return 1
  case "${blob_size}" in
    '' | *[!0-9]*) return 1 ;;
  esac
  if [ "${max_bytes}" -gt 0 ] && [ "${blob_size}" -gt "${max_bytes}" ]; then
    return 1
  fi
  # コミットの大きさ + 1 バイトだけ読む。それより大きい作業ツリーのファイルは、複写の内容が blob と一致しない。
  copy="${work_dir}/hint-copy"
  bounded_copy "${file}" "${copy}" $((blob_size + 1)) || return 1
  { [ -f "${file}" ] && [ ! -L "${file}" ]; } || return 1
  have_id="$(g -C "${root}" hash-object --no-filters -- "${copy}" 2> /dev/null)" || return 1
  [ -n "${want_id}" ] && [ "${want_id}" = "${have_id}" ]
}
# resolve_real_file <起動パス>: 検出器が除外の一覧を読む実体のファイルとルートを、検出器と同じ規則で求める。
# 起動パス自身のリンクを（最大 40 回）辿り、その実体のディレクトリを物理パスで取って 2 階層上がルート。
# 結果は det_real_file（実体の物理パス）・det_real_root（求まらなければ空）・det_linked（自身がリンクなら 1）へ入れる。求まらないときは 1。
resolve_real_file() {
  local src="$1" hops=0 link link_dir real_dir
  det_real_file=""
  det_real_root=""
  det_linked=0
  [ -e "${src}" ] || return 1
  while [ -L "${src}" ]; do
    det_linked=1
    hops=$((hops + 1))
    [ "${hops}" -le 40 ] || return 1
    link="$(readlink -- "${src}" 2> /dev/null && printf x)" || return 1
    link="${link%x}"
    link="${link%$'\n'}"
    [ -n "${link}" ] || return 1
    case "${link}" in
      /*) src="${link}" ;;
      *)
        case "${src}" in
          */*) link_dir="${src%/*}" ;;
          *) link_dir="." ;;
        esac
        [ -n "${link_dir}" ] || link_dir="/"
        src="${link_dir}/${link}"
        ;;
    esac
  done
  case "${src}" in
    */*) link_dir="${src%/*}" ;;
    *) link_dir="." ;;
  esac
  [ -n "${link_dir}" ] || link_dir="/"
  real_dir="$(cd -P -- "${link_dir}" 2> /dev/null && pwd -P && printf x)" || return 1
  real_dir="${real_dir%x}"
  real_dir="${real_dir%$'\n'}"
  case "${real_dir}" in
    /*/*) ;;
    *) return 1 ;;
  esac
  det_real_file="${real_dir}/${src##*/}"
  det_real_root="${real_dir%/*}"
  det_real_root="${det_real_root%/*}"
  [ -n "${det_real_root}" ] || det_real_root="/"
}
# rescan_hint [state]: 再走査の案内の文を標準出力へ返す（出力は emit_hint が受ける）。
# state を渡すと、案内の分岐と検出器の HEAD だけを返す（走査の前の記録に使う。pre_state と走査の後の状態を比べる）。
rescan_hint() {
  local mode="${1:-hint}" trusted_sha root_real det_root top_real detector_src tail_text no_run_text private_text det_ok=0 branch=fallback loc_state=resolved
  local det_real_root="" det_real_file="" det_linked=0 tgt_file="" entry_file="" same_exact=0 lead_text
  # 走査対象の ./scripts/hooks/secret-scan.sh の実体（同じ規則で求める。求まらなければ空）を先に控える。
  if resolve_real_file "${target}/scripts/hooks/secret-scan.sh"; then
    tgt_file="${det_real_file}"
  fi
  if resolve_real_file "${ROOT_DIR}/scripts/hooks/secret-scan.sh"; then
    det_root="${det_real_root}"
    entry_file="${det_real_file}"
  else
    det_root=""
    loc_state=unresolved
  fi
  root_real="$(cd "${ROOT_DIR}" 2> /dev/null && pwd -P && printf x)" || root_real=""
  root_real="${root_real%x}"
  root_real="${root_real%$'\n'}"
  # sha の照合（matched）の条件: 実体のルート（検出器自身がリンクでも、辿った先の実体から求める）が入口のルートと一致し、
  # git のリポジトリのルートそのものであること（同じ checkout の判定には使わない。満たさなければ作業ツリーの案内に倒す）。
  if [ -n "${det_root}" ] && [ "${det_root}" = "${root_real}" ]; then
    top_real="$(g -C "${det_root}" rev-parse --show-toplevel 2> /dev/null && printf x)" || top_real=""
    top_real="${top_real%x}"
    top_real="${top_real%$'\n'}"
    if [ -n "${top_real}" ]; then
      top_real="$(cd "${top_real}" 2> /dev/null && pwd -P && printf x)" || top_real=""
      top_real="${top_real%x}"
      top_real="${top_real%$'\n'}"
    fi
    [ -n "${top_real}" ] && [ "${top_real}" = "${det_root}" ] && det_ok=1
  fi
  trusted_sha="unknown"
  if [ "${det_ok}" = 1 ]; then
    trusted_sha="$(g -C "${det_root}" rev-parse --verify "HEAD^{commit}" 2> /dev/null)" || trusted_sha="unknown"
  fi
  if [ -n "${det_root}" ] && [ "${det_root}" = "${target}" ]; then
    # 除外の規則は、検出器の実体のルートと走査対象の一致で決まる（検出器の判定と同じ）。入口のルートとの一致は問わない。
    branch=same
    [ -n "${tgt_file}" ] && [ "${tgt_file}" = "${entry_file}" ] && same_exact=1
  elif [ "${det_ok}" = 1 ] && [ "${det_linked}" = 0 ] && [ "${trusted_sha}" != "unknown" ] \
    && matches_commit "${det_root}" "${trusted_sha}" scripts/hooks/secret-scan.sh 0 0 \
    && matches_commit "${det_root}" "${trusted_sha}" skills-lock.json 1 65536; then
    branch=sha
  fi
  if [ "${mode}" = state ]; then
    printf '%s %s' "${branch}" "${trusted_sha}"
    return 0
  fi
  # 走査の前にも同じ sha で一致していたときだけ matched にする。前の記録が無い・違うときは作業ツリーの案内に倒す。
  # 前後の一致は、走査が読んだ版の証明ではない（前後の間に差し替えて戻せる）ため、matched でも sha は案内に出さない（#6145）。
  if [ "${branch}" = sha ] && [ "${pre_state}" != "sha ${trusted_sha}" ]; then
    branch=fallback
  fi
  if [ "${branch}" = same ]; then
    # 同じ checkout の実行: 検出器の実体は走査対象の作業ツリーにある。別の場所から流すと別の checkout の規則になるので、
    # この checkout の中で、対象の引数なしのまま流す（検出器と一覧がこの実行の時と同じ版で、HEAD も同じままであること）。
    if [ "${root_real}" = "${target}" ]; then
      lead_text="this run scanned this entry's own checkout"
    else
      lead_text="this run scanned the checkout that contains the detector's real file (its real root is the scanned repository, not this entry's own root)"
    fi
    detector_src="${lead_text}, so its detector and exclusion list come from that checkout's working tree and the same-checkout exclusion rules apply (the detector also reads the exclusion list at HEAD); do not copy the detector elsewhere or run it from another checkout, because that selects the other-checkout rules and can change which files are excluded; rescan inside this same checkout only while scripts/hooks/secret-scan.sh and skills-lock.json are unchanged from this run and HEAD is still ${head_sha}"
    if [ "${same_exact}" = 1 ]; then
      tail_text="run ./scripts/hooks/secret-scan.sh --tracked (without --summary and without a target argument) from the root of this same checkout with the same GIT_INDEX_FILE"
    else
      tail_text="run the same entry this run started, i.e. this entry's scripts/hooks/secret-scan.sh, which resolves through a symbolic link to a different file inside this checkout than ./scripts/hooks/secret-scan.sh (so that file is not the one that ran), with --tracked (without --summary and without a target argument) from the root of this same checkout with the same GIT_INDEX_FILE"
    fi
    no_run_text="do not run make"
  elif [ "${branch}" = sha ]; then
    detector_src="use the detector and exclusion list exactly as this run used them: the working-tree copies of scripts/hooks/secret-scan.sh and skills-lock.json matched the detector's commit before and after the scan, but this entry cannot prove which version the scan read (they could have been replaced and restored in between), so no commit is named; keep copies of the detector file and of skills-lock.json as this run used them (do not rely on the main tip)"
    tail_text="run <detector copy>/scripts/hooks/secret-scan.sh --tracked (without --summary) with the same GIT_INDEX_FILE, with skills-lock.json at the root two directories above it, as in the original layout"
    no_run_text="do not run make or any script from the scanned commit"
  elif [ "${loc_state}" = unresolved ]; then
    # 検出器の実体の所在を求められない: 再現に要るファイルを特定できない。コミットも main の先端も当てにしない。
    detector_src="the files this run needed cannot be identified, because the location of the detector it used could not be resolved; do not rely on any commit or the main tip, and reproduce the rescan from the original run environment (the detector and skills-lock.json exactly as that run read them)"
    tail_text="run <detector copy>/scripts/hooks/secret-scan.sh --tracked (without --summary) with the same GIT_INDEX_FILE"
    no_run_text="do not run make or any script from the scanned commit"
  else
    # 所在は求まったが、コミットと同じとは言えない（変更・リンク・別のルート等。一致を確かめられた場合は上の matched）。検出器の規則どおり、除外の一覧は
    # 実体のルート（リンクを辿った検出器のファイルの 2 階層上）から読まれる。パスは出さず、その規則だけを書く。
    detector_src="use the detector and exclusion list exactly as this run used them: the working-tree copies of scripts/hooks/secret-scan.sh and skills-lock.json differ from, or cannot be compared with, the commit (${trusted_sha}), so that commit does not reproduce them; the detector reads skills-lock.json from its real root, i.e. two directories above the directory of the detector file after following symbolic links, which can differ from this entry's own root; keep copies of the resolved detector file and of that skills-lock.json (do not rely on the commit or the main tip)"
    if [ "${det_linked}" = 1 ]; then
      detector_src="${detector_src}; the detector was started through a symbolic link, so copy the file it resolves to and place skills-lock.json at the matching root"
    fi
    tail_text="run <detector copy>/scripts/hooks/secret-scan.sh --tracked (without --summary) with the same GIT_INDEX_FILE, with skills-lock.json at the root two directories above it, as in the original layout"
    no_run_text="do not run make or any script from the scanned commit"
  fi
  private_text="Run this rescan only in a private local terminal: without --summary it prints file names and raw git diagnostics, which can contain secret values (SECR-23); never run it in a public CI job and never paste or upload its output to a public log, PR or issue."
  echo "secret-scan-ci: to see file names and diagnostics, ${detector_src}; then, with the scanned repository as the working directory and every existing GIT_* variable unset for both commands below, as this entry does (e.g. env -u GIT_DIR -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES -u GIT_WORK_TREE -u GIT_INDEX_FILE -u GIT_COMMON_DIR -u GIT_NAMESPACE; list others with env | grep ^GIT_), build a dedicated index from the scanned commit (${head_sha}) with GIT_NO_REPLACE_OBJECTS=1 GIT_INDEX_FILE=<new file> git -c core.hooksPath=/dev/null -c core.fsmonitor=false read-tree ${head_sha}, and ${tail_text}; ${no_run_text}. ${private_text}"
}

# emit_hint: 再走査の案内を 1 行だけ出す。案内は検出の結果（終了コード）を変えない。
# 案内の組み立てはサブシェルで行い、その stderr は捨てる（bash 自身の診断・ジョブの通知にパスや行番号が出得るため。SECR-23）。
# サブシェルでは ERR トラップ・EXIT トラップ（一時領域の削除）・errexit を外す。組み立てに失敗したら何も出さない。
emit_hint() {
  local hint=""
  # コマンド置換の shell 自身で stderr を捨て、トラップと errexit を外す（内側のサブシェルだけに掛けると、この shell の診断が漏れる）。
  hint="$(
    exec 2> /dev/null
    trap - ERR EXIT
    set +eE
    rescan_hint
  )" || hint=""
  if [ -n "${hint}" ]; then
    printf '%s\n' "${hint}" >&2
  fi
  return 0
}

# 走査の前の案内の状態（案内の分岐と検出器の HEAD）を記録する。失敗は無視し、検出の結果には関与しない。
pre_state="$(
  exec 2> /dev/null
  trap - ERR EXIT
  set +eE
  rescan_hint state
)" || pre_state=""

# 検出器の終了状態は 0 = 検出なし / 1 = 検出 / 2 = 検査不能。それ以外（起動失敗・シグナル等）も検査不能に畳む。
# 受け皿（out・filtered）を作れないときは、パス付きの bash の診断を出さず、固定の文で検査不能（2）にする（SECR-23）。
# 作れないまま進むと、サブシェルが動かず状態 1 を「検出」と、grep が動かず 1 を「該当なし」と取り違える。
if ! { : > "${out}"; } 2> /dev/null; then
  echo "secret-scan-ci: cannot record the scanner output; aborting" >&2
  exit 2
fi
rc=0
# 起動前の失敗（対象に入れない・検出器が通常のファイルでない・実行できない）と、サブシェルが動いたことは、印のファイルで親へ伝える。
# 印は案内を出すかと固定文の選択にだけ使い、終了コードの決定には使わない（起動前の失敗は常に 2）。
{
  (
    # サブシェルは set -E と ERR トラップを受け継ぐ。検出器の終了状態 1（検出）がトラップで 2 に変わらないよう、
    # ここでは外し、終了状態は下の明示の `exit 2` と検出器の終了状態だけで決める（bash の版による差も避ける）。
    trap - ERR
    set +E
    { : > "${work_dir}/started"; } 2> /dev/null || true
    cd "${target}" 2> /dev/null || {
      { : > "${work_dir}/not-started-target"; } 2> /dev/null || true
      exit 2
    }
    # 起動できない検出器（通常のファイルでない・実行できない）は、shell による起動失敗の終了状態の差（1 と取り違え得る）に頼らず、
    # 起動の前に検査不能（2）にする。起動できる検出器の判定・終了状態は変えない。
    if ! { [ -f "${ROOT_DIR}/scripts/hooks/secret-scan.sh" ] && [ -x "${ROOT_DIR}/scripts/hooks/secret-scan.sh" ]; }; then
      { : > "${work_dir}/not-started-detector"; } 2> /dev/null || true
      exit 2
    fi
    GIT_INDEX_FILE="${tmp_index}" "${ROOT_DIR}/scripts/hooks/secret-scan.sh" --tracked --summary
  ) > "${out}" 2>&1
} 2> /dev/null || rc=$?
if [ -e "${work_dir}/not-started-target" ]; then
  echo "secret-scan-ci: cannot enter the target directory; the scan was not started" >&2
  exit 2
fi
if [ -e "${work_dir}/not-started-detector" ]; then
  echo "secret-scan-ci: the detector is not an executable regular file; the scan was not started" >&2
  exit 2
fi
if [ ! -e "${work_dir}/started" ]; then
  echo "secret-scan-ci: cannot record the scanner output; aborting" >&2
  exit 2
fi
# 規則 ID は許可した一覧との完全一致だけを通す（任意の文字列・git の診断を公開ログへ通さない）。
# grep の終了状態は 0 = 一致 / 1 = 該当なし / 2 以上 = エラー。1 だけを「該当なし」として許す（受け皿を作れたときだけ）。
filtered="${work_dir}/filtered"
if ! { : > "${filtered}"; } 2> /dev/null; then
  echo "secret-scan-ci: cannot record the filtered scanner output; aborting" >&2
  exit 2
fi
grep_rc=0
grep -aE '^secret-scan: (env-file|vendor-token|private-key|jwt|jwk-private|credential-url|assignment): [0-9]+ (line|file)\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) in [0-9]+ file\(s\)$|^secret-scan: total: [0-9]+ finding\(s\) \([0-9]+ tracked file\(s\) checked\)$|^secret-scan: findings in tracked files$' "${out}" 2> /dev/null > "${filtered}" || grep_rc=$?
if [ "${grep_rc}" -gt 1 ]; then
  echo "secret-scan-ci: failed to filter the scanner output (grep exit ${grep_rc})" >&2
  exit 2
fi
# 許可した行だけを公開ログへ出し、走査完了の集計行（検出 0 件）の数と、それ以外の行の数を数える。
clean_re='^secret-scan: total: 0 finding\(s\) \([0-9]+ tracked file\(s\) checked\)$'
clean_lines=0
other_lines=0
# filtered を読めないときの bash の診断（一時パス付き）は出さず、固定の文で検査不能（2）にする。
if ! {
  while IFS= read -r line || [ -n "${line}" ]; do
    printf '%s\n' "${line}"
    if [[ ${line} =~ ${clean_re} ]]; then
      clean_lines=$((clean_lines + 1))
    else
      other_lines=$((other_lines + 1))
    fi
  done < "${filtered}"
} 2> /dev/null; then
  echo "secret-scan-ci: cannot read the filtered scanner output; aborting" >&2
  exit 2
fi
case "${rc}" in
  0)
    # 検出器が 0 を返しても、走査完了の集計行（検出 0 件）がちょうど 1 行で、ほかの許可行（検出の行）が無いことを
    # 合格の条件にする。欠けていたり矛盾していたりしたら、走査が完了した証拠が無いため検査不能（2）。
    if [ "${clean_lines}" -ne 1 ] || [ "${other_lines}" -ne 0 ]; then
      echo "secret-scan-ci: the scanner exited 0 without exactly one completion summary line; treating as incomplete" >&2
      exit 2
    fi
    ;;
  1)
    echo "secret-scan-ci: findings reported; details are withheld on the public log." >&2
    emit_hint || true
    exit 1
    ;;
  *)
    # 検査不能のときだけ、検出器の固定の診断文を許可リスト（lib.sh。陰性対照と共有）との行全体の完全一致で通す。
    # 値・パス・git の生の診断は通さない（SECR-23）。失敗は無視する（既に検査不能（2）で終える）。
    # 許可リストを作れたときだけ照合する（作れなければ何も通さない）。標準エラーは先に抑える（開けない旨の文にパスが出るため。SECR-23）。
    if secret_scan_fixed_diagnostics 2> /dev/null > "${work_dir}/allowed-diag"; then
      grep -aFx -f "${work_dir}/allowed-diag" "${out}" >&2 2> /dev/null || true
    fi
    echo "secret-scan-ci: scan could not be completed (exit ${rc}); details are withheld on the public log." >&2
    emit_hint || true
    exit 2
    ;;
esac

# 陰性対照の終了状態: 0 = 期待どおり / 1 = 検出能力の異常 / 2 以上 = 検査不能。
canary_rc=0
"${ROOT_DIR}/scripts/secret-scan-canary.sh" || canary_rc=$?
case "${canary_rc}" in
  0) ;;
  1) exit 1 ;;
  *) exit 2 ;;
esac
