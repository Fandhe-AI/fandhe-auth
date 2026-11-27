#!/usr/bin/env bash
# tree-lib.sh — イシューツリーの実ツリー取得と表セル無害化の共通ヘルパー（source 専用）
#
# 呼び出し元: create-root-body.sh（Step 6 新規作成経路）と merge-root-body.sh（Step 6 --root 経路）、
#           human-blockers.sh（Step 7 の人手 issue の数え方。list_subs と CELL だけを使う）。
# 写し: update-issue-tree の scripts/ に同じ内容の写しがある（そちらの human-blockers.sh が読む）。このファイルを変えたら、
#       human-blockers.sh の冒頭コメントの「写し」の cp で写し直す（一致は上流の dev/update-issue-tree/tests/human-queue.test.mjs が確かめる）。
# 役割: sub_issues API のページング取得・open 子孫の再帰集計・issue タイトルの表セル無害化・
#       プレースホルダー検査の正規表現を 1 箇所に定義し、2 つのスクリプトで本文生成ロジックを
#       重複させない（Issue #555・#556）。
# 前提: gh（認証済み）と jq が PATH にあること。リポジトリは gh の {owner}/{repo} 解決に従う（list_subs にホストと
#       owner/repo を渡した場合だけ、そのリポジトリを明示して読む）。
# 契約: 失敗は非ゼロ return。source 元のシェル状態を壊さないため set -e / set -u / trap / exit
#       は置かない（エラー処理は呼び出し側が行う）。二重 source は無害（ガードで即 return）。
#
# このファイルは単体実行しない。

# CELL / TREE_PLACEHOLDER_RE は source 元が参照する（このファイル内では未使用）
# shellcheck disable=SC2034
[[ -n "${TREE_LIB_LOADED:-}" ]] && return 0
TREE_LIB_LOADED=1

# 指定 issue の sub-issues 全件（JSON 配列）をページングで取得する。失敗時は非ゼロ。
# 使い方: list_subs <n> [<host> <owner/repo>]。ホストと owner/repo（呼び出し側で形式を検証した値）を渡すと、
# gh の {owner}/{repo} の解決に頼らずそのリポジトリを明示して読み、repository_url の owner/repo が渡した owner/repo と
# 一致する（大小を区別しない）要素だけを返す（sub-issue は別リポジトリの issue でもよく、番号はリポジトリごとなので、
# 別リポジトリの要素を同じ番号の対象リポジトリの issue と取り違えない）。除くのは別リポジトリと確かめられた要素だけ、
# つまり repository_url が API の URL の形式（https?://<host>[:<port>]/repos/<owner>/<repo>。GHES は /api/v3/repos/…。
# 大小を区別しない。文字列全体で照合し、改行や余分なパスを含むものは形式違い）で owner/repo が違う要素だけである。
# repository_url が無い・文字列でない・形式が違う要素が 1 つでもあれば取得の失敗として非ゼロを返す（識別できない要素を
# 落とすと、対象リポジトリの子とその下を数え漏らしたまま成功になる）。
# 渡さなければ従来どおり {owner}/{repo} で読み、全要素を返す。ページの終わりは絞り込む前の件数で判定する。
# どちらの経路でも、1 ページの応答は単一の JSON 配列でなければならない（配列の後ろに別の値が続く・空・null・配列でない応答は
# 取得の失敗として非ゼロ）。確かめずに件数を取ると jq 'length' が複数行や 0 行を返して整数の比較が失敗し、ページングが
# 終わらないか、最初の値だけの不完全な集計で成功する
list_subs() {
  local n="$1" host="${2:-}" slug="${3:-}" page=1 res count all='[]' more=1
  # 呼び出し元が export した関数で差し替えられる組み込み（true・[・break）は、ループの条件・判定・脱出に使わない。
  # [[ ]]・for・while は構文なので関数で上書きできない。ページの終わりは break ではなく more で抜ける
  while [[ "${more}" -eq 1 ]]; do
    if [[ -n "${host}" ]]; then
      res=$(gh api --hostname "${host}" "repos/${slug}/issues/${n}/sub_issues?per_page=100&page=${page}") || return 1
    else
      res=$(gh api "repos/{owner}/{repo}/issues/${n}/sub_issues?per_page=100&page=${page}") || return 1
    fi
    # 応答には本文が含まれ、引数に載せると ARG_MAX / MAX_ARG_STRLEN を超え得るため、以降も printf（組み込み）経由の
    # stdin で jq へ渡す
    count=$(printf '%s' "${res}" | jq -s 'if length == 1 and (.[0] | type) == "array" then .[0] | length
      else error("sub_issues の応答が単一の JSON 配列でない") end') || return 1
    case "${count}" in '' | *[!0-9]*) return 1 ;; esac
    if [[ -n "${host}" ]]; then
      # 形式は固定の正規表現で確かめ、owner/repo は文字列で比べる（slug を正規表現へ埋め込まない。リポジトリ名は . を含み得る）。
      # \A と \z は文字列全体の端（^ と $ は行の端にも一致するので、改行を挟んだ値を通してしまう）
      all=$({ printf '%s' "${all}"; printf '%s' "${res}"; } | jq -s --arg slug "${slug}" '.[0] + (.[1]
        | map(if (.repository_url | type) == "string"
                and (.repository_url | test("\\Ahttps?://[A-Za-z0-9.-]+(:[0-9]{1,5})?/(api/v3/)?repos/[A-Za-z0-9_-]+/[A-Za-z0-9._-]+\\z"; "i"))
              then . else error("repository_url が無い・文字列でない・形式が違う要素") end
          | select(.repository_url | ascii_downcase | split("/") | .[-2:] | join("/") == ($slug | ascii_downcase))))') || return 1
    else
      all=$({ printf '%s' "${all}"; printf '%s' "${res}"; } | jq -s '.[0] + .[1]') || return 1
    fi
    if [[ "${count}" -lt 100 ]]; then
      more=0
    else
      page=$((page + 1))
    fi
  done
  printf '%s' "${all}"
}

# 指定 issue の全子孫のうち open な issue 件数を再帰で数えて stdout へ出す。Step 5 は sub-issue への
# 追加分解を許すため、直下だけでなく 3 階層目より深い open issue も総件数に含める。
# 失敗時は非ゼロ。depth は循環・異常な深さへの安全弁（超過は失敗扱い）
count_open_desc() {
  local n="$1" depth="${2:-0}" subs open c cnum
  [[ "${depth}" =~ ^[0-9]+$ ]] && [[ "${depth}" -le 20 ]] || return 1
  subs=$(list_subs "${n}") || return 1
  open=$(printf '%s' "${subs}" | jq '[.[] | select(.state == "open")] | length') || return 1
  for cnum in $(printf '%s' "${subs}" | jq -r '.[].number'); do
    c=$(count_open_desc "${cnum}" $((depth + 1))) || return 1
    open=$((open + c))
  done
  printf '%s' "${open}"
}

# issue タイトルは非信頼データ。シェル展開には載せず、本文へ書く前に HTML 文字参照へ変換する。
# 改行（CR/LF）は空白 1 つへ、& < > ` \ | は文字参照へ置換する。& を最初に置換するのは、後続の置換が出す
# 文字参照の & を二重変換しないため。生成行にバッククォート・<!--・-->・バックスラッシュ・HTML タグ・列区切りの | が
# 入らないので、merge-root-body.sh の判定（曖昧な記号での中止・<details> の数え上げ）を乱さず、
# GitHub 上の表示は元のタイトルのまま（文字参照として描画される）
CELL='gsub("[\r\n]+"; " ") | gsub("&"; "&amp;") | gsub("<"; "&lt;") | gsub(">"; "&gt;") | gsub("`"; "&#96;") | gsub("\\\\"; "&#92;") | gsub("\\|"; "&#124;")'

# ルート本文に残ってはならないプレースホルダー（grep -E 用）。Step 3 の雛形・手書き例の取り残しを検出する
TREE_PLACEHOLDER_RE='<phase[0-9]*_number>|\(作成後に更新\)|#N([^0-9A-Za-z]|$)|^\|.*\|[[:space:]]*N[[:space:]]*(\||$)'
