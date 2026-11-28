#!/usr/bin/env bash
# fix-overlap-lib.sh — observe-pr.sh の fix-overlap モードの判定だけを持つ純関数（source 専用）
#
# 呼び出し元: observe-pr.sh（Issue #910）。直前の fix が追加した行に、fix より後のコミットで新しいレビュー指摘（スレッド）が
#             付いたかを数える。ホストは「重なりが連続 2 回」で次の fix を起動せず blocked で止める。判定をシェル側（jq）に置くのは、
#             生成物（implement-issue-tree.js）のバイト予算を使わないためと、テストが 1 回の bash 起動で表の全ケースを検査できる
#             ようにするため（lib/policy-paths-lib.sh と同じ）。
# 契約: exit せず、git・gh・ネットワークを呼ばない。結果は標準出力と return 値だけ。set -e / set -u / trap / exit は置かない。二重 source は無害。
# 入力: 標準入力に 1 つの JSON
#         {"threads":[{"id","path","side","start","line","commit"}…],   未解決スレッドの正規化（side は RIGHT / LEFT、start・line は整数か null、
#                                                                      commit は最初のコメントの originalCommit.oid か null）
#          "commits":["<sha>"…],                                         fix の範囲（compare の from...head）のコミット
#          "files":[{"filename","patch"}…],                              同じ範囲の変更ファイル（patch が無いものは null）
#          "capped":0|1}                                                 compare の応答が上限で切り詰められた（スクリプトが判定して渡す）
# 出力: {"newThreads":N,"overlap":N,"threadIds":["id"…],"truncated":N}
#   newThreads  右側（RIGHT）で line が非 null、かつ最初のコメントのコミットが commits に含まれる未解決スレッドの件数
#   overlap     そのうち、同じパスの『追加された行（patch の新側の行番号）』とスレッドの範囲 [start // line, line] が 1 行でも交わる件数
#   threadIds   重なったスレッドの ID（GitHub が発行する [A-Za-z0-9_-]{1,100} だけ・最大 20 件。書き手が選べない値）
#   truncated   数えられなかった件数（patch の無いファイルに付いた新しいスレッド + capped）。重なり 0 側に倒れる = 従来どおり止めない
# 重要度（P0〜P2）は本文にあり非信頼データなので解析しない。重要度を問わず数える（過検出は止める方向にしか働かない）。
fix_overlap_count() {
  jq -c '
    # patch の追加行（新側の行番号）。hunk 見出し @@ -a[,b] +c[,d] @@ で新側の番号を c に置き、+ 行を数え、- 行と改行なしマーカーは数えず、文脈行は進める
    def added_lines:
      (. // "" | split("\n"))
      | reduce .[] as $l ({n: null, a: []};
          if ($l | startswith("@@")) then
            (($l | capture("^@@ -[0-9]+(,[0-9]+)? \\+(?<c>[0-9]+)(,[0-9]+)? @@") // null) as $m
              | if $m == null then .n = null else .n = ($m.c | tonumber) end)
          elif .n == null then .
          elif ($l | startswith("+")) then .a += [.n] | .n += 1
          elif ($l | startswith("-")) or ($l | startswith("\\")) then .
          else .n += 1 end)
      | .a;
    (.commits // []) as $cs
    | (.files // []) as $fs
    | ([$fs[] | select(.filename | type == "string") | {key: .filename, value: {has: ((.patch | type) == "string"), added: (.patch | added_lines)}}] | from_entries) as $fm
    | [(.threads // [])[]
        | select(.side == "RIGHT" and (.line | type) == "number" and (.commit | type) == "string" and (. as $t | $cs | index($t.commit) != null))] as $new
    | [$new[] | . as $t
        | ($fm[$t.path] // null) as $f
        | select($f != null and $f.has)
        | (if (.start | type) == "number" and .start <= .line then .start else .line end) as $s
        | select(any($f.added[]; . >= $s and . <= $t.line))
        | $t.id] as $ids
    | {newThreads: ($new | length),
        overlap: ($ids | length),
        threadIds: ([$ids[] | select(type == "string" and test("^[A-Za-z0-9_-]{1,100}$"))] | .[0:20]),
        truncated: (([$new[] | . as $t | ($fm[$t.path] // null) as $f | select($f != null and ($f.has | not))] | length) + (.capped // 0))}'
}
