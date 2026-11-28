#!/usr/bin/env bash
# human-result.sh — 反映の issue の前提にある人の作業（owner:human）の結果を、決まった規則で読む（読み取り専用。Issue #932）
#
# 使い方（通常は run-verified.sh 経由。ホストがコマンドを組み立てる）:
#   bash human-result.sh --nonce <nonce> --repo <owner/repo> --numbers <n,n,...>
#   --numbers は前提の issue 番号（昇順・重複なし・1〜20 件）。--repo は必須（省くと gh がカレントのリポジトリへ倒れるため exit 2。Issue #808 と同じ）。
#
# 役割: 人の作業の結果の読み方を、反映の issue の本文の文章ではなくスクリプトに固定する。Plan が本文を言い換えて要約しても、判定の式は
#       一字一句のまま実行される。判定はエンジンに依存しない規則（lib/human-result-lib.sh）で行い、fail-closed にする。
# 規則（番号ごと）:
#   - owner:human ラベル（ASCII の大文字小文字は区別しない）が無い（または PR）→ not-human（判定の対象外）
#   - state が closed でない → open
#   - assignee が 0 人 → no-assignee
#   - コメントのうち、書き手が assignee・author_association が OWNER / MEMBER / COLLABORATOR・1 行目が「結果:」の報告のものだけを候補にし、
#     id が最大の 1 件だけを採る。採った 1 件の種別がそのまま verdict（success / failure / not-done / malformed）。古い成功へはフォールバックしない
#   - 候補が無い → missing。取得失敗・形の崩れ・候補の過多 → fetch-failed
# 出力: 成功時は封筒 {nonce, payload, sha256}（lib/signed-output.sh）の 1 行だけ。ホストが照合してから payload を使う。
#   payload {"entries":[{"comment":C,"n":N,"verdict":"…"},…],"inputSha256":"…"}（entries は --numbers の順に全件。comment は採ったコメントの id、無ければ 0）
#   inputSha256 = sha256("human-result\n" + repo + "\n" + numbers)
# 終了コード:
#   0  封筒を出力した（番号ごとの失敗は verdict に出る）
#   2  使い方の誤り
#   3  gh・sha256sum / shasum が無い、または封筒を作れない
#   標準エラーには固定語彙だけを出す。コメント本文・login は出力しない（verdict と id だけ）。
# 自衛: gh api の GET だけ。書き込み・ファイル作成をしない。
# 互換: macOS（BSD userland・bash 3.2）と Linux（GNU）。

set -u
unset POSIXLY_CORRECT
LC_ALL=C
export LC_ALL

script_dir=$(cd "$(dirname "$0")" && pwd -P) || exit 2
# shellcheck source=lib/signed-output.sh
. "${script_dir}/lib/signed-output.sh" || exit 3
# shellcheck source=lib/base-merge-lib.sh
. "${script_dir}/lib/base-merge-lib.sh" || exit 3
# shellcheck source=lib/human-result-lib.sh
. "${script_dir}/lib/human-result-lib.sh" || exit 3

usage() {
  echo "使い方: human-result.sh --nonce NONCE --repo OWNER/REPO --numbers N,N,..." >&2
  [ $# -eq 0 ] || echo "human-result: $1" >&2
  exit 2
}

nonce=""
repo=""
have_repo=0
numbers=""
have_numbers=0
while [ $# -gt 0 ]; do
  case "$1" in
    --nonce) [ $# -ge 2 ] || usage "--nonce に値が無い"; [ -z "${nonce}" ] || usage "--nonce は 1 回だけ"; nonce=$2; shift 2 ;;
    --repo) [ $# -ge 2 ] || usage "--repo に値が無い"; [ "${have_repo}" -eq 0 ] || usage "--repo は 1 回だけ"; repo=$2; have_repo=1; shift 2 ;;
    --numbers) [ $# -ge 2 ] || usage "--numbers に値が無い"; [ "${have_numbers}" -eq 0 ] || usage "--numbers は 1 回だけ"; numbers=$2; have_numbers=1; shift 2 ;;
    *) usage "未知の引数" ;;
  esac
done
so_valid_nonce "${nonce}" || usage "--nonce の形式が不正"
[ "${have_repo}" -eq 1 ] || usage "--repo は必須"
valid_repo "${repo}" || usage "--repo は OWNER/REPO の形で指定する"
[ "${have_numbers}" -eq 1 ] || usage "--numbers は必須"
hr_valid_numbers_csv "${numbers}" || usage "--numbers の形式が不正（昇順・重複なしの番号のカンマ区切り・1〜${HR_NUMBERS_MAX} 件）"

command -v gh >/dev/null 2>&1 || { echo "human-result: gh が無い" >&2; exit 3; }
input_sha=$(hr_input_sha "${repo}" "${numbers}") || { echo "human-result: 入力のハッシュを計算できない（sha256sum / shasum が無い等）" >&2; exit 3; }

base="repos/${repo}"

# 1 件の判定。結果は HR_VERDICT と HR_COMMENT に置く
hr_judge() {
  local n=$1 line pr st human lg extra rows row id login assoc kind best_id best_kind cnt
  HR_VERDICT="fetch-failed"
  HR_COMMENT=0
  line=$(gh api "${base}/issues/${n}" --jq "${HR_ISSUE_JQ}" 2>/dev/null </dev/null) || return 0
  extra=""
  read -r pr st human lg extra <<HR_EOF
${line}
HR_EOF
  [ -z "${extra}" ] || return 0
  case "${pr}:${human}" in
    true:true | true:false | false:true | false:false) ;;
    *) return 0 ;;
  esac
  case "${st}" in open | closed) ;; *) return 0 ;; esac
  case "${lg}" in L:*) lg=${lg#L:} ;; *) return 0 ;; esac
  if [ "${pr}" = true ] || [ "${human}" != true ]; then HR_VERDICT="not-human"; return 0; fi
  if [ "${st}" != closed ]; then HR_VERDICT="open"; return 0; fi
  if [ -z "${lg}" ]; then HR_VERDICT="no-assignee"; return 0; fi
  local l assignees=","
  while IFS= read -r l; do
    hr_valid_login "${l}" || return 0
    assignees="${assignees}${l},"
  done <<HR_EOF
$(printf '%s\n' "${lg}" | tr ',' '\n')
HR_EOF
  rows=$(gh api --paginate "${base}/issues/${n}/comments?per_page=100" --jq "${HR_COMMENTS_JQ}" 2>/dev/null </dev/null) || return 0
  best_id=""
  best_kind=""
  cnt=0
  while IFS= read -r row; do
    [ -n "${row}" ] || continue
    cnt=$((cnt + 1))
    [ "${cnt}" -le "${HR_ROWS_MAX}" ] || return 0
    extra=""
    read -r id login assoc kind extra <<HR_EOF
${row}
HR_EOF
    [ -z "${extra}" ] || return 0
    hr_valid_id "${id}" || return 0
    hr_valid_login "${login}" || return 0
    case "${assoc}" in OWNER | MEMBER | COLLABORATOR) ;; *) return 0 ;; esac
    case "${kind}" in success | failure | not-done | malformed) ;; *) return 0 ;; esac
    case "${assignees}" in *",${login},"*) ;; *) continue ;; esac
    if [ -z "${best_id}" ] || hr_id_gt "${id}" "${best_id}"; then
      best_id=${id}
      best_kind=${kind}
    fi
  done <<HR_EOF
${rows}
HR_EOF
  if [ -z "${best_id}" ]; then HR_VERDICT="missing"; return 0; fi
  HR_VERDICT=${best_kind}
  HR_COMMENT=${best_id}
}

entries=""
for n in $(printf '%s\n' "${numbers}" | tr ',' ' '); do
  hr_judge "${n}"
  entries="${entries}${entries:+,}$(hr_entry "${HR_COMMENT}" "${n}" "${HR_VERDICT}")"
done
env=$(so_emit "${nonce}" "$(hr_payload "${entries}" "${input_sha}")") || { echo "human-result: 出力封筒を作れなかった" >&2; exit 3; }
printf '%s\n' "${env}"
exit 0
