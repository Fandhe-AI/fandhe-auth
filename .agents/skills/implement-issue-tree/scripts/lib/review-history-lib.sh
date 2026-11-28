# review-history-lib.sh — observe-pr.sh --mode review-history が PR の全レビュースレッドを整形する純関数（source 専用）
#
# 役割: GraphQL で取ったスレッドのページ（新しい側から順）を、箇所（path）ごとの巡の数つきの履歴 JSON 1 行へ整える。
#       observe-pr.sh は取得とページングと封筒を担い、この lib は「形」と「切り詰め」だけを持つ（gh を呼ばない・状態を持たない）。
#       出力の形は Fandhe-AI/actions の ai-review/scripts/collect-review-history.mjs（version / files[] / threads[]、snake_case のキー）に
#       そろえる。fix エージェントと、手元で PR の履歴を調べる人の両方が読む。
# 呼び出し元: scripts/observe-pr.sh の observe_review_history。単体テストは dev/implement-issue-tree/tests/review-history-unit.test.mjs。
# 保証しないこと: 本文・ログイン名・is_resolved・resolved_by は PR の参加者が作れる外部由来のデータで、信頼できない。
#       resolve 済みは決着を意味しない（レビュー側は自分の旧指摘を毎巡 resolve する）。文字数は jq の length（コードポイント）で数えるため、
#       UTF-16 で数える JS 実装とは境界の数値が少しずれ得る。
# 契約: set -e・trap・exit を置かない（呼び出し側のシェルを巻き込まない）。シェル変数は jq へ --arg / --argjson で渡し、
#       フィルタ文字列へ連結しない。bash 3.2 と jq 1.5 以上で動く。

# 先頭コメントの抜粋の上限・返信の抜粋の上限・files[] の件数
REVIEW_HISTORY_FIRST_CAP=1500
REVIEW_HISTORY_REPLY_CAP=500
REVIEW_HISTORY_FILES_MAX=100

# review_history_build COMPLETE MAX_THREADS MAX_CHARS REPO PR TOTAL   （標準入力: PAGES）
#   PAGES        標準入力の JSON。reviewThreads オブジェクト（.nodes を持つ）の配列。先頭が最も新しいページ。各ページの nodes は古い → 新しいの順
#                （サイズが大きくなり得るため引数ではなく標準入力で渡す。OS の引数長の上限を避ける）
#   COMPLETE     1 = 全ページを取れた / 0 = 上限か壁時計で打ち切った（truncated を真にする）
#   MAX_THREADS  出すスレッド数の上限。MAX_CHARS 整形後 JSON 全体の文字数の上限（超えたら threads を古い側から落とし〔最新 1 件は必ず残す〕、なお超えれば files を重要度の低い末尾から落とす）
#   REPO         空なら null。PR は PR 番号。TOTAL は GraphQL の totalCount
# 標準出力に履歴 JSON を 1 行で出す。引数・入力が不正なら jq が非 0 で終わる（呼び出し側は取得失敗として扱う）。
review_history_build() {
  jq -c \
    --argjson complete "$1" --argjson maxt "$2" --argjson maxc "$3" \
    --arg repo "$4" --argjson pr "$5" --argjson total "$6" \
    --argjson fcap "${REVIEW_HISTORY_FIRST_CAP}" --argjson rcap "${REVIEW_HISTORY_REPLY_CAP}" --argjson filesmax "${REVIEW_HISTORY_FILES_MAX}" '
    # 末尾の ai-review マーカー（本文の途中のものは数えない）。\z で文字列の末尾に固定する（$ は行末にも一致するため使わない）
    def trimend: sub("\\s+\\z"; "");
    def marker_re: "<!-- (?:ai-review-finding reviewer=(?<r>[a-z0-9-]+)|(?<c>codex-review-finding)) -->\\z";
    def marker($body):
      ($body | trimend) as $t
      | if ($t | test(marker_re)) then
          ($t | capture(marker_re)) as $m
          | { is: true, reviewer: (if $m.c != null then "codex" else $m.r end) }
        else { is: false, reviewer: null } end;
    def bot($login): ($login == "github-actions" or $login == "github-actions[bot]");
    def strip($body): ($body | trimend | sub(" ?<!-- (?:ai-review-finding reviewer=[a-z0-9-]+|codex-review-finding) -->\\z"; "") | trimend);
    def excerpt($s; $cap):
      if ($s | length) > $cap then ($s[0:$cap]) + "…（" + (($s | length) - $cap | tostring) + " 文字省略）" else $s end;
    def comment_view($c; $cap):
      ($c.author.login // "") as $a
      | ($c.body // "") as $b
      | marker($b) as $mk
      | { author: $a, is_ai_finding: (bot($a) and $mk.is), comment: excerpt(strip($b); $cap) };
    def shape:
      (.firstComment.nodes[0] // {}) as $f
      | ($f.author.login // "") as $fa
      | ($f.body // "") as $fb
      | marker($fb) as $mk
      | (bot($fa) and $mk.is) as $ai
      | ((.comments.totalCount // 0) as $tc | (if $tc > 1 then $tc - 1 else 0 end)) as $rc
      | ((.lastComments.nodes // []) as $ln
          | (if $rc == 0 then [] else $ln[(($ln | length) - ([$rc, 3, ($ln | length)] | min)):] end)) as $rn
      | {
          id: .id,
          path: (.path // null),
          line: (.line // .originalLine // null),
          is_resolved: (.isResolved == true),
          resolved_by: (.resolvedBy.login // null),
          is_outdated: (.isOutdated == true),
          author: $fa,
          is_ai_finding: $ai,
          ai_reviewer: (if $ai then $mk.reviewer else null end),
          original_commit: ($f.originalCommit.oid // null),
          comment: excerpt(strip($fb); $fcap),
          reply_count: $rc,
          replies: [ $rn[] | comment_view(.; $rcap) ]
        };
    def summarize:
      group_by(.path)
      | map({
          path: .[0].path,
          review_rounds: ([.[] | .original_commit | select(. != null)] | unique | length),
          threads: length,
          resolved: ([.[] | select(.is_resolved)] | length),
          ai_findings: ([.[] | select(.is_ai_finding)] | length)
        })
      | sort_by([-.review_rounds, -.threads, (.path // "")]);
    ([ reverse | .[] | .nodes[] ] | map(shape)) as $all
    | ($all | summarize) as $files_all
    | ($all | length) as $fetched
    | (if $fetched > $maxt then $all[($fetched - $maxt):] else $all end) as $cut
    | def mkout($ts; $fs):
        { version: 1,
          repo: (if $repo == "" then null else $repo end),
          pr: $pr,
          total_threads: $total,
          fetched_threads: $fetched,
          included_threads: ($ts | length),
          truncated: (($complete == 0) or (($ts | length) < $total) or (($fs | length) < ($files_all | length))),
          files: $fs,
          files_truncated: (($fs | length) < ($files_all | length)),
          threads: $ts };
    ($files_all[0:$filesmax]) as $fcut
    | ($cut | until(length <= 1 or (mkout(.; $fcut) | tojson | length) <= $maxc; .[1:])) as $kept
    | ($fcut | until(length == 0 or (mkout($kept; .) | tojson | length) <= $maxc; .[0:length - 1])) as $fkept
    | mkout($kept; $fkept)
  '
}
