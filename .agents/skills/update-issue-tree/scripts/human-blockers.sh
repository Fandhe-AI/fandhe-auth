#!/usr/bin/env bash
# human-blockers.sh — ツリーの open な人手 issue ごとに、依存する後続 issue の数と未完了の前提を数える（読み取り専用）
#
# 呼び出し元: skills/create-issue-tree/SKILL.md Step 7（完了レポートの人の作業の表）。
# 写し: update-issue-tree の scripts/ に、このファイル・tree-lib.sh・declared-deps-filter.jq と同じ内容の写しがあり、
#       update-issue-tree の SKILL.md Step 9（報告の人手の待ち行列）が実行する。正本は create-issue-tree の scripts/ の 3 ファイルで、
#       どれかを変えたら写しを手で直さず、上流のリポジトリ根で次を実行して写し直す（一致は上流の
#       dev/update-issue-tree/tests/human-queue.test.mjs が確かめる）:
#         for f in human-blockers.sh tree-lib.sh declared-deps-filter.jq; do cp "skills/create-issue-tree/scripts/${f}" skills/update-issue-tree/scripts/; done
# 役割: 数え方は以前、SKILL.md の文章の手順としてエージェントが gh と jq を組み立てて数えていた。依存の読み方が
#       implement-issue-tree とずれる余地と、取得の失敗を「依存なし」と取り違える余地があったため、スクリプトへ移した。
#
# 使い方: human-blockers.sh --root <n> --repo <host>/<owner>/<repo> [--max-bytes <b> [--offset <k>]]
#         human-blockers.sh --root <n> --repo <host>/<owner>/<repo> --rejected
#   --root  ルート issue 番号（正整数）
#   --repo  対象リポジトリ。Step 5 の状態ファイルのヘッダの repo と同じ形（host は名前と任意のポート）
#   --max-bytes  ページ分割モード。1 ページに出す人手 issue の行の合計バイト数の上限（各行の UTF-8 のバイト数 + 改行 1）。
#                1024〜1000000 の整数。渡さないときは全行を 1 回で出す（従来の出力とバイト単位で同じ）。エージェントの
#                Bash ツールの出力上限（24,000 バイト）を超えて末尾の件数行が読めなくなるのを避けるために使う
#   --offset     ページの開始位置（並べた行の 0 始まりの番号）。--max-bytes が無いときは指定できない（exit 1）。省略時は 0
#   --rejected   別モード（Issue #923）。人手 issue ではなく、却下・取りやめで閉じた前提（下の「--rejected モード」）を数える。
#                --max-bytes を足すと前提の行を 1 ページの上限バイト数で切り、最後の行に offset=・shown=・next= を足す
#                （--offset は --max-bytes と一緒に指定する。人手の行のページ分割と同じ規則）。引数なしの出力はバイト単位で変わらない
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
# --rejected モード（Issue #923）: implement-issue-tree は、前提の issue が not planned・duplicate で閉じられていると（承認の却下など）
#   その前提に依存する open な issue を blocked にして着手しない。この棚卸しは、同じ判定で止まる open な issue を事前に見つける。
#   ツリー（上の 1）の closed な issue の state_reason と、open な issue の deps（上の 2）のうちツリー外で closed な前提の
#   state_reason（issues/<n> の同じ応答から読む。呼び出しは増やさない）を使い、前提ごとに 1 行の JSON
#     {"prereq":d,"reason":"not_planned|duplicate|other","dependents":[…]}
#   を前提の番号の昇順で出し（dependents は昇順の open な issue の番号。1 行に 100 件までで、超える前提は同じ prereq の行を続けて出す。
#   --max-bytes・--offset の単位はこの行）、最後に result=ok root=<n> rejected=<前提の数> repo=<…> を出す。
#   state_reason が completed・null・空の closed は解消済みで出さない。それ以外の値は other（implement-issue-tree も未解消として止める）。
#   自分の祖先・ルート自身への依存は implement-issue-tree が待たないので数えない（ただしツリー内の祖先に限る。限界は次の段落）。題名・本文は出さない。sub-issue と issues/<n> の
#   state_reason の欄が無い・文字列でも null でもないときは exit 3（欄の欠落を解消済みとして数えない）。ツリー外の前提が PR のときは
#   このモードでは数えずに飛ばす（PR は issue の close の理由を持たない）。書き込みはしない。
#   限界（Issue #1000）: ルートより上の祖先（ルートを sub-issue に持つ issue など）への依存は数える。このスクリプトはルートより上を辿らず、
#   自分の祖先の判定はツリー内の親の連鎖でしか行わないので、ルートの祖先はツリー外の前提として読まれる。そのため not planned・duplicate で
#   閉じたルートの祖先を前提に書いた open な issue は、implement-issue-tree が待たないのに行として出る（過剰に出る側の限界。上の open の表の
#   説明の「ルートの祖先は waiting_outside に出る」と同じ食い違い）。祖先を読む gh の呼び出しを足すと複雑さが増えるので見分けない。
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
#   ページ分割モード（--max-bytes あり）の追加: 1 は --max-bytes・--offset の誤り。3 は --offset が人手 issue 数以上（offset > 0。
#   数えている間にツリーが変わった可能性があるので offset 0 からやり直す。stdout は空）。
#
# ページ分割: --max-bytes を渡すと、並べた行（上の並び）のうち --offset 番目から、合計が上限を超える手前までを出す。残りがあれば
#   最低 1 行は出すので、1 行だけで上限を超える行は、その行だけのページになる（限界）。最終行は 2 行になり、
#     result=ok root=<n> open=<o> humans=<全体の人手 issue 数> repo=<渡された --repo のまま> offset=<k> shown=<このページの行数> next=<k+shown | end>
#     sig=<全行の指紋>
#   sig は並べた全行（このページだけではない）を連結した文字列の指紋で、ページを取り直す間にツリーが変わったことを見つけるためのもの
#   （改ざんの検知ではない）。ページごとに GitHub を読み直すので、読み取りの回数はページ数倍になる（ファイルは作らない）。
#   humans=0・offset=0 は shown=0 next=end で成功する。--max-bytes を渡さないときの出力は従来と同じ。
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
#   作り直したら、冒頭の「写し」の cp で update-issue-tree の scripts/ へも写す。
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

USAGE='使い方: human-blockers.sh --root <n> --repo <host>/<owner>/<repo> [--max-bytes <b> [--offset <k>]] | --rejected'

ROOT_NUMBER=''
REPO=''
HAVE_ROOT=0
HAVE_REPO=0
MAX_BYTES=''
OFFSET=''
HAVE_MAX=0
HAVE_OFFSET=0
REJECTED=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --rejected)
      [[ "${REJECTED}" -eq 0 ]] || die 1 "--rejected は 1 回だけ。${USAGE}"
      REJECTED=1
      shift
      ;;
    --root | --repo | --max-bytes | --offset)
      [[ $# -ge 2 ]] || die 1 "$1 に値がありません。${USAGE}"
      if [[ "$1" == "--max-bytes" ]]; then
        [[ "${HAVE_MAX}" -eq 0 ]] || die 1 "--max-bytes は 1 回だけ。${USAGE}"
        MAX_BYTES="$2"
        HAVE_MAX=1
      elif [[ "$1" == "--offset" ]]; then
        [[ "${HAVE_OFFSET}" -eq 0 ]] || die 1 "--offset は 1 回だけ。${USAGE}"
        OFFSET="$2"
        HAVE_OFFSET=1
      elif [[ "$1" == "--root" ]]; then
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
if [[ "${HAVE_MAX}" -eq 1 ]]; then
  [[ "${MAX_BYTES}" =~ ^[1-9][0-9]{3,6}$ && "${MAX_BYTES}" -ge 1024 && "${MAX_BYTES}" -le 1000000 ]] \
    || die 1 "--max-bytes は 1024〜1000000 の整数で指定する。${USAGE}"
  [[ "${HAVE_OFFSET}" -eq 0 ]] && OFFSET=0
  [[ "${OFFSET}" =~ ^(0|[1-9][0-9]{0,8})$ ]] || die 1 "--offset は 0 以上の整数（先頭の 0 なし）で指定する。${USAGE}"
else
  [[ "${HAVE_OFFSET}" -eq 0 ]] || die 1 "--offset は --max-bytes と一緒に指定する。${USAGE}"
fi
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
  NEW=$({ printf '%s\n' "${NODES}"; printf '%s' "${SUBS}"; } | jq -c -s --argjson n "${N}" --arg rej "${REJECTED}" '.[0] + [.[1][]
      | if type == "object" and (.number | type) == "number" and (.state == "open" or .state == "closed")
          and (.title | type) == "string" and (.labels | type) == "array"
          and all(.labels[]; type == "object" and (.name | type) == "string")
          and ($rej == "0" or (has("state_reason") and ((.state_reason | type) == "string" or .state_reason == null)))
        then {number, state, title, labels: [.labels[].name | ascii_downcase], parent: $n, reason: .state_reason}
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
OUTSIDE_NUMS=$({ printf '%s\n' "${NODES}"; printf '%s' "${DEPS}"; } | jq -r -s --arg rej "${REJECTED}" --arg root "${ROOT_NUMBER}" "${HUMAN_DEF}"'
    .[0] as $nodes | .[1:] as $deps
    | [$nodes[].number] as $all
    | [$nodes[] | select(human) | .number] as $humans
    | [$deps[] | .number as $x | select($rej == "1" or any($humans[]; . == $x))
        | .deps[] | select(. != $x) | select($rej == "0" or . != ($root | tonumber)) | select(. as $d | any($all[]; . == $d) | not)]
    | unique | .[]') \
  || die 3 "ツリー外の前提の番号を読めません。"
OUTSIDE=''
for N in ${OUTSIDE_NUMS}; do
  [[ "${N}" =~ ^[1-9][0-9]*$ ]] || die 3 "ツリー外の前提の番号が正整数ではありません。"
  # issues/<n> は PR も返す。PR は state だけを読むと「ツリー外の open な issue の完了待ち」として表に出るので、pull_request の
  # 有無も同じ呼び出しで読み、PR なら止める（読み取りは番号ごとに 1 回のまま）
  # repository_url も同じ呼び出しで読み、番号より先に対象リポジトリの issue であることを確かめる（移管された issue は
  # リダイレクトで移管先の応答になる。冒頭コメントの「個別の issue の応答の照合」）
  OUT_JQ='{number, state, pr: has("pull_request"), repository_url}'
  [[ "${REJECTED}" -eq 0 ]] || OUT_JQ='{number, state, state_reason, sr: has("state_reason"), pr: has("pull_request"), repository_url}'
  OUT=$(gh api --hostname "${HOST}" "repos/${SLUG}/issues/${N}" --jq "${OUT_JQ}") \
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
  LINE=$(printf '%s\n' "${OUT}" | jq -c -s --argjson n "${N}" --arg rej "${REJECTED}" '
      if length == 1 and (.[0] | type) == "object" and .[0].number == $n and (.[0].state == "open" or .[0].state == "closed")
        and (.[0].pr == true or .[0].pr == false)
        and ($rej == "0" or (.[0].sr == true and ((.[0].state_reason | type) == "string" or .[0].state_reason == null)))
      then {number: .[0].number, state: .[0].state, pr: .[0].pr, reason: .[0].state_reason}
      else error("ツリー外の前提の応答の形が想定外") end') \
    || die 3 "ツリー外の前提 #${N} の応答が想定の形ではありません。前提なしとして続けません。"
  # jq の失敗を空文字のまま「PR でない」として続けない（fail-open）。yes / no 以外は止める
  IS_PR=$(printf '%s' "${LINE}" | jq -r 'if .pr == true then "yes" elif .pr == false then "no" else error("pr") end') \
    || die 3 "ツリー外の前提 #${N} が PR かどうかを判定できません。前提なしとして続けません。"
  case "${IS_PR}" in yes | no) ;; *) die 3 "ツリー外の前提 #${N} が PR かどうかの判定結果が想定外です。前提なしとして続けません。" ;; esac
  if [[ "${IS_PR}" == "yes" && "${REJECTED}" -eq 1 ]]; then
    continue
  fi
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
  LINE=$(printf '%s' "${LINE}" | jq -c '{number, state, reason}') || die 3 "ツリー外の前提の集約に失敗しました。"
  OUTSIDE="${OUTSIDE}${LINE}"$'\n'
done
OUTSIDE_JSON=$(printf '%s' "${OUTSIDE}" | jq -c -s '.') || die 3 "ツリー外の前提の集約に失敗しました。"

# ---- --rejected: 却下・取りやめで閉じた前提と、それに依存する open な issue（Issue #923）----
# 入力の 1 件目が NODES（reason = sub_issues の state_reason、parent = 親の番号）、2 件目がツリー外の前提の [{number, state, reason}]、
# 3 件目以降が open な issue ごとの {number, deps}。closed の前提のうち reason が completed・null・空以外のものだけを前提ごとに集める。
# 自分の祖先への依存は implement-issue-tree が待たないので数えない（親の番号の連鎖で判定）
if [[ "${REJECTED}" -eq 1 ]]; then
  # shellcheck disable=SC2016  # jq の $ 変数
  REJ=$({ printf '%s\n' "${NODES}"; printf '%s\n' "${OUTSIDE_JSON}"; printf '%s' "${DEPS}"; } | jq -c -s '
      def cls($r): (($r // "") | ascii_downcase) as $x
        | if $x == "" or $x == "completed" then null
          elif $x == "not_planned" or $x == "duplicate" then $x else "other" end;
      .[0] as $nodes | .[1] as $outs | .[2:] as $deps
      | ($nodes | map({key: (.number | tostring), value: .parent}) | from_entries) as $par
      | ([$nodes[] | select(.state == "closed") | {key: (.number | tostring), value: cls(.reason)}]
          + [$outs[] | select(.state == "closed") | {key: (.number | tostring), value: cls(.reason)}] | from_entries) as $why
      | def anc($x; $d): ([$x | recurse($par[tostring]; . != null)] | index($d)) != null;
      [$deps[] | .number as $x | .deps[] | select(. != $x) | select($why[tostring] != null)
        | select(anc($x; .) | not) | {prereq: ., reason: $why[tostring], x: $x}]
      | group_by(.prereq) | map({prereq: .[0].prereq, reason: .[0].reason, dependents: ([.[].x] | unique)})
      | sort_by(.prereq)
      | .[] | . as $g | ($g.dependents | length) as $len
      | range(0; ([$len, 1] | max); 100) as $i
      | {prereq: $g.prereq, reason: $g.reason, dependents: $g.dependents[$i:$i + 100]}') \
    || die 3 "却下・取りやめで閉じた前提の数え上げに失敗しました。"
  COUNT=0
  ROWS=0
  if [[ -n "${REJ}" ]]; then
    COUNT=$(printf '%s\n' "${REJ}" | jq -s 'map(.prereq) | unique | length') || die 3 "前提の行を数えられません。"
    ROWS=$(printf '%s\n' "${REJ}" | jq -s 'length') || die 3 "前提の行を数えられません。"
    [[ "${HAVE_MAX}" -eq 1 ]] || printf '%s\n' "${REJ}"
  fi
  if [[ "${HAVE_MAX}" -eq 0 ]]; then
    echo "result=ok root=${ROOT_NUMBER} rejected=${COUNT} repo=${REPO}"
    exit 0
  fi
  # ページ分割（--max-bytes あり）。人手の行のページ分割と同じ規則で、offset 番目から上限を超える手前までを出す
  if [[ "${OFFSET}" -gt 0 && "${OFFSET}" -ge "${ROWS}" ]]; then
    die 3 "--offset ${OFFSET} が前提の行数 ${ROWS} 以上です。offset 0 からやり直してください。"
  fi
  RSHOWN=$(printf '%s\n' "${REJ}" | jq -Rn --arg o "${OFFSET}" --arg m "${MAX_BYTES}" '
      [inputs | select(length > 0)] as $l | ($o | tonumber) as $o | ($m | tonumber) as $m
      | reduce $l[$o:][] as $x ({n: 0, b: 0, stop: false};
          if .stop then .
          else (($x | utf8bytelength) + 1) as $s
            | if .n > 0 and .b + $s > $m then .stop = true else .n += 1 | .b += $s end
          end)
      | .n') || die 3 "ページに載せる行数を求められません。"
  [[ "${RSHOWN}" =~ ^[0-9]+$ ]] || die 3 "ページに載せる行数が想定の形ではありません。"
  if [[ "${RSHOWN}" -gt 0 ]]; then
    RPAGE=$(printf '%s\n' "${REJ}" | jq -Rrn --arg o "${OFFSET}" --arg n "${RSHOWN}" \
      '($o | tonumber) as $o | ($n | tonumber) as $n | [inputs | select(length > 0)] | .[$o:$o + $n][]') || die 3 "ページの行を切り出せません。"
    [[ -n "${RPAGE}" ]] || die 3 "ページの行が空です。"
    printf '%s\n' "${RPAGE}"
  fi
  RNEXT=$((OFFSET + RSHOWN))
  if [[ "${RNEXT}" -ge "${ROWS}" ]]; then RNEXT=end; fi
  echo "result=ok root=${ROOT_NUMBER} rejected=${COUNT} repo=${REPO} offset=${OFFSET} shown=${RSHOWN} next=${RNEXT}"
  exit 0
fi

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
fi
if [[ "${HAVE_MAX}" -eq 0 ]]; then
  [[ -z "${RESULT}" ]] || printf '%s\n' "${RESULT}"
  echo "result=ok root=${ROOT_NUMBER} open=${OPEN_COUNT} humans=${HUMANS} repo=${REPO}"
  exit 0
fi
# ページ分割モード。開始位置が人手 issue の数以上なら、数えている間にツリーが変わった可能性があるので何も出さず止める
if [[ "${OFFSET}" -gt 0 && "${OFFSET}" -ge "${HUMANS}" ]]; then
  die 3 "--offset ${OFFSET} が人手 issue 数 ${HUMANS} 以上です。数えている間にツリーが変わった可能性があるので、offset 0 からやり直してください。"
fi
SHOWN=$(printf '%s\n' "${RESULT}" | jq -Rn --arg o "${OFFSET}" --arg m "${MAX_BYTES}" '
    [inputs | select(length > 0)] as $l | ($o | tonumber) as $o | ($m | tonumber) as $m
    | reduce $l[$o:][] as $x ({n: 0, b: 0, stop: false};
        if .stop then .
        else (($x | utf8bytelength) + 1) as $s
          | if .n > 0 and .b + $s > $m then .stop = true else .n += 1 | .b += $s end
        end)
    | .n') || die 3 "ページに載せる行数を求められません。"
[[ "${SHOWN}" =~ ^[0-9]+$ ]] || die 3 "ページに載せる行数が想定の形ではありません。"
SIG=$(printf '%s' "${RESULT}" | jq -Rs 'explode | reduce .[] as $c (0; (. * 31 + $c) % 2147483647)') \
  || die 3 "全行の指紋を求められません。"
[[ "${SIG}" =~ ^[0-9]+$ ]] || die 3 "全行の指紋が想定の形ではありません。"
if [[ "${SHOWN}" -gt 0 ]]; then
  # sed などの外部コマンドに頼らず jq で行を切り出す。失敗したら result=ok を出さずに止める（行が欠けたまま成功にしない）
  PAGE=$(printf '%s\n' "${RESULT}" | jq -Rrn --arg o "${OFFSET}" --arg n "${SHOWN}" \
    '($o | tonumber) as $o | ($n | tonumber) as $n | [inputs | select(length > 0)] | .[$o:$o + $n][]') || die 3 "ページの行を切り出せません。"
  [[ -n "${PAGE}" ]] || die 3 "ページの行が空です。"
  printf '%s\n' "${PAGE}"
fi
NEXT=$((OFFSET + SHOWN))
if [[ "${NEXT}" -ge "${HUMANS}" ]]; then NEXT=end; fi
echo "result=ok root=${ROOT_NUMBER} open=${OPEN_COUNT} humans=${HUMANS} repo=${REPO} offset=${OFFSET} shown=${SHOWN} next=${NEXT}"
echo "sig=${SIG}"
