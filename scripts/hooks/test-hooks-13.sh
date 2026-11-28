#!/usr/bin/env bash
# scripts/hooks/ のフックスクリプト自体の回帰テストの part 13（TASK-6 9/10。#56）。scripts/ci-timing.sh をネットワークなしで流す。
# 入口は test-hooks.sh（`make test-hooks`・CI の test-hooks ジョブ）。jq が要る。無ければ CI（CI か GITHUB_ACTIONS が設定）では失敗、ローカルでは TAP の `# SKIP`（合格に数えない）。
# fetch は PATH の先頭に置いた偽の gh で流し、GET だけを使うこと（-f・-F・-X・--method・--input を渡さない）を確かめる。
# 期待は緩めない（AUDB-6）。part の間で状態は引き継がない。
set -euo pipefail
# shellcheck source=./test-hooks-core.sh
source "$(dirname "${BASH_SOURCE[0]}")/test-hooks-core.sh"

cd "${work_dir}"

if ! command -v jq > /dev/null 2>&1; then
  if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then
    fail "ci-timing: jq is required in CI" "missing prerequisite"
  else
    echo "ok - ci-timing # SKIP jq not found (not counted as a pass)"
  fi
  finish "part 13"
  exit 0
fi

ct="${HOOKS_DIR}/../ci-timing.sh"
ct_in="${work_dir}/ct-in"
mkdir -p "${ct_in}"

# 待ちの長いジョブ・skipped（completed が started より前）・cancelled・未完了・タブ入りの名前・目印入りの runner を含む fixture。
cat > "${ct_in}/jobs.json" << 'EOF'
{"jobs":[
{"run_id":7,"run_attempt":1,"head_sha":"0123456789abcdef","name":"build","conclusion":"success","created_at":"2026-01-01T00:00:00Z","started_at":"2026-01-01T00:08:00Z","completed_at":"2026-01-01T00:08:30Z","runner_name":"MARK-RUNNER","runner_group_name":"MARK-GROUP","labels":["MARK-LABEL"],"html_url":"MARK-URL","steps":[{"name":"MARK-STEP"}]},
{"run_id":7,"run_attempt":1,"head_sha":"0123456789abcdef","name":"tab\tname","conclusion":"failure","created_at":"2026-01-01T00:00:00Z","started_at":"2026-01-01T00:00:10Z","completed_at":"2026-01-01T00:01:10Z"},
{"run_id":7,"run_attempt":1,"head_sha":"0123456789abcdef","name":"skipped","conclusion":"skipped","created_at":"2026-01-01T00:00:00Z","started_at":"2026-01-01T00:08:00Z","completed_at":"2026-01-01T00:07:00Z"},
{"run_id":7,"run_attempt":1,"head_sha":"0123456789abcdef","name":"cancelled","conclusion":"cancelled","created_at":"2026-01-01T00:00:00Z","started_at":"2026-01-01T00:00:05Z","completed_at":"2026-01-01T00:00:06Z"},
{"run_id":7,"run_attempt":1,"head_sha":"0123456789abcdef","name":"pending","conclusion":null,"created_at":"2026-01-01T00:00:00Z","started_at":null,"completed_at":null}
]}
EOF

# ct_run <名前> <期待の終了状態> <出力の断片（空可）> <引数...>: stdin は空。出力は ct_out に残す。
ct_out=""
ct_run() {
  local name="$1" want="$2" frag="$3" rc=0
  shift 3
  ct_out="$("${HOOK_BASH}" "${ct}" "$@" 2>&1 < /dev/null)" || rc=$?
  if [ "${rc}" -eq "${want}" ] && { [ -z "${frag}" ] || [[ "${ct_out}" == *"${frag}"* ]]; }; then
    pass "ci-timing: ${name}"
  else
    fail "ci-timing: ${name}" "expected exit ${want} with '${frag}', got ${rc}: ${ct_out}"
  fi
}

# (1) rows: 10 列・秒・除外・許可リスト
ct_run "rows: ok" 0 "" rows --workflow ci.yml --event push "${ct_in}/jobs.json"
ct_rows="${ct_out}"
bad_cols="$(printf '%s\n' "${ct_rows}" | awk -F '\t' 'NF != 10' | wc -l | tr -d ' ')"
if [ "${bad_cols}" = "0" ] && [ "$(printf '%s\n' "${ct_rows}" | wc -l | tr -d ' ')" = "5" ]; then
  pass "ci-timing: rows has 5 rows of 10 columns"
else
  fail "ci-timing: rows has 5 rows of 10 columns" "${ct_rows}"
fi
want_build="$(printf 'ci.yml\tpush\t7\t1\t0123456789ab\tbuild\tsuccess\t1767225600\t1767226080\t1767226110')"
case "${ct_rows}" in
  *"${want_build}"*) pass "ci-timing: rows epochs" ;;
  *) fail "ci-timing: rows epochs" "${ct_rows}" ;;
esac
case "${ct_rows}" in
  *"tab name"*) pass "ci-timing: rows replaces tab in job name" ;;
  *) fail "ci-timing: rows replaces tab in job name" "${ct_rows}" ;;
esac
case "${ct_rows}" in
  *"pending"*"-"*"-"*"-"*) pass "ci-timing: rows marks missing times" ;;
  *) fail "ci-timing: rows marks missing times" "${ct_rows}" ;;
esac
if printf '%s' "${ct_rows}" | grep -q 'MARK-'; then
  fail "ci-timing: rows excludes runner, label, step and url" "marker leaked"
else
  pass "ci-timing: rows excludes runner, label, step and url"
fi

# (2) summary: 既知の値から分位を確かめる
printf '%s\n' "${ct_rows}" > "${ct_in}/rows.tsv"
ct_run "summary: ok" 0 "" summary "${ct_in}/rows.tsv"
ct_sum="${ct_out}"
case "${ct_sum}" in
  *"| ci.yml / push | build | 1 / 0 | 30 | 30 | 30 | 480 | 480 | 480 |"*) pass "ci-timing: summary per job" ;;
  *) fail "ci-timing: summary per job" "${ct_sum}" ;;
esac
case "${ct_sum}" in
  *"| ci.yml / push | skipped | 0 / 1 | - | - | - | - | - | - |"*) pass "ci-timing: summary excludes skipped" ;;
  *) fail "ci-timing: summary excludes skipped" "${ct_sum}" ;;
esac
case "${ct_sum}" in
  *"| ci.yml / push | cancelled | 0 / 1 |"*) pass "ci-timing: summary excludes cancelled" ;;
  *) fail "ci-timing: summary excludes cancelled" "${ct_sum}" ;;
esac
# 壁時計 = max(completed) - min(created) = 1767226110 - 1767225600 = 510、合計 = 30 + 60 = 90
case "${ct_sum}" in
  *"| ci.yml / push | 1 | 510 | 510 | 510 | 90 | 90 | 90 |"*) pass "ci-timing: summary per run" ;;
  *) fail "ci-timing: summary per run" "${ct_sum}" ;;
esac
ct_run "summary: deterministic" 0 "" summary "${ct_in}/rows.tsv"
if [ "${ct_out}" = "${ct_sum}" ]; then
  pass "ci-timing: summary is deterministic"
else
  fail "ci-timing: summary is deterministic" "outputs differ"
fi

# 分位: 実行 10, 20, 30, 40 秒の 4 件（偶数件）→ 中央値は 2 番目 = 20、p90 は 4 番目 = 40
: > "${ct_in}/q.tsv"
for ct_d in 10 20 30 40; do
  printf 'w.yml\te\t%s\t1\tabc\tj\tsuccess\t100\t100\t%s\n' "${ct_d}" "$((100 + ct_d))" >> "${ct_in}/q.tsv"
done
ct_run "summary: even count quantiles" 0 "| w.yml / e | j | 4 / 0 | 20 | 40 | 40 | 0 | 0 | 0 |" summary "${ct_in}/q.tsv"
printf 'w.yml\te\t1\t1\tabc\tj\tsuccess\t100\t100\t107\n' > "${ct_in}/one.tsv"
ct_run "summary: single sample" 0 "| w.yml / e | j | 1 / 0 | 7 | 7 | 7 | 0 | 0 | 0 |" summary "${ct_in}/one.tsv"

# (3) 異常系（fail-closed）
printf 'a\tb\tc\n' > "${ct_in}/cols.tsv"
ct_run "summary: column mismatch is exit 2" 2 "invalid input" summary "${ct_in}/cols.tsv"
printf 'w.yml\te\t1\t1\tabc\tj\tsuccess\tx\t100\t107\n' > "${ct_in}/nan.tsv"
ct_run "summary: non-numeric epoch is exit 2" 2 "invalid input" summary "${ct_in}/nan.tsv"
: > "${ct_in}/empty.tsv"
ct_run "summary: empty input is exit 2" 2 "empty input" summary "${ct_in}/empty.tsv"
printf '{broken' > "${ct_in}/broken.json"
ct_run "rows: broken JSON is exit 2" 2 "cannot parse" rows "${ct_in}/broken.json"
ct_run "fetch: path traversal workflow is exit 2" 2 "invalid workflow" fetch --workflow ../x --event push
ct_run "fetch: dot-leading workflow is exit 2" 2 "invalid workflow" fetch --workflow .. --event push
ct_run "fetch: slash workflow is exit 2" 2 "invalid workflow" fetch --workflow a/b --event push
ct_run "fetch: space in workflow is exit 2" 2 "invalid workflow" fetch --workflow "a b" --event push
ct_run "fetch: limit 0 is exit 2" 2 "invalid --limit" fetch --workflow ci.yml --event push --limit 0
ct_run "fetch: limit 101 is exit 2" 2 "invalid --limit" fetch --workflow ci.yml --event push --limit 101
ct_run "fetch: limit abc is exit 2" 2 "invalid --limit" fetch --workflow ci.yml --event push --limit abc
ct_run "unknown subcommand is exit 2" 2 "usage" bogus

# (4) fetch: 偽の gh。呼び出しの引数を記録し、GET だけを使うことを確かめる
ct_bin="${work_dir}/ct-bin"
ct_log="${work_dir}/ct-gh.log"
mkdir -p "${ct_bin}"
cat > "${ct_bin}/gh" << 'EOF'
#!/bin/sh
# 偽の gh。引数を記録し、FAKE_GH_MODE で応答を変える。
printf '%s\n' "$*" >> "${FAKE_GH_LOG}"
case "$*" in
  "api repos/{owner}/{repo} "*) echo "example-owner/example-repo"; exit 0 ;;
esac
case "${FAKE_GH_MODE}" in
  fail) exit 1 ;;
  empty) echo '{"workflow_runs":[]}' ;;
  ok)
    case "$*" in
      *"/jobs"*) cat "${FAKE_GH_JOBS}" ;;
      *) echo '{"workflow_runs":[{"id":7,"conclusion":"success"},{"id":8,"conclusion":"cancelled"}]}' ;;
    esac
    ;;
esac
EOF
chmod +x "${ct_bin}/gh"

ct_fetch() {
  local name="$1" want="$2" frag="$3" mode="$4" rc=0
  : > "${ct_log}"
  ct_out="$(PATH="${ct_bin}:${PATH}" FAKE_GH_MODE="${mode}" FAKE_GH_LOG="${ct_log}" FAKE_GH_JOBS="${ct_in}/jobs.json" "${HOOK_BASH}" "${ct}" fetch --workflow ci.yml --event push --branch main --limit 5 2>&1 < /dev/null)" || rc=$?
  if [ "${rc}" -eq "${want}" ] && { [ -z "${frag}" ] || [[ "${ct_out}" == *"${frag}"* ]]; }; then
    pass "ci-timing: ${name}"
  else
    fail "ci-timing: ${name}" "expected exit ${want} with '${frag}', got ${rc}: ${ct_out}"
  fi
}

ct_fetch "fetch: gh failure is exit 2" 2 "gh api failed" fail
ct_fetch "fetch: zero runs is exit 2" 2 "no completed runs" empty
ct_fetch "fetch: ok" 0 "0123456789ab" ok
ct_runs_calls="$(grep -c 'actions/workflows/ci.yml/runs' "${ct_log}" || true)"
ct_jobs_calls="$(grep -c 'actions/runs/7/jobs' "${ct_log}" || true)"
if [ "${ct_runs_calls}" = "1" ] && [ "${ct_jobs_calls}" = "1" ]; then
  pass "ci-timing: fetch reads runs once and jobs of success/failure runs only"
else
  fail "ci-timing: fetch reads runs once and jobs of success/failure runs only" "runs=${ct_runs_calls} jobs=${ct_jobs_calls}"
fi
# gh api の -f・-F は POST に変わる。GET だけを使う回帰（gh repo view・graphql は POST /graphql なので同じく検出する）
ct_re='(^| )(-[fFX]|--method|--input|--field|--raw-field|(-H|--header)[ =]?X-HTTP-Method-Override|repo view|api graphql)'
ct_nonget="$(grep -Eic "${ct_re}" "${ct_log}" || true)"
if [ "${ct_nonget}" = "0" ]; then
  pass "ci-timing: fetch uses GET only"
else
  fail "ci-timing: fetch uses GET only" "non-GET flags in ${ct_nonget} call(s)"
fi

# report: 解決した repo を冒頭に出す（REST の GET。fetch と同じ解決）。ジョブ名の Markdown の記号を逃がす
: > "${ct_log}"
ct_rep="$(PATH="${ct_bin}:${PATH}" FAKE_GH_MODE=ok FAKE_GH_LOG="${ct_log}" FAKE_GH_JOBS="${ct_in}/jobs.json" "${HOOK_BASH}" "${ct}" report 2>&1 < /dev/null)" || true
case "${ct_rep}" in
  *"- repo (resolved by gh): example-owner/example-repo"*) pass "ci-timing: report shows the resolved repo" ;;
  *) fail "ci-timing: report shows the resolved repo" "${ct_rep}" ;;
esac
ct_nonget="$(grep -Eic "${ct_re}" "${ct_log}" || true)"
if [ "${ct_nonget}" = "0" ]; then
  pass "ci-timing: report uses GET only"
else
  fail "ci-timing: report uses GET only" "non-GET flags in ${ct_nonget} call(s)"
fi
# 値をくっつけた形（-XPOST 等）も検出する正規表現であること
ct_missed=0
for ct_s in 'repo view --json nameWithOwner' 'api graphql' 'api x -HX-HTTP-Method-Override:POST' 'api x --header=x-http-method-override:POST' 'api x -XPOST' 'api x --method=POST' 'api x -Fk=v' 'api x -fk=v' 'api x -H X-HTTP-Method-Override:POST'; do
  printf '%s\n' "${ct_s}" | grep -Eiq "${ct_re}" || ct_missed=$((ct_missed + 1))
done
if [ "${ct_missed}" = "0" ]; then
  pass "ci-timing: non-GET detector catches attached forms"
else
  fail "ci-timing: non-GET detector catches attached forms" "missed ${ct_missed}"
fi
# 相対パスの FILE も、スクリプト内の cd の後で同じファイルを指す
ct_rel="$(cd "${ct_in}" && "${HOOK_BASH}" "${ct}" summary one.tsv 2>&1 < /dev/null)" || true
case "${ct_rel}" in
  *"| w.yml / e | j | 1 / 0 | 7 | 7 | 7 | 0 | 0 | 0 |"*) pass "ci-timing: summary accepts a relative path" ;;
  *) fail "ci-timing: summary accepts a relative path" "${ct_rel}" ;;
esac
# 外から来たジョブ名の | [ ] < > を逃がす
# shellcheck disable=SC2016 # バッククォートはそのままの文字列で、シェルに展開させない
printf 'w.yml\te\t1\t1\tabc\ta|[b](u)<i>`c`\tsuccess\t100\t100\t107\n' > "${ct_in}/esc.tsv"
# shellcheck disable=SC2016 # 同上
ct_run "summary: escapes markdown characters in job names" 0 '| w.yml / e | a\|\[b\](u)\<i\>\`c\` | 1 / 0 |' summary "${ct_in}/esc.tsv"

finish "part 13"
