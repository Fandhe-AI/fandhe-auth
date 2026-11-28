#!/usr/bin/env bash
# CI のジョブごとの実行時間の記録を取る（TASK-6 9/10。#56）。結果は TASK-66（時間の予算の決定。利用者が決める）の入力になる。
#
# 終わった実行を GitHub の REST（GET のみ）から読み、許可した列だけの TSV にし、系統ごとに集計して Markdown で出す。
# ワークフローは変えず、CI の中でも流さない（読み取り専用。make ci-timing は手元で流す入口で、ネットワークと gh の認証が要る）。
# 記録の形式・集計の定義・読み方の正は docs/records/task-6-ci-timing.md。予算の値・合否は決めない（ACPT-7）。
#
# 使い方:
#   scripts/ci-timing.sh report
#   scripts/ci-timing.sh fetch --workflow <file> --event <event> [--branch <name>] [--limit <1..100>]
#   scripts/ci-timing.sh rows [--workflow <file>] [--event <event>] [FILE]   jobs API の JSON（{"jobs":[...]}）を行の TSV にする（オフライン）
#   scripts/ci-timing.sh summary [FILE]                                       行の TSV を集計して Markdown で出す
# 終了コード: 0 = 成功、2 = 誤用、または取得・検査ができない（0 件の取得も 2。空の記録を成功にしない）。
# 出す列は許可リストに限る（runner の名前・label・step・URL・ブランチ名は出さない。SECR-23。実機の名前を書かない規約）。
# 認証は gh の既存の資格情報をそのまま使う。トークンを読まず・出さず・環境変数へ書かない。宛先は gh の既定の GitHub だけ。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"
cd "${ROOT_DIR}"

# 時刻は jq の fromdateiso8601 で秒にする（macOS の date -d を使わない）。可変の値は --arg で渡し、フィルタは固定の文字列にする。
# タブ・改行は空白に置き換え、列を壊さない。欠けた時刻は "-"。
# shellcheck disable=SC2016 # jq のプログラムで、シェルに展開させない
ROWS_FILTER='
def ts: if . == null then "-" else (fromdateiso8601 | tostring) end;
.jobs[] | [
  $wf, $ev, (.run_id | tostring), (.run_attempt | tostring), (.head_sha[0:12]),
  (.name | gsub("[\t\r\n]"; " ")), (.conclusion // "-"),
  (.created_at | ts), (.started_at | ts), (.completed_at | ts)
] | @tsv'

usage() {
  echo "usage: ci-timing.sh report | fetch --workflow F --event E [--branch B] [--limit N] | rows [--workflow F] [--event E] [FILE] | summary [FILE]" >&2
  exit 2
}

die() {
  echo "ci-timing: $*" >&2
  exit 2
}

TMP_FILES=()
cleanup() {
  if [ "${#TMP_FILES[@]}" -gt 0 ]; then
    rm -f -- "${TMP_FILES[@]}"
  fi
}
trap cleanup EXIT

# 一時ファイルを作って登録し、パスを NEW_TMP に返す（サブシェルを介さず trap の対象にする）。
NEW_TMP=""
new_tmp() {
  NEW_TMP="$(mktemp)" || die "mktemp failed"
  TMP_FILES+=("${NEW_TMP}")
}

# API のパスに入る値は許可リストで先に検証する（A03）。
check_name() {
  case "$2" in
    '' | *[!A-Za-z0-9._/-]*) die "invalid $1" ;;
  esac
}

check_limit() {
  case "$1" in
    '' | *[!0-9]* | 0*) die "invalid --limit (1..100)" ;;
  esac
  [ "$1" -le 100 ] || die "invalid --limit (1..100)"
}

OPT_WF="-"
OPT_EV="-"
OPT_BRANCH=""
OPT_LIMIT=30
OPT_FILE=""

# parse_opts <許可する名前（workflow event branch limit file）> "$@"
parse_opts() {
  local allowed="$1"
  shift
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --workflow | --event | --branch | --limit)
        [ "$#" -ge 2 ] || die "missing value for $1"
        case " ${allowed} " in *" ${1#--} "*) ;; *) die "unknown option: $1" ;; esac
        case "$1" in
          --workflow)
            check_name workflow "$2"
            case "$2" in */* | .*) die "invalid workflow" ;; esac
            OPT_WF="$2"
            ;;
          --event)
            check_name event "$2"
            OPT_EV="$2"
            ;;
          --branch)
            check_name branch "$2"
            OPT_BRANCH="$2"
            ;;
          --limit)
            check_limit "$2"
            OPT_LIMIT="$2"
            ;;
        esac
        shift 2
        ;;
      -*) die "unknown option: $1" ;;
      *)
        case " ${allowed} " in *" file "*) ;; *) die "unexpected argument" ;; esac
        [ -z "${OPT_FILE}" ] || die "unexpected argument"
        OPT_FILE="$1"
        shift
        ;;
    esac
  done
}

# rows_from: jobs API の JSON（OPT_FILE か stdin）を行の TSV にする。
rows_from() {
  require_cmd jq || exit 2
  if [ -n "${OPT_FILE}" ]; then
    jq -r --arg wf "${OPT_WF}" --arg ev "${OPT_EV}" "${ROWS_FILTER}" "${OPT_FILE}" || die "cannot parse the jobs JSON"
  else
    jq -r --arg wf "${OPT_WF}" --arg ev "${OPT_EV}" "${ROWS_FILTER}" || die "cannot parse the jobs JSON"
  fi
}

# fetch_rows: 完了した実行ごとの jobs を読んで行の TSV を出す。対象の実行が 0 件なら 3（呼び出し側が扱いを決める）。
# GET だけを使う（gh api の -f・-F・-X・--input は渡さない。-f は POST に変わる）。
fetch_rows() {
  local runs_json jobs_json ids id total kept query
  require_cmd gh || exit 2
  require_cmd jq || exit 2
  if [ "${OPT_WF}" = "-" ] || [ "${OPT_EV}" = "-" ]; then
    die "fetch needs --workflow and --event"
  fi
  query="status=completed&event=${OPT_EV}&per_page=${OPT_LIMIT}"
  if [ -n "${OPT_BRANCH}" ]; then
    query="${query}&branch=${OPT_BRANCH}"
  fi
  new_tmp
  runs_json="${NEW_TMP}"
  new_tmp
  jobs_json="${NEW_TMP}"
  gh api "repos/{owner}/{repo}/actions/workflows/${OPT_WF}/runs?${query}" > "${runs_json}" 2> /dev/null || die "gh api failed (runs)"
  ids="$(jq -r '.workflow_runs[] | select(.conclusion == "success" or .conclusion == "failure") | .id | tostring' "${runs_json}")" || die "cannot parse the runs JSON"
  total="$(jq -r '.workflow_runs | length' "${runs_json}")" || die "cannot parse the runs JSON"
  kept=0
  for id in ${ids}; do
    kept=$((kept + 1))
  done
  echo "ci-timing: ${OPT_WF} ${OPT_EV}: ${total} completed run(s), $((total - kept)) not success/failure (excluded)" >&2
  if [ "${kept}" -eq 0 ]; then
    return 3
  fi
  for id in ${ids}; do
    case "${id}" in '' | *[!0-9]*) die "unexpected run id" ;; esac
    gh api "repos/{owner}/{repo}/actions/runs/${id}/jobs?per_page=100" > "${jobs_json}" 2> /dev/null || die "gh api failed (jobs)"
    # 1 ページ（100 件）に収まらない実行は、静かに欠落させず失敗にする。
    jq -e '(.total_count // 0) <= (.jobs | length)' "${jobs_json}" > /dev/null || die "jobs exceed one page (per_page=100); not supported"
    jq -r --arg wf "${OPT_WF}" --arg ev "${OPT_EV}" "${ROWS_FILTER}" "${jobs_json}" || die "cannot parse the jobs JSON"
  done
}

# summary_from: 行の TSV（OPT_FILE か stdin）を、系統（workflow / event）×ジョブと系統ごとの実行で集計する。
# 分位は最近傍順位（昇順の ceil(p×n) 番目）。単位は整数の秒。出力は決定的（LC_ALL=C・キーの順）。
summary_from() {
  local awk_prog
  require_cmd awk || exit 2
  # shellcheck disable=SC2016 # awk のプログラムで、シェルに展開させない
  awk_prog='
function bad(why) { print "ci-timing: invalid input (" why ")" > "/dev/stderr"; failed = 1; exit 2 }
function isnum(s) { return s ~ /^[0-9]+$/ }
function sortarr(a, n,   i, j, v) {
  for (i = 2; i <= n; i++) { v = a[i]; for (j = i - 1; j >= 1 && a[j] > v; j--) a[j + 1] = a[j]; a[j + 1] = v }
}
function pct(a, n, p,   k) { k = int((p * n + 99) / 100); if (k < 1) k = 1; return a[k] }
function esc(s) { gsub(/\|/, "\\|", s); return s }
{
  if (NF != 10) bad("column count")
  for (i = 8; i <= 10; i++) if ($i !~ /^([0-9]+|-)$/) bad("epoch")
  n++
  sys = $1 " / " $2
  jk = sys SUBSEP $6
  jobset[jk] = 1; jsys[jk] = sys; jname[jk] = $6
  if (($7 == "success" || $7 == "failure") && isnum($8) && isnum($9) && isnum($10) && $10 + 0 >= $9 + 0 && $9 + 0 >= $8 + 0) {
    c = ++cnt[jk]
    ex[jk, c] = $10 - $9
    wt[jk, c] = $9 - $8
    rk = sys SUBSEP $3 SUBSEP $4
    if (!(rk in rsys)) { rsys[rk] = sys; rmin[rk] = $8 + 0; rmax[rk] = $10 + 0; rsum[rk] = 0 }
    if ($8 + 0 < rmin[rk]) rmin[rk] = $8 + 0
    if ($10 + 0 > rmax[rk]) rmax[rk] = $10 + 0
    rsum[rk] += $10 - $9
  } else {
    excl[jk]++
  }
}
END {
  if (failed) exit 2
  if (n == 0) bad("empty input")
  nj = 0; for (k in jobset) jl[++nj] = k
  sortarr(jl, nj)
  print "### Per job (seconds)"
  print ""
  print "| workflow / event | job | counted / excluded | exec median | exec p90 | exec max | wait median | wait p90 | wait max |"
  print "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
  for (j = 1; j <= nj; j++) {
    k = jl[j]; m = cnt[k] + 0
    line = "| " esc(jsys[k]) " | " esc(jname[k]) " | " m " / " (excl[k] + 0) " |"
    if (m == 0) { line = line " - | - | - | - | - | - |" }
    else {
      for (i = 1; i <= m; i++) t[i] = ex[k, i]
      sortarr(t, m)
      line = line " " pct(t, m, 50) " | " pct(t, m, 90) " | " t[m] " |"
      for (i = 1; i <= m; i++) t[i] = wt[k, i]
      sortarr(t, m)
      line = line " " pct(t, m, 50) " | " pct(t, m, 90) " | " t[m] " |"
    }
    print line
  }
  print ""
  ns = 0; for (k in jsys) if (!(jsys[k] in seen)) { seen[jsys[k]] = 1; sl[++ns] = jsys[k] }
  sortarr(sl, ns)
  print "### Per run (seconds; wall = max(completed) - min(created) over counted jobs, job sum = sum of exec)"
  print ""
  print "| workflow / event | runs | wall median | wall p90 | wall max | job sum median | job sum p90 | job sum max |"
  print "| --- | --- | --- | --- | --- | --- | --- | --- |"
  for (s = 1; s <= ns; s++) {
    m = 0
    for (k in rsys) if (rsys[k] == sl[s]) { m++; w[m] = rmax[k] - rmin[k]; q[m] = rsum[k] }
    line = "| " esc(sl[s]) " | " m " |"
    if (m == 0) { line = line " - | - | - | - | - | - |" }
    else {
      sortarr(w, m); sortarr(q, m)
      line = line " " pct(w, m, 50) " | " pct(w, m, 90) " | " w[m] " | " pct(q, m, 50) " | " pct(q, m, 90) " | " q[m] " |"
    }
    print line
  }
}'
  if [ -n "${OPT_FILE}" ]; then
    LC_ALL=C awk -F '\t' "${awk_prog}" "${OPT_FILE}" || exit 2
  else
    LC_ALL=C awk -F '\t' "${awk_prog}" || exit 2
  fi
}

cmd_report() {
  local spec wf ev br rows_file rc
  [ "$#" -eq 0 ] || usage
  require_cmd gh || exit 2
  require_cmd jq || exit 2
  echo "# CI timing report"
  echo ""
  echo "- fetched (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- origin/main: $(git rev-parse origin/main 2> /dev/null || echo unknown)"
  echo "- tools: $(gh --version 2> /dev/null | head -n 1), $(jq --version 2> /dev/null)"
  for spec in "ci.yml pull_request -" "ci.yml push main" "ai-review.yml pull_request -" "scheduled.yml workflow_dispatch -" "update-external.yml schedule -"; do
    # shellcheck disable=SC2086 # 固定の 3 語を分割する
    set -- ${spec}
    wf="$1"
    ev="$2"
    br="$3"
    OPT_WF="${wf}"
    OPT_EV="${ev}"
    OPT_BRANCH=""
    if [ "${br}" != "-" ]; then
      OPT_BRANCH="${br}"
    fi
    echo ""
    echo "## ${wf} / ${ev}${OPT_BRANCH:+ (${OPT_BRANCH})}"
    echo ""
    new_tmp
    rows_file="${NEW_TMP}"
    rc=0
    fetch_rows > "${rows_file}" || rc=$?
    if [ "${rc}" -eq 3 ]; then
      echo "0 runs"
    elif [ "${rc}" -ne 0 ]; then
      die "fetch failed for ${wf} / ${ev}"
    else
      OPT_FILE="${rows_file}"
      summary_from
    fi
  done
}

[ "$#" -ge 1 ] || usage
sub="$1"
shift
case "${sub}" in
  report) cmd_report "$@" ;;
  fetch)
    parse_opts "workflow event branch limit" "$@"
    rc=0
    fetch_rows || rc=$?
    if [ "${rc}" -eq 3 ]; then
      die "no completed runs"
    fi
    exit "${rc}"
    ;;
  rows)
    parse_opts "workflow event file" "$@"
    rows_from
    ;;
  summary)
    parse_opts "file" "$@"
    summary_from
    ;;
  *) usage ;;
esac
