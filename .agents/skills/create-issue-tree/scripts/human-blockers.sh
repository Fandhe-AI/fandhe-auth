#!/usr/bin/env bash
# human-blockers.sh — ツリーの open な人手 issue ごとに、依存する後続 issue の数と未完了の前提を数える（読み取り専用）
#
# 呼び出し元: skills/create-issue-tree/SKILL.md Step 7（完了レポートの人の作業の表）。
# 役割: 数え方は以前、SKILL.md の文章の手順としてエージェントが gh と jq を組み立てて数えていた。依存の読み方が
#       implement-issue-tree とずれる余地と、取得の失敗を「依存なし」と取り違える余地があったため、スクリプトへ移した。
#
# 使い方: human-blockers.sh --root <n> --repo <host>/<owner>/<repo>
#   --root  ルート issue 番号（正整数）
#   --repo  対象リポジトリ。Step 5 の状態ファイルのヘッダの repo と同じ形（host は名前と任意のポート）
#
# 数え方:
#   0. ルート: sub_issues を辿る前に issues/<root> を 1 回読み、下の「個別の issue の応答の照合」と同じ REPO_DEF で
#      repository_url → 番号 → PR でないの順に照合する。確かめられなければ（取得の失敗を含む）ルートの番号を示して exit 3。
#      リポジトリの改名・ルートの移管で sub_issues の読み取りがリダイレクトされると、全要素の repository_url が別リポジトリに
#      なって 1 の list_subs が全部落とし、open=0 humans=0 で成功してしまう（人手 issue は 0 件と誤って報告する）ため。
#   1. ツリー: ルートの sub_issues を全ページ・再帰で読み（closed の issue の下も辿る）、repository_url が対象リポジトリの
#      要素だけを集める（tree-lib.sh の list_subs。別リポジトリの sub-issue は番号が同じでも別物なので、数えず、その番号で
#      子も読まない。その配下に対象リポジトリの issue があっても辿らず、数えない）。repository_url が無い・文字列でない・API の URL の形式でない要素は別リポジトリと確かめられないので、
#      落とさずに exit 3。要素の番号・state・題名・ラベル（配列で、各要素がオブジェクトで name が文字列）の型も確かめ、
#      欠落・null・false を空の値で埋めて数えずに exit 3（空の配列のラベルは正当な値として受け入れる）。ルート自身は含めない。
#   2. 依存: open な issue ごとに issues/<n> を読み、同梱の declared-deps-filter.jq（下の「依存の抽出フィルタ」）を
#      gh api --jq で通した deps を使う。フィルタは本文の欠落・false を空の本文として読むので、フィルタへ渡す前に同じ --jq の
#      中で本文の形を確かめる（body が有り、文字列か null。GitHub は空の本文を null で返す）。それ以外は exit 3。
#      同じ応答の repository_url と pull_request の有無も --jq の出力に足し、応答が対象リポジトリの issue（repository_url が
#      API の URL の形式で owner/repo が --repo と一致する。大小を区別しない・GHES の /api/v3 も通る・host は比べない）で、
#      番号が依頼した番号と一致し、PR でないことを確かめる。確かめられなければ exit 3（下の「個別の issue の応答の照合」）。
#      issues/<n> の経路では本文を gh の --jq の中だけで読み、シェルの変数にも出力にも入れない（変数に入るのは
#      {number, deps, sig, repository_url, pr}）。1 の sub_issues の応答は本文を含んだまま変数（SUBS）に入るが、NODES へ移すのは番号・state・
#      題名・ラベルだけで、本文は出力しない。
#   3. 辺: deps のうち、open でツリー内の issue（自分以外）だけを辺にする。closed・ツリー外の issue は辺にせず、経由もしない。
#   4. 人手 issue: open で owner:human か owner:joint のラベル（大小を区別しない）を持つ issue。
#      advances        = その issue へ直接・間接に依存する open issue の数（重複なし・自分を除く。3 の辺だけで数える）
#      waiting_on      = その issue 自身の deps のうち open でツリー内のもの（自分を除く。昇順）
#      waiting_outside = その issue 自身の deps のうちツリー外（closed を含むツリーのどの issue でもない）で open のもの（昇順）。
#                        state は issues/<n>（--repo と同じリポジトリ。本文の #N は同じリポジトリを指す）を番号ごとに 1 回読んで
#                        確かめる。取得に失敗したら前提なしとせず exit 3。応答の repository_url と番号も 2 と同じく照合し、
#                        対象リポジトリの issue と確かめられなければ exit 3（別リポジトリへ移管された issue は、GitHub が移管先へ
#                        リダイレクトして移管先の応答を返す。移管先の番号が偶然同じでも、対象リポジトリの前提として数えない）。別リポジトリの参照（owner/repo#N）は依存の抽出が
#                        拾わないので対象外。ツリー外の issue は辺にしないので advances には効かない。
#                        implement-issue-tree は state を読めないツリー外の前提を open として待つ側に倒して実行を続けるが、
#                        このスクリプトは止める（表の前提の列を、確かめていない値で埋めないため）。削除された issue・
#                        番号の書き違いで読めないときに直す先は、その前提を書いた人手 issue の本文の ## 前提
#                        前提の番号が PR（issues/<n> が pull_request を返す）のときも exit 3 で止め、書いた人手 issue と PR の
#                        番号を stderr に示す（PR を issue の完了待ちとして表に出さない）
#   表の並びは実行時の着手の可否とは条件が違う。open なルート自身・ツリー内の自分の祖先（Phase 親など）・ルートの祖先を前提に書いた
#   人手 issue は表では完了待ちに並ぶ（自分の祖先は waiting_on、ルート自身とルートの祖先は waiting_outside に出る）が、implement-issue-tree は
#   その前提を待たない。
#   このスクリプトは祖先かどうかを見分けず、ツリー内の open な前提を waiting_on へ、それ以外の open な前提を waiting_outside へ出す。
#   advances は依存で到達できる後続の数で、close した直後に着手できるようになる数ではない（人手 issue → A → B なら、close
#   した直後も B は A の完了を待つ。A に別の未完了の前提があれば、着手できるようになる issue が 0 件でも advances は 2）。
#   宣言された依存だけを数えるので、implement-issue-tree の実行時（エージェントによる依存の抽出や phaseGate）には後続が
#   もっと多いことがある。着手できる件数は implement-issue-tree の dryRun の見積りで確かめる。
#   個別の issue の応答の照合（0・2・2b で共有する REPO_DEF）: issues/<n> は要求した番号の応答とは限らない（移管された issue は
#   リダイレクトで別リポジトリの issue を返す）ので、repository_url → 番号の順に照合する（移管では番号も変わるのが普通なので、
#   先にリポジトリを見て理由を「別リポジトリ」と示す）。repository_url の形式の検査と owner/repo の比べ方は tree-lib.sh の
#   list_subs と同じ（写しなので、上流の human-blockers.test.mjs が両者の一致を確かめる）。host は比べない（gh api --hostname で読む先を決めているうえ、API の host は github.com では
#   api.github.com、GHES では <host>/api/v3 のように --repo の host と字面が違う）。照合に失敗したら、どの経路でも
#   空のツリー・前提なし・依存なしとして続けずに exit 3。PR かどうかはどの経路も has("pull_request") で判定する（欄があれば値が null でも
#   PR とみなして止める側に倒す）。ルート自身は 0 で照合する（sub_issues の要素には親を識別する欄が無く、1 の list_subs では
#   ルートが対象リポジトリの issue かを確かめられないため）。
#
# 出力（stdout）: 人手 issue ごとに 1 行の JSON {"number":n,"title":"…","advances":k,"waiting_on":[…],"waiting_outside":[…]} を、
#   waiting_on と waiting_outside がどちらも空のもの → どちらかが空でないもの、その中で advances の多い順、同じなら番号の小さい順に出し、最後に
#   result=ok root=<n> open=<ツリーの open issue 数> humans=<人手 issue 数> repo=<渡された --repo のまま> を出す（humans=0 なら JSON の行は無い）。
#   title は tree-lib.sh の CELL で変換し、[ と ] も文字参照にし、裸の URL の自動リンクを消す変換（`://` の : と www. の . を文字参照）も
#   した題名（表のセルへそのまま入れられる。残る制約: メールアドレスは文字参照にしても mailto のリンクになる。外部サイトへの遷移ではないので許容）。失敗時は stdout に何も出さない。
#
# 終了コード: 0 数えた / 1 引数の誤り（gh は呼ばない）/ 2 前提不備（gh・jq が無い、スクリプトの symlink を実体まで辿れない、
#              tree-lib.sh を読み込めない・読み込んだ tree-lib.sh が list_subs か CELL を定義していない、同梱の
#              declared-deps-filter.jq が無い・読めない・空・壊れている〔試しの入力で jq が失敗する・出力の形が違う〕。gh は呼ばない）/
#            3 GitHub の読み取りか jq の失敗（ルート・ツリー・issue・ツリー外の前提の取得、フィルタの適用、フィルタ・ツリー外の前提・
#              sub-issue〔repository_url が無い・文字列でない・API の URL の形式でない、番号・state・題名・ラベルの型が違う〕・
#              issue の本文〔body が無い・文字列でも null でもない〕の応答が想定の形でない、issues/<n> の応答が対象リポジトリの
#              issue と確かめられない〔repository_url が無い・形式が違う・別リポジトリ、番号が違う、ルート・ツリー内の issue が PR、
#              応答が 1 件のオブジェクトでない〕。失敗した issue を依存なし・前提なし・
#              人手でないとして続けない）
#   エラーは stderr へ出す。
#
# 依存の抽出フィルタ: このスクリプトの実体と同じディレクトリ（SCRIPT_DIR。symlink を辿った先で、tree-lib.sh と同じ基準）の
#   declared-deps-filter.jq を読んで使う。中身は同じ版の implement-issue-tree の scripts/declared-deps.sh --print-filter（兄弟の
#   タスク ID の表なし）の出力の写しで、依存の読み方を同じ版の implement-issue-tree と揃えるために同梱する。スキルを個別に
#   更新すると、導入済みの implement-issue-tree と版がずれ、読み方が違い得る。読み込みの後、gh を呼ぶ前に小さな入力で試しに
#   通し、壊れていれば前提不備で止める。以前は implement-issue-tree の declared-deps.sh を探して実行して
#   いたが、create-issue-tree だけを導入した配置では数えられず、探す場所によっては信頼していない checkout のコードを実行し得た
#   ため、他スキルのスクリプトを探すことも実行することもやめた。
#   写しは 1 行の jq と末尾の改行 1 つで、コメントを入れない（jq が評価するフィルタが正本と同じになるように）。読み込みのコマンド
#   置換が末尾の改行を落とすので、gh api --jq へ渡す文字列は、--print-filter の出力から末尾の改行 1 つを除いたものを、本文の形を
#   確かめる固定の if … then (( と ) + {repository_url, pr: has("pull_request")}) else error(…) end で包んだもの（DEPS_JQ）になる。
#   形の正しい応答ではフィルタの評価は変わらず、出力に照合用の repository_url と pr が足されるだけ（本文は出さない）。
#   正本は上流（Fandhe-AI/agent-cli-skills）の skills/implement-issue-tree/scripts/lib/declared-deps-lib.sh で、正本が変わると上流の
#   dev/create-issue-tree/tests/human-blockers.test.mjs の一致テストが落ちる。そのときは手で書き写さず、上流のリポジトリ根で次を実行して
#   作り直す（--print-filter は gh を呼ばない）:
#     bash skills/implement-issue-tree/scripts/declared-deps.sh --print-filter > skills/create-issue-tree/scripts/declared-deps-filter.jq
#
# 自衛: gh api の GET だけ。書き込みはせず、ファイルも作らない（一時ファイルも使わない）。issue の題名・本文は非信頼データで、
#       題名は CELL で変換してから出し、本文は出さない。集計のデータは標準入力で jq へ渡し、件数で伸びる値（ツリー外の前提を
#       含む）をコマンドライン引数に載せない（引数の長さの上限で jq を起動できなくならないように）。
# 互換: macOS（bash 3.2）と Linux。連想配列・mapfile を使わない。set -e は使わず、各所の `|| die` で失敗を明示処理する。
# 環境: 呼び出し元のシェルの設定で数え方を変えない。CDPATH は cd の前に消す（Step 7 のフェンスは相対パスで起動するので、
#       CDPATH があると cd が別の場所を選び、移り先を stdout へ書いて SCRIPT_DIR を壊す）。人手 issue の行は grep ではなく
#       jq で数える（GREP_OPTIONS を読む grep では数が変わる）。ほかに冒頭で次を外す:
#       - 呼び出し元から export された関数（BASH_FUNC_<名前>%%）。使う名前の関数を消し、同名のコマンド・組み込みを呼ぶ。
#         unset と builtin 自身の関数は、このスクリプトの中からは外せない。BASH_ENV のファイルはこのスクリプトより先に
#         動くので中からは防げず、Step 7 のフェンスが BASH_ENV を外して起動する
#       - xtrace（SHELLOPTS で持ち込める）。sub_issues の応答の本文がトレースで stderr へ出るため
#       - gh の出力の形を変える環境変数（GH_FORCE_TTY・CLICOLOR_FORCE は色・TTY の書式、GH_DEBUG・DEBUG はデバッグ出力）。
#         NO_COLOR=1 を渡す
#       - jq が HOME の ~/.jq を自動で読み込むこと（組み込みの関数を差し替えられる。ラベルと repository_url の比較に使う
#         ascii_downcase など）。jq は HOME を存在しない場所にして呼ぶ関数で包み、tree-lib.sh の list_subs の jq にも効かせる
# 単体実行用であり source しない。
unset -f gh jq cat dirname readlink pwd cd command printf echo declare source set exit shift local
# 制御に使う組み込み（true・false・:・[・break・continue・return・export）も同じく消す。true を非ゼロにする関数があると
# ループが 1 回も回らずに空の結果で成功する
unset -f true false : '[' break continue return export
set +x
unset CDPATH GH_FORCE_TTY CLICOLOR_FORCE GH_DEBUG DEBUG
export NO_COLOR=1
set -uo pipefail

die() {
  local code="$1"
  shift
  echo "エラー: $*" >&2
  exit "${code}"
}

USAGE='使い方: human-blockers.sh --root <n> --repo <host>/<owner>/<repo>'

ROOT_NUMBER=''
REPO=''
HAVE_ROOT=0
HAVE_REPO=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root | --repo)
      [[ $# -ge 2 ]] || die 1 "$1 に値がありません。${USAGE}"
      if [[ "$1" == "--root" ]]; then
        [[ "${HAVE_ROOT}" -eq 0 ]] || die 1 "--root は 1 回だけ。${USAGE}"
        ROOT_NUMBER="$2"
        HAVE_ROOT=1
      else
        [[ "${HAVE_REPO}" -eq 0 ]] || die 1 "--repo は 1 回だけ。${USAGE}"
        REPO="$2"
        HAVE_REPO=1
      fi
      shift 2
      ;;
    *) die 1 "未知の引数: $1。${USAGE}" ;;
  esac
done
[[ "${ROOT_NUMBER}" =~ ^[1-9][0-9]*$ ]] || die 1 "--root は正整数で指定する。${USAGE}"
# host は名前と任意のポート（GHES の host:port。Step 5 のスクリプト・検証のフェンスと同じ形）
[[ "${REPO}" =~ ^[A-Za-z0-9.-]+(:[0-9]{1,5})?/[A-Za-z0-9_-]+/[A-Za-z0-9._-]+$ ]] \
  || die 1 "--repo は <host>/<owner>/<repo> の形で指定する。${USAGE}"
HOST="${REPO%%/*}"
SLUG="${REPO#*/}"
case "${SLUG##*/}" in
  . | ..) die 1 "--repo のリポジトリ名が不正。${USAGE}" ;;
esac

command -v gh >/dev/null 2>&1 || die 2 "gh が見つかりません。"
command -v jq >/dev/null 2>&1 || die 2 "jq が見つかりません。"
# jq は ~/.jq を読まない形で呼ぶ（冒頭コメントの「環境」）。存在の確認（上の command -v）の後に定義する（関数があると
# command -v は関数の名前を返し、jq が無くても成功するため）。tree-lib.sh の source より前に定義し、list_subs の jq にも効かせる
jq() { HOME=/nonexistent command jq "$@"; }
# SCRIPT_DIR は実体のディレクトリの物理パス。tree-lib.sh と同梱のフィルタはどちらもここを基準にする（起動に使った symlink の
# 置き場所・作業ディレクトリを基準にしない）。ディレクトリの symlink だけなら pwd -P で足りるが、スクリプト自身がファイルの
# symlink だと dirname がリンクの置き場所になるので、先に BASH_SOURCE[0] を readlink で 1 段ずつ実体まで辿る。相対のリンクは
# リンクのあるディレクトリを基準にする。readlink -f は古い macOS に無いので使わない。40 段を超える連なりと readlink の失敗は
# 前提不備で止める
SCRIPT_PATH="${BASH_SOURCE[0]}"
HOPS=0
while [[ -L "${SCRIPT_PATH}" ]]; do
  HOPS=$((HOPS + 1))
  [[ "${HOPS}" -le 40 ]] || die 2 "スクリプトの symlink の連なりが 40 段を超えます。スクリプトの場所を決められません。"
  LINK=$(readlink -- "${SCRIPT_PATH}") || die 2 "スクリプトの symlink を読めません（readlink の失敗）。スクリプトの場所を決められません。"
  case "${LINK}" in
    /*) SCRIPT_PATH="${LINK}" ;;
    *) SCRIPT_PATH="$(dirname -- "${SCRIPT_PATH}")/${LINK}" ;;
  esac
done
SCRIPT_DIR=$(cd -P -- "$(dirname -- "${SCRIPT_PATH}")" && pwd -P) || die 2 "スクリプトの場所を決められません。"
# 環境から持ち込んだ値で読み込みを素通りさせない。TREE_LIB_LOADED は tree-lib.sh の二重 source のガードで、環境にあると
# tree-lib.sh は何も定義せずに return する。このスクリプトは単体実行専用なので、ここで消してから読む。読み込みの後で確かめる
# list_subs と CELL も、環境の関数（export -f）・変数を tree-lib.sh の定義と取り違えないよう先に消す
unset TREE_LIB_LOADED CELL
unset -f list_subs
# shellcheck source=tree-lib.sh
source "${SCRIPT_DIR}/tree-lib.sh" 2>/dev/null || die 2 "tree-lib.sh を読み込めません。"
declare -F list_subs >/dev/null && [[ -n "${CELL:-}" ]] \
  || die 2 "読み込んだ tree-lib.sh が list_subs か CELL を定義していません。数えません。"

# 依存の抽出フィルタは同梱の写しを読む（冒頭コメントの「依存の抽出フィルタ」）。gh を呼ぶ前に読み、読めなければ前提不備で止める
FILTER_FILE="${SCRIPT_DIR}/declared-deps-filter.jq"
[[ -f "${FILTER_FILE}" && -r "${FILTER_FILE}" ]] || die 2 "同梱の依存の抽出フィルタ ${FILTER_FILE} が無いか読めません。数えません。"
FILTER=$(cat "${FILTER_FILE}") || die 2 "同梱の依存の抽出フィルタ ${FILTER_FILE} を読めません。数えません。"
[[ -n "${FILTER}" ]] || die 2 "同梱の依存の抽出フィルタ ${FILTER_FILE} が空です。数えません。"
# フィルタは本文を (.body // "") で読み、欠落・false を空の本文にしてしまう。gh 内蔵の jq が読むのと同じ応答で本文の形を確かめて
# からフィルタへ渡す（body が有り、文字列か null。GitHub は空の本文を null で返す）。フィルタの中身は変えない。
# フィルタの出力へ同じ応答の repository_url と pull_request の有無を足し、後段（PATH の jq）で REPO_DEF により照合する
# （照合は --arg で slug を渡したいので gh の --jq の中ではしない。gh api の --jq には変数を渡せない）
DEPS_JQ="if type == \"object\" and has(\"body\") and ((.body | type) == \"string\" or .body == null) then ((${FILTER}) + {repository_url, pr: has(\"pull_request\")}) else error(\"issue の本文の形が想定外（body が無い・文字列でも null でもない）\") end"
# 壊れたフィルタ（構文の誤り・出力の形の違い）を gh の --jq で初めて失敗させず、ここで前提不備として止める。入力は gh api の
# issues/<n> の応答のうちフィルタが読む number と body だけの小さな issue（本文が空なので依存は無い）で、出力が
# {number: 1, deps: []} を含む 1 件のオブジェクトであることを確かめる。本番と同じ DEPS_JQ（本文の形の検査で包んだもの）で通す。
# 試し通しは PATH の jq、本番は gh 内蔵の jq で評価するので、両者の版の差による違いはここでは見つからない
PROBE=$(printf '%s' '{"number":1,"body":""}' | jq -c "${DEPS_JQ}" 2>/dev/null) \
  || die 2 "同梱の依存の抽出フィルタ ${FILTER_FILE} が壊れています（試しの入力で jq が失敗）。数えません。"
printf '%s\n' "${PROBE}" | jq -e -s 'length == 1 and (.[0] | type) == "object" and .[0].number == 1 and .[0].deps == []' >/dev/null 2>&1 \
  || die 2 "同梱の依存の抽出フィルタ ${FILTER_FILE} が壊れています（試しの入力の出力が {number, deps} の形でない）。数えません。"

# issues/<n> の応答（オブジェクト 1 件）が対象リポジトリの issue かを確かめる（冒頭コメントの「個別の issue の応答の照合」）。
# 結果は "ok" か理由。形式の正規表現は tree-lib.sh の list_subs と同じで、owner/repo は文字列で比べる（slug を正規表現へ
# 埋め込まない）。理由に出す owner/repo は形式の検査を通った値だけ（応答の値をそのまま出さない）
# shellcheck disable=SC2016  # jq の $ 変数
REPO_DEF='def repo_why($slug):
    .repository_url as $u
    | if ($u | type) != "string" then "repository_url が無い・文字列でない"
      elif ($u | test("\\Ahttps?://[A-Za-z0-9.-]+(:[0-9]{1,5})?/(api/v3/)?repos/[A-Za-z0-9_-]+/[A-Za-z0-9._-]+\\z"; "i")) | not
      then "repository_url が API の URL の形式でない"
      elif ($u | ascii_downcase | split("/") | .[-2:] | join("/")) != ($slug | ascii_downcase)
      then "別リポジトリ \($u | split("/") | .[-2:] | join("/")) の issue（移管された issue の可能性がある）"
      else "ok" end;'

# ---- 0. ルート（sub_issues を辿る前に、ルート自身が対象リポジトリの issue であることを確かめる）----
# 改名・移管で sub_issues の読み取りがリダイレクトされると、全要素が別リポジトリとして落ち、空のツリーで成功してしまう
# （冒頭コメントの「数え方」の 0）。照合の順は 2 と同じ（repository_url → 番号 → PR でない）。jq へ引数で渡す番号は
# どの経路も N の 1 つだけにそろえる（件数で伸びる値を引数に載せないことを上流のテストが字面で確かめる）
N="${ROOT_NUMBER}"
OUT=$(gh api --hostname "${HOST}" "repos/${SLUG}/issues/${N}" --jq '{number, pr: has("pull_request"), repository_url}') \
  || die 3 "ルート #${ROOT_NUMBER} を取得できません。数えません。"
# shellcheck disable=SC2016  # jq の $ 変数
WHY=$(printf '%s\n' "${OUT}" | jq -r -s --argjson n "${N}" --arg slug "${SLUG}" "${REPO_DEF}"'
    if length != 1 or (.[0] | type) != "object" then "応答が 1 件のオブジェクトでない"
    else .[0] | repo_why($slug) as $w
      | if $w != "ok" then $w
        elif (.number | type) != "number" or .number != $n then "応答の番号が違う"
        elif .pr != false then "PR である（issue ではない）"
        else "ok" end end') \
  || die 3 "ルート #${ROOT_NUMBER} の応答を ${REPO} の issue として照合できません（jq の失敗）。数えません。"
[[ "${WHY}" == "ok" ]] \
  || die 3 "ルート #${ROOT_NUMBER} の応答を ${REPO} の issue #${ROOT_NUMBER} と確かめられません（${WHY:-判定結果が空}）。数えません。リポジトリの改名・ルートの移管なら、対象リポジトリとルートの番号を確かめ直してください。"

# ---- 1. ツリー（ルートの子孫。対象リポジトリの要素だけ・closed の下も辿る）----
# NODES は [{number, state, title, labels（小文字）}]。SEEN は読んだ番号を空白で挟んで持つ（同じ issue を 2 度読まない）
NODES='[]'
QUEUE="${ROOT_NUMBER}"
SEEN=" ${ROOT_NUMBER} "
while [[ -n "${QUEUE}" ]]; do
  N="${QUEUE%% *}"
  if [[ "${QUEUE}" == *" "* ]]; then QUEUE="${QUEUE#* }"; else QUEUE=''; fi
  SUBS=$(list_subs "${N}" "${HOST}" "${SLUG}") || die 3 "#${N} の sub-issues を取得できません。"
  # 応答の形が想定外なら（番号・state・題名・ラベルの型）ここで止める。ラベルは人手の判定の必須の入力なので、欠落・null・false を
  # 空の配列として読まない（ラベルの無い issue の応答は空の配列）
  NEW=$({ printf '%s\n' "${NODES}"; printf '%s' "${SUBS}"; } | jq -c -s '.[0] + [.[1][]
      | if type == "object" and (.number | type) == "number" and (.state == "open" or .state == "closed")
          and (.title | type) == "string" and (.labels | type) == "array"
          and all(.labels[]; type == "object" and (.name | type) == "string")
        then {number, state, title, labels: [.labels[].name | ascii_downcase]}
        else error("sub-issue の要素の形が想定外") end]') \
    || die 3 "#${N} の sub-issues の応答を読めません。"
  KIDS=$(printf '%s' "${SUBS}" | jq -r '.[].number') || die 3 "#${N} の sub-issues の番号を読めません。"
  for C in ${KIDS}; do
    [[ "${C}" =~ ^[1-9][0-9]*$ ]] || die 3 "#${N} の sub-issue の番号が正整数ではありません。"
    case "${SEEN}" in *" ${C} "*) continue ;; esac
    SEEN="${SEEN}${C} "
    QUEUE="${QUEUE:+${QUEUE} }${C}"
  done
  NODES="${NEW}"
done
NODES=$(printf '%s' "${NODES}" | jq -c 'unique_by(.number)') || die 3 "ツリーの集約に失敗しました。"

# ---- 2. 依存（open な issue ごとに同梱のフィルタを通す）----
OPEN_NUMS=$(printf '%s' "${NODES}" | jq -r '.[] | select(.state == "open") | .number') \
  || die 3 "open な issue の番号を読めません。"
DEPS=''
for N in ${OPEN_NUMS}; do
  OUT=$(gh api --hostname "${HOST}" "repos/${SLUG}/issues/${N}" --jq "${DEPS_JQ}") \
    || die 3 "#${N} の依存を取り出せません（gh か jq の失敗、本文の形が想定外）。依存なしとして続けません。"
  # 応答が対象リポジトリの issue #N であること（repository_url → 番号 → PR でないの順。冒頭コメントの「個別の issue の応答の照合」）
  # shellcheck disable=SC2016  # jq の $ 変数
  WHY=$(printf '%s\n' "${OUT}" | jq -r -s --argjson n "${N}" --arg slug "${SLUG}" "${REPO_DEF}"'
      if length != 1 or (.[0] | type) != "object" then "応答が 1 件のオブジェクトでない"
      else .[0] | repo_why($slug) as $w
        | if $w != "ok" then $w
          elif (.number | type) != "number" or .number != $n then "応答の番号が違う"
          elif .pr != false then "PR である（issue ではない）"
          else "ok" end end') \
    || die 3 "#${N} の応答を ${REPO} の issue として照合できません（jq の失敗）。依存なしとして続けません。"
  [[ "${WHY}" == "ok" ]] \
    || die 3 "#${N} の応答を ${REPO} の issue #${N} と確かめられません（${WHY:-判定結果が空}）。依存なしとして続けません。"
  # 出力は {number, deps, sig, repository_url, pr} の 1 件だけで、number が依頼した番号、deps が正整数の配列であること
  # （repository_url と pr は上の WHY で照合済み。以降は number と deps だけを持つ）
  LINE=$(printf '%s\n' "${OUT}" | jq -c -s --argjson n "${N}" '
      if length == 1 and (.[0] | type) == "object" and .[0].number == $n and (.[0].deps | type) == "array"
          and all(.[0].deps[]; type == "number" and . == floor and . > 0)
      then {number: .[0].number, deps: .[0].deps}
      else error("依存の抽出結果の形が想定外") end') \
    || die 3 "#${N} の依存の抽出結果が想定の形ではありません。依存なしとして続けません。"
  DEPS="${DEPS}${LINE}"$'\n'
done

# 人手 issue の判定（下の 2 つの jq で共有する）
HUMAN_DEF='def human: .state == "open" and any(.labels[]; . == "owner:human" or . == "owner:joint");'

# ---- 2b. 人手 issue の前提のうちツリー外のもの（state を読む）----
# ツリー内は closed を含む NODES 全体で判定する（closed のツリー内の前提をツリー外として読まない）。番号ごとに 1 回だけ読む
# shellcheck disable=SC2016  # jq の $ 変数
OUTSIDE_NUMS=$({ printf '%s\n' "${NODES}"; printf '%s' "${DEPS}"; } | jq -r -s "${HUMAN_DEF}"'
    .[0] as $nodes | .[1:] as $deps
    | [$nodes[].number] as $all
    | [$nodes[] | select(human) | .number] as $humans
    | [$deps[] | .number as $x | select(any($humans[]; . == $x))
        | .deps[] | select(. != $x) | select(. as $d | any($all[]; . == $d) | not)]
    | unique | .[]') \
  || die 3 "ツリー外の前提の番号を読めません。"
OUTSIDE=''
for N in ${OUTSIDE_NUMS}; do
  [[ "${N}" =~ ^[1-9][0-9]*$ ]] || die 3 "ツリー外の前提の番号が正整数ではありません。"
  # issues/<n> は PR も返す。PR は state だけを読むと「ツリー外の open な issue の完了待ち」として表に出るので、pull_request の
  # 有無も同じ呼び出しで読み、PR なら止める（読み取りは番号ごとに 1 回のまま）
  # repository_url も同じ呼び出しで読み、番号より先に対象リポジトリの issue であることを確かめる（移管された issue は
  # リダイレクトで移管先の応答になる。冒頭コメントの「個別の issue の応答の照合」）
  OUT=$(gh api --hostname "${HOST}" "repos/${SLUG}/issues/${N}" --jq '{number, state, pr: has("pull_request"), repository_url}') \
    || die 3 "ツリー外の前提 #${N} の state を取得できません。前提なしとして続けません。"
  # shellcheck disable=SC2016  # jq の $ 変数
  WHY=$(printf '%s\n' "${OUT}" | jq -r -s --argjson n "${N}" --arg slug "${SLUG}" "${REPO_DEF}"'
      if length != 1 or (.[0] | type) != "object" then "応答が 1 件のオブジェクトでない"
      else .[0] | repo_why($slug) as $w
        | if $w != "ok" then $w
          elif (.number | type) != "number" or .number != $n then "応答の番号が違う"
          else "ok" end end') \
    || die 3 "ツリー外の前提 #${N} の応答を ${REPO} の issue として照合できません（jq の失敗）。前提なしとして続けません。"
  [[ "${WHY}" == "ok" ]] \
    || die 3 "ツリー外の前提 #${N} の応答を ${REPO} の issue #${N} と確かめられません（${WHY:-判定結果が空}）。前提なしとして続けません。移管された issue なら、その前提を書いた人手 issue の本文の \`## 前提\` を直してください。"
  LINE=$(printf '%s\n' "${OUT}" | jq -c -s --argjson n "${N}" '
      if length == 1 and (.[0] | type) == "object" and .[0].number == $n and (.[0].state == "open" or .[0].state == "closed")
        and (.[0].pr == true or .[0].pr == false)
      then {number: .[0].number, state: .[0].state, pr: .[0].pr}
      else error("ツリー外の前提の応答の形が想定外") end') \
    || die 3 "ツリー外の前提 #${N} の応答が想定の形ではありません。前提なしとして続けません。"
  # jq の失敗を空文字のまま「PR でない」として続けない（fail-open）。yes / no 以外は止める
  IS_PR=$(printf '%s' "${LINE}" | jq -r 'if .pr == true then "yes" elif .pr == false then "no" else error("pr") end') \
    || die 3 "ツリー外の前提 #${N} が PR かどうかを判定できません。前提なしとして続けません。"
  case "${IS_PR}" in yes | no) ;; *) die 3 "ツリー外の前提 #${N} が PR かどうかの判定結果が想定外です。前提なしとして続けません。" ;; esac
  if [[ "${IS_PR}" == "yes" ]]; then
    # この前提を書いた人手 issue の番号だけを示す（題名・本文は出さない）
    # shellcheck disable=SC2016  # jq の $ 変数
    WRITERS=$({ printf '%s\n' "${NODES}"; printf '%s' "${DEPS}"; } | jq -r -s --argjson n "${N}" "${HUMAN_DEF}"'
        .[0] as $nodes | .[1:] as $deps
        | [$nodes[] | select(human) | .number] as $humans
        | [$deps[] | .number as $x | select(any($humans[]; . == $x)) | select(any(.deps[]; . == $n)) | $x]
        | unique | map("#\(.)") | join("、")') || WRITERS='（特定できません）'
    die 3 "人手 issue ${WRITERS} の前提 #${N} は PR です。前提には issue の番号を書きます。その人手 issue の本文の \`## 前提\` を直してください。前提なしとして続けません。"
  fi
  LINE=$(printf '%s' "${LINE}" | jq -c '{number, state}') || die 3 "ツリー外の前提の集約に失敗しました。"
  OUTSIDE="${OUTSIDE}${LINE}"$'\n'
done
OUTSIDE_JSON=$(printf '%s' "${OUTSIDE}" | jq -c -s '.') || die 3 "ツリー外の前提の集約に失敗しました。"

# ---- 3・4. 辺と人手 issue ごとの数え上げ（1 つの jq に標準入力で渡す）----
# 入力の 1 件目が NODES、2 件目がツリー外の前提の [{number, state}]、3 件目以降が open な issue ごとの {number, deps}。
# ツリー外の前提も件数の上限が無いので --argjson（引数の長さの上限を受ける）ではなく標準入力で渡す
# shellcheck disable=SC2016  # jq の $ 変数
PROG_BODY='
  .[0] as $nodes | .[1] as $outs | .[2:] as $deps
  | ([$outs[] | select(.state == "open") | .number]) as $outopen
  | ($deps | map({key: (.number | tostring), value: .deps}) | from_entries) as $raw
  | ([$nodes[] | select(.state == "open") | .number]) as $open
  | ($open | map({key: tostring, value: true}) | from_entries) as $isopen
  # x → x の前提（open でツリー内・自分以外。昇順）
  | ($deps | map(.number as $x | {key: ($x | tostring),
      value: ([.deps[] | select(. != $x and ($isopen[tostring] == true))] | unique)}) | from_entries) as $pre
  | def succ($h):
      {set: [$h], front: [$h]}
      | until((.front | length) == 0;
          .set as $s | .front as $f
          | ([$pre | to_entries[] | select(any(.value[]; . as $v | any($f[]; . == $v))) | .key | tonumber]
              | unique | map(select(. as $y | any($s[]; . == $y) | not))) as $new
          | {set: ($s + $new), front: $new})
      | .set | map(select(. != $h)) | length;
    [$nodes[] | select(human)]
  | map({number, title: (.title | cell), advances: succ(.number),
          waiting_on: ($pre[.number | tostring] // error("open な issue の依存が無い")),
          waiting_outside: ([$raw[.number | tostring][] | select(. as $d | any($outopen[]; . == $d))] | unique)})
  | sort_by([(if (.waiting_on | length) + (.waiting_outside | length) == 0 then 0 else 1 end), -.advances, .number])
  | .[]'
# 題名が表の中でリンク・画像にならないよう、CELL の変換の後に [ と ] も文字参照にする（CELL は update-issue-tree と揃える契約が
# あるので変えない）。裸の URL も GFM の自動リンクでクリックできてしまうので、scheme の : を（直後が // のときだけ）&#58; に、
# 大文字小文字を問わない www の . を &#46; にして自動リンクを消す（題名の他の : と . は読みやすさのため変えない）
TITLE_CELL="${CELL}"' | gsub("\\["; "&#91;") | gsub("\\]"; "&#93;") | gsub(":(?=//)"; "&#58;") | gsub("(?<w>[Ww][Ww][Ww])\\."; "\(.w)&#46;")'
RESULT=$({ printf '%s\n' "${NODES}"; printf '%s\n' "${OUTSIDE_JSON}"; printf '%s' "${DEPS}"; } \
    | jq -c -s "def cell: ${TITLE_CELL}; ${HUMAN_DEF} ${PROG_BODY}") \
  || die 3 "人手 issue の数え上げに失敗しました。"
OPEN_COUNT=$(printf '%s' "${NODES}" | jq '[.[] | select(.state == "open")] | length') \
  || die 3 "open な issue の数を数えられません。"
HUMANS=0
if [[ -n "${RESULT}" ]]; then
  HUMANS=$(printf '%s\n' "${RESULT}" | jq -s 'length') || die 3 "人手 issue の行を数えられません。"
  printf '%s\n' "${RESULT}"
fi
echo "result=ok root=${ROOT_NUMBER} open=${OPEN_COUNT} humans=${HUMANS} repo=${REPO}"
