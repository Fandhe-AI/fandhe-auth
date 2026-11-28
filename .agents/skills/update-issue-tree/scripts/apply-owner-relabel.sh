#!/usr/bin/env bash
# apply-owner-relabel.sh — 承認された owner:human の除去を、前提の追記と対にして 1 件だけ適用する。
#
# 呼び出し元: skills/update-issue-tree/SKILL.md Step 7.5（owner ラベルの再判定の適用。承認後のみ）。
# 役割: 「## 前提 を追記 → 本文を読み直して確かめる → owner:human を除去」の順序を 1 プロセスで守る。
#       各コードフェンスは独立したシェルで実行され得るため、順序の保証を SKILL.md の文章に置けない
#       （reassign-sub-issue.sh と同じ理由）。前提が本文から依存として読めないままラベルを外すと、
#       人の結果が出る前に implement-issue-tree が後続を実装するため、除去は追記の確認の後にだけ行う。
#       判定（維持・除去・分割のどれか、前提が妥当か）は SKILL.md Step 2 の責務で、ここでは行わない。
#
# 使い方:
#   apply-owner-relabel.sh --issue <n> --prereq <n>[,<n>...] --dry-run
#   apply-owner-relabel.sh --issue <n> --prereq <n>[,<n>...] --expect-sha256 <64 桁の小文字 16 進>
#   --dry-run は書き込みをせず、提示用の sha256（[題名, 本文] の jq -c 出力の SHA-256）を出す。
#   適用では --expect-sha256 が必須で、承認時に提示した値と現在の [題名, 本文] が違えば何も書かずに止まる。
#   番号は正整数。前提は重複なし・対象自身を含めず・10 件まで。リポジトリは gh の {owner}/{repo} で解決する。
#
# 終了コード（stdout 最終行は result=<dry-run|applied|unchanged> issue=<n> add=<#A,#B|-> declared=<#C|-> [sha256=<hex>]）:
#   0 成功（dry-run / applied / unchanged。applied のときだけ本文とラベルが変わり得る）
#   1 引数の誤り（本文・ラベルとも変更なし）
#   2 前提不備（gh・jq・sha256 の手段・同梱フィルタが無い、取得失敗、PR、closed、owner:joint 付き、前提の state が不適、
#     ラベルが無いのに前提が足りない）。変更なし
#   3 本文へ安全に追記できない（閉じていないコードフェンス、## 前提 が複数、追記後に前提が依存として読めない）。変更なし
#   4 「## 人の作業の結果」節が本文に無い（足すのは人。スキルは書かない）。変更なし
#   5 承認時から題名・本文が変わった、または書き込み直前の再取得で変化を検知した。変更なし
#   6 本文の書き込みに失敗（本文の状態は不明。ラベルには触れていない）
#   7 読み直した本文が書いた本文と違う、または前提が依存として読めない（本文は変わっている。ラベルは残る）
#   8 ラベルの除去を確かめられない（ラベルの状態は不明。stderr に書く）
#   9 ラベルは外れたが前提が依存として読めない（要確認。人が owner:human を付け直す。自動では付け直さない）
#   exit 5〜9 の後に再実行するときは、--dry-run から承認をやり直す（古い sha256 を流用しない）。
#
# 保証しないこと（守れないことを守れるように見せない）:
#   - issue の更新 API に条件付き更新は無い。書き込み直前の再取得から書き込みまでの間に入った第三者の編集は上書きされ得る。
#   - 本文の読み直しより後に入った本文の編集（前提の削除）は、除去後の確認でしか検知できず、その後の変化は防げない。
#   - 前提がツリー内にあるか・祖先でないか・Phase の前後・循環（推移的なもの、phaseGate の待機を含む）は見ない（Step 2 の責務）。
#   - 承認から適用までに他の issue の本文が変わって生じる循環は辿らない。
#   - 実行中の implement-issue-tree のランは検知できない（SKILL.md の実行条件で防ぐ）。
#   - 依存の判定は同梱の declared-deps-filter.jq の版で決まる。実際に使う implement-issue-tree の版と違えば差が出得る。
# 題名と本文は非信頼データ。シェルへ展開せず、ファイルと jq だけで扱い、stdout / stderr へ出さない。

# set -e は使わない。終了コードを明示して扱う（失敗の握り潰しを避けるため、各コマンドの結果を個別に見る）。
die() { local code="${1}"; shift; printf 'エラー: %s\n' "$*" >&2; exit "${code}"; }

ISSUE=''
PREREQ_RAW=''
EXPECT=''
DRY=0
while (( $# > 0 )); do
  case "$1" in
    --issue) (( $# >= 2 )) || die 1 "--issue に値が無い"; ISSUE="$2"; shift 2 ;;
    --prereq) (( $# >= 2 )) || die 1 "--prereq に値が無い"; PREREQ_RAW="$2"; shift 2 ;;
    --expect-sha256) (( $# >= 2 )) || die 1 "--expect-sha256 に値が無い"; EXPECT="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    *) die 1 "不明な引数: $1" ;;
  esac
done
[[ "${ISSUE}" =~ ^[1-9][0-9]*$ ]] || die 1 "--issue は正整数にする"
[[ -n "${PREREQ_RAW}" ]] || die 1 "--prereq は必須"
if (( DRY == 1 )); then
  [[ -z "${EXPECT}" ]] || [[ "${EXPECT}" =~ ^[0-9a-f]{64}$ ]] || die 1 "--expect-sha256 は 64 桁の小文字 16 進にする"
else
  [[ "${EXPECT}" =~ ^[0-9a-f]{64}$ ]] || die 1 "適用には --expect-sha256（64 桁の小文字 16 進）が必須（先に --dry-run で値を得て承認を取る）"
fi
IFS=',' read -r -a PREREQS <<< "${PREREQ_RAW}"
(( ${#PREREQS[@]} >= 1 && ${#PREREQS[@]} <= 10 )) || die 1 "--prereq は 1〜10 件にする"
SEEN=','
for P in "${PREREQS[@]}"; do
  [[ "${P}" =~ ^[1-9][0-9]*$ ]] || die 1 "--prereq の要素は正整数にする"
  [[ "${P}" != "${ISSUE}" ]] || die 1 "--prereq に対象自身を含めない"
  case "${SEEN}" in *",${P},"*) die 1 "--prereq が重複している" ;; esac
  SEEN="${SEEN}${P},"
done
PREREQ_JSON=$(printf '%s\n' "${PREREQS[@]}" | jq -s -c 'map(tonumber)' 2>/dev/null) || PREREQ_JSON=''

# 同梱ファイルは実体のディレクトリ基準で読む（symlink を辿る）
SRC="${BASH_SOURCE[0]}"
while [[ -L "${SRC}" ]]; do
  LINK=$(readlink "${SRC}") || die 2 "スクリプトの symlink を辿れない"
  case "${LINK}" in /*) SRC="${LINK}" ;; *) SRC="${SRC%/*}/${LINK}" ;; esac
done
SCRIPT_DIR=$(cd "${SRC%/*}" 2>/dev/null && pwd) || die 2 "スクリプトのディレクトリを解決できない"

command -v gh >/dev/null 2>&1 || die 2 "gh が見つからない"
command -v jq >/dev/null 2>&1 || die 2 "jq が見つからない"
[[ -n "${PREREQ_JSON}" ]] || die 2 "前提の番号を jq で整形できない"
if command -v sha256sum >/dev/null 2>&1; then
  sha256_stdin() { sha256sum | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_stdin() { shasum -a 256 | awk '{print $1}'; }
else
  die 2 "sha256sum も shasum も見つからない"
fi
FILTER="${SCRIPT_DIR}/declared-deps-filter.jq"
[[ -f "${FILTER}" && -r "${FILTER}" && -s "${FILTER}" ]] || die 2 "同梱の依存の抽出フィルタ ${FILTER} が無い・読めない・空"
PROBE=$(printf '%s' '{"number":1,"body":""}' | jq -c -f "${FILTER}" 2>/dev/null) || die 2 "同梱の依存の抽出フィルタが壊れている"
printf '%s' "${PROBE}" | jq -e '.number == 1 and .deps == []' >/dev/null 2>&1 || die 2 "同梱の依存の抽出フィルタの出力の形が違う"

TMPD=$(mktemp -d 2>/dev/null) || die 2 "一時ディレクトリを作れない"
[[ -n "${TMPD}" && -d "${TMPD}" ]] || die 2 "一時ディレクトリを作れない"
trap 'rm -rf "${TMPD}"' EXIT

# issues/<n> の応答を検査してファイルへ書く（gh のエラーは stdout に出るので終了コードと型で判定する）
fetch_issue() {
  local n="$1" out="$2" resp
  resp=$(gh api "repos/{owner}/{repo}/issues/${n}" 2>/dev/null) || return 1
  printf '%s' "${resp}" | jq -e --argjson n "${n}" \
    'type == "object" and .number == $n and (.title | type) == "string"
    and ((.body | type) == "string" or .body == null) and (.labels | type) == "array" and (.state | type) == "string"' \
    >/dev/null 2>&1 || return 1
  printf '%s' "${resp}" > "${out}"
}
# 依存の番号配列（フィルタの deps）を本文ファイルから求める
deps_of() {
  local bodyfile="$1" n="$2" out
  out=$(jq -n -c --argjson n "${n}" --rawfile b "${bodyfile}" '{number: $n, body: $b}' 2>/dev/null \
    | jq -c -f "${FILTER}" 2>/dev/null | jq -c '.deps' 2>/dev/null) || return 1
  printf '%s' "${out}" | jq -e 'type == "array"' >/dev/null 2>&1 || return 1
  printf '%s' "${out}"
}
covers() { jq -n -e --argjson have "$1" --argjson need "$2" '($need - $have) == []' >/dev/null 2>&1; }
label_name() { jq -r '[.labels[]? | .name | select(type == "string") | select(ascii_downcase == "'"$1"'")] | first // empty' "$2"; }
tag_list() { # 番号の JSON 配列を #A,#B 形式にする（空は -）
  jq -r 'if length == 0 then "-" else map("#" + tostring) | join(",") end' <<< "$1"
}

# --- 2. 対象の取得と状態の確認 ---
CUR="${TMPD}/cur.json"
fetch_issue "${ISSUE}" "${CUR}" || die 2 "#${ISSUE} を取得できない、または応答の形が違う"
jq -e 'has("pull_request") | not' "${CUR}" >/dev/null 2>&1 || die 2 "#${ISSUE} は PR"
[[ "$(jq -r '.state' "${CUR}")" == "open" ]] || die 2 "#${ISSUE} は open でない"
[[ -z "$(label_name 'owner:joint' "${CUR}")" ]] || die 2 "#${ISSUE} に owner:joint が付いている（分割の提案だけで、ここでは扱わない）"
HUMAN_LABEL=$(label_name 'owner:human' "${CUR}")
BODY="${TMPD}/body"
jq -j '.body // ""' "${CUR}" > "${BODY}" || die 2 "本文を取り出せない"
DEPS=$(deps_of "${BODY}" "${ISSUE}") || die 2 "依存を抽出できない"
ADD_JSON=$(jq -n -c --argjson p "${PREREQ_JSON}" --argjson d "${DEPS}" '$p - $d')
DECL_JSON=$(jq -n -c --argjson p "${PREREQ_JSON}" --argjson d "${DEPS}" '$p - ($p - $d)')
ADD_TAG=$(tag_list "${ADD_JSON}")
DECL_TAG=$(tag_list "${DECL_JSON}")
if [[ -z "${HUMAN_LABEL}" ]]; then
  if [[ "${ADD_JSON}" == "[]" ]]; then
    printf 'result=unchanged issue=%s add=- declared=%s\n' "${ISSUE}" "${DECL_TAG}"
    exit 0
  fi
  die 2 "#${ISSUE} に owner:human が無いのに前提が足りない（要確認。何も書かない）"
fi

# --- 3. 本文の安全性 ---
FACTS=$(awk '
  BEGIN { fence = ""; sect = 0; pre = 0; res = 0; ins = 0; last = 0 }
  {
    l = $0; sub(/\r$/, "", l)
    if (fence != "") {
      t = l; gsub(/^[ \t]+|[ \t]+$/, "", t)
      if (t ~ /^(```+|~~~+)$/ && substr(t, 1, 1) == substr(fence, 1, 1) && length(t) >= length(fence)) fence = ""
      if (sect && l !~ /^[ \t]*$/) last = NR
      next
    }
    if (l ~ /^ ? ? ?(```+|~~~+)/) {
      t = l; sub(/^ +/, "", t); match(t, /^(`+|~+)/); fence = substr(t, 1, RLENGTH)
      if (sect) last = NR
      next
    }
    if (l ~ /^ ? ? ?#+([ \t]|$)/) {
      if (sect) { ins = last; sect = 0 }
      if (l ~ /^ ? ? ?##[ \t]+前提[ \t]*$/) { pre++; sect = 1; last = NR }
      if (l ~ /^ ? ? ?##[ \t]+人の作業の結果[ \t]*$/) res++
      next
    }
    if (sect && l !~ /^[ \t]*$/) last = NR
  }
  END { if (sect) ins = last; printf "fence_open=%d pre=%d res=%d ins=%d\n", (fence != "") ? 1 : 0, pre, res, ins }
' "${BODY}") || die 3 "本文を解析できない"
eval_fact() { printf '%s' "${FACTS}" | tr ' ' '\n' | sed -n "s/^$1=//p"; }
F_FENCE=$(eval_fact fence_open); F_PRE=$(eval_fact pre); F_RES=$(eval_fact res); F_INS=$(eval_fact ins)
[[ "${F_FENCE}" == "0" ]] || die 3 "本文に閉じていないコードフェンスがある"
[[ "${F_PRE}" == "0" || "${F_PRE}" == "1" ]] || die 3 "本文に ## 前提 が複数ある"
[[ "${F_RES}" -ge 1 ]] 2>/dev/null || die 4 "本文に「## 人の作業の結果」節が無い（人が足してから、--dry-run からやり直す）"

# --- 4. 承認との一致 ---
SHA=$(jq -c '[.title, (.body // "")]' "${CUR}" | sha256_stdin)
[[ -n "${SHA}" ]] || die 2 "sha256 を計算できない"
if [[ -n "${EXPECT}" && "${SHA}" != "${EXPECT}" ]]; then
  die 5 "承認時から #${ISSUE} の題名か本文が変わった。何も書かない。--dry-run からやり直す"
fi

# --- 5. 前提の確認 ---
CUR_REPO=$(jq -r '.repository_url // ""' "${CUR}" | tr 'A-Z' 'a-z')
for P in "${PREREQS[@]}"; do
  PF="${TMPD}/p${P}.json"
  fetch_issue "${P}" "${PF}" || die 2 "前提 #${P} を取得できない、または応答の形が違う"
  jq -e 'has("pull_request") | not' "${PF}" >/dev/null 2>&1 || die 2 "前提 #${P} は PR"
  jq -e '.state == "open" or (.state == "closed" and .state_reason == "completed")' "${PF}" >/dev/null 2>&1 \
    || die 2 "前提 #${P} は open でも completed で closed でもない"
  P_REPO=$(jq -r '.repository_url // ""' "${PF}" | tr 'A-Z' 'a-z')
  [[ "${P_REPO}" == "${CUR_REPO}" ]] || die 2 "前提 #${P} が対象と別のリポジトリの issue に見える"
done

# --- 6. 新しい本文の組み立て ---
NEWBODY="${TMPD}/newbody"
ADD_LINES="${TMPD}/addlines"
jq -r '.[] | "- #" + tostring' <<< "${ADD_JSON}" > "${ADD_LINES}" || die 3 "追記行を作れない"
ADD_COUNT=$(jq 'length' <<< "${ADD_JSON}")
if (( ADD_COUNT > 0 )); then
  if [[ "${F_PRE}" == "1" ]]; then
    awk -v at="${F_INS}" -v af="${ADD_LINES}" '
      { print }
      NR == at { while ((getline x < af) > 0) print x; close(af) }
    ' "${BODY}" > "${NEWBODY}" || die 3 "本文を組み立てられない"
    # 差し込んだ行を除くと元の本文に戻ることを確かめる（既存の行を 1 行も変えない）
    STRIPPED=$(awk -v at="${F_INS}" -v k="${ADD_COUNT}" 'NR <= at || NR > at + k { print }' "${NEWBODY}")
    [[ "${STRIPPED}" == "$(cat "${BODY}")" ]] || die 3 "追記が既存の行を変える（中止）"
  else
    cp "${BODY}" "${NEWBODY}" || die 3 "本文を複写できない"
    if [[ -s "${BODY}" ]]; then
      [[ -z "$(tail -c 1 "${BODY}")" ]] || printf '\n' >> "${NEWBODY}"
      printf '\n' >> "${NEWBODY}"
    fi
    { printf '## 前提\n'; cat "${ADD_LINES}"; } >> "${NEWBODY}"
    ORIG_SIZE=$(wc -c < "${BODY}" | tr -d ' ')
    head -c "${ORIG_SIZE}" "${NEWBODY}" | cmp -s - "${BODY}" || die 3 "追記が既存の本文を変える（中止）"
  fi
  NEW_DEPS=$(deps_of "${NEWBODY}" "${ISSUE}") || die 3 "追記後の依存を抽出できない"
  covers "${NEW_DEPS}" "${PREREQ_JSON}" || die 3 "追記後も前提が依存として読めない（閉じていない HTML コメント・コードスパン等）"
  covers "${NEW_DEPS}" "${DEPS}" || die 3 "追記により既存の依存が読めなくなる"
fi
if (( DRY == 1 )); then
  printf 'result=dry-run issue=%s add=%s declared=%s sha256=%s\n' "${ISSUE}" "${ADD_TAG}" "${DECL_TAG}" "${SHA}"
  exit 0
fi

# --- 7. 書き込み直前の再取得 ---
KEYQ='[.title, (.body // ""), ([.labels[]? | .name | select(type == "string") | ascii_downcase] | sort), .state]'
RE="${TMPD}/re.json"
fetch_issue "${ISSUE}" "${RE}" || die 5 "書き込み直前に #${ISSUE} を再取得できない（何も書かない）"
[[ "$(jq -c "${KEYQ}" "${CUR}")" == "$(jq -c "${KEYQ}" "${RE}")" ]] || die 5 "#${ISSUE} が並行して編集された（何も書かない）"

# --- 8. 本文の追記（ラベルより先） ---
if (( ADD_COUNT > 0 )); then
  gh issue edit "${ISSUE}" --body-file "${NEWBODY}" >/dev/null 2>&1 || die 6 "#${ISSUE} の本文の書き込みに失敗（本文の状態は不明。ラベルは残る）"
  # --- 9. 読み直し ---
  AFTER="${TMPD}/after.json"
  fetch_issue "${ISSUE}" "${AFTER}" || die 7 "#${ISSUE} の本文を読み直せない（ラベルは残る）"
  AFTER_BODY="${TMPD}/afterbody"
  jq -j '.body // ""' "${AFTER}" > "${AFTER_BODY}" || die 7 "読み直した本文を取り出せない（ラベルは残る）"
  [[ "$(tr -d '\r' < "${AFTER_BODY}")" == "$(tr -d '\r' < "${NEWBODY}")" ]] || die 7 "読み直した本文が書いた本文と違う（ラベルは残る）"
  AFTER_DEPS=$(deps_of "${AFTER_BODY}" "${ISSUE}") || die 7 "読み直した本文の依存を抽出できない（ラベルは残る）"
  covers "${AFTER_DEPS}" "${PREREQ_JSON}" || die 7 "読み直した本文で前提が依存として読めない（ラベルは残る）"
fi

# --- 10. ラベルの除去（追記の確認の後） ---
gh issue edit "${ISSUE}" --remove-label "${HUMAN_LABEL}" >/dev/null 2>&1 || true
FIN="${TMPD}/fin.json"
if ! fetch_issue "${ISSUE}" "${FIN}"; then
  die 8 "#${ISSUE} のラベルの除去を確かめられない（取得に失敗。ラベルの状態は不明）"
fi
if [[ -n "$(label_name 'owner:human' "${FIN}")" ]]; then
  die 8 "#${ISSUE} の owner:human が外れていない（本文は変わっている）"
fi
FIN_BODY="${TMPD}/finbody"
jq -j '.body // ""' "${FIN}" > "${FIN_BODY}" || die 9 "最終の本文を取り出せない（owner:human は外れた。要確認）"
FIN_DEPS=$(deps_of "${FIN_BODY}" "${ISSUE}") || die 9 "最終の本文の依存を抽出できない（owner:human は外れた。要確認）"
covers "${FIN_DEPS}" "${PREREQ_JSON}" || die 9 "owner:human は外れたが前提が依存として読めない（人が owner:human を付け直す）"
printf 'result=applied issue=%s add=%s declared=%s\n' "${ISSUE}" "${ADD_TAG}" "${DECL_TAG}"
exit 0
