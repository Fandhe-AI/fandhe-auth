#!/usr/bin/env bash
# create-phase-issues.sh — 1 つの Phase の Phase 親・子 issue・sub-issue を spec（JSON）から起票し、下から順に紐付ける
#
# 呼び出し元: skills/create-issue-tree/SKILL.md Step 5（入力の spec は Step 4 でエージェントがファイル作成で書く）。
# 役割: 旧 Step 4・Step 5 は題名・本文をシェルのフェンスへ埋め込んで gh issue create していた。二重引用符では
#       `$(...)` が評価され、引用符付き heredoc でも終端と同じ行が本文にあれば後ろが実行され得た。依存は 1 件しか
#       書けず、フェンスごとにシェルが変わるため番号やフラグが渡らなかった（Issue #918）。本スクリプトは題名・本文を
#       JSON から読み、シェルで再評価せずに `--title "$t" --body-file <一時ファイル>` で渡す。
#
# 使い方: create-phase-issues.sh --root <n> --phase <N> --spec <spec.json> --state <状態ファイル>
#                                [--phase-number <n>] [--milestone <名前>]
#   --root          ルート issue 番号（正整数）
#   --phase         Phase 番号 N（正整数・先頭 0 なし。phase:N ラベル・feat(phase-N): 題名に使う。01 と 1 を別の Phase に
#                   しないため先頭 0 は拒否する）
#   --spec          spec のパス（下記）。開始時に 1 回だけ読み、以降は複製を使う
#   --state         状態ファイルのパス（無ければ作る。シンボリックリンクは不可）。途中で止まったら同じ spec と同じ
#                   状態ファイルで再実行する。実行中は隣に <状態ファイル>.lock（ディレクトリ）を作り、同じ状態ファイルでの
#                   並行実行を拒否する
#   --phase-number  再利用する Phase 親の番号（任意）。ルート直下の open な issue でなければ拒否する。省略時は
#                   ルート直下の open で、題名が feat(phase-N): で始まり phase:N ラベルを持つ最初の issue を再利用する
#   --milestone     付ける milestone 名（任意）。spec の milestone と両方あって値が違えば拒否する。
#                   SKILL.md はシェルへ自由文字列を埋め込まないため spec の milestone を使う
#
# spec（JSON。未知のキーは拒否）:
#   {"milestone": "<名前・任意>",
#    "phase": {"title": "feat(phase-N): …", "body": "…"},      … Phase 親を新設するときだけ使う
#    "children": [{"title", "body", "owner", "depends_on": [要素のパス], "depends_on_issues": [既存 issue 番号],
#                  "subs": [{"title", "body", "owner", "depends_on": [要素のパス], "depends_on_issues": […]}]}]}
#   owner は "" / "owner:human" のどちらか（省略は ""）。sub を持つ child の owner は "" でなければ拒否する
#   （Issue #904。子を持つ issue に owner ラベルを付けず、sub の owner は sub ごとの値だけを使う。親から複写しない）。
#   "owner:joint" は新規に発行しないので、ほかの不正な値と同じく gh を呼ぶ前に拒否する（Issue #905。人とエージェントの
#   作業が混ざるタスクは準備 → 人の作業 → 反映の 3 段に分ける。既存ツリーに残る owner:joint は、implement-issue-tree の
#   skipLabels で待たせる側の扱いで、このスクリプトは関わらない）。
#   depends_on は spec 内の要素を状態ファイルと同じパス（children[i] は "c<i>"、children[i].subs[j] は "c<i>.s<j>"）で
#   指す。指せるのは作成順（下記）で自分より先に作られる要素だけで、sub から自分の親 child（祖先）は指せない。別の親の
#   sub・先行する child・同じ親の先行する sub をどれも同じ書き方で指す。spec の外の issue は depends_on_issues に番号で書く。
#   配列のキーは、省略だけを空配列とみなす（null・false 等は配列ではないとして拒否する）。
#   implement-issue-tree は親の依存を子へ継承しないため、先行する要素（人手の作業等）に依存する sub は、sub 自身の
#   depends_on に書く。
#
# 処理順: 引数 → gh・jq → spec の全件検証（owner の値を含む）→ ロック → 状態ファイルの照合（番号の重複を含む）→
#         対象リポジトリの確定（最初の gh 呼び出し）と状態ファイルのリポジトリとの照合 → 読み取り（ルート・ルート直下・
#         Phase 親の探索と再確認・状態ファイルの番号の実物との照合・milestone の実在・depends_on_issues の参照先）→
#         状態ファイルのヘッダ・ラベルの確保（最初の書き込み）→ 起票と紐付け。spec か状態ファイルが 1 つでも不正なら gh を 1 回も呼ばずに止め、読み取りで
#         分かる不備（下の終了コード 1）では書き込まずに止める。
#         起票と紐付けは下から: child を作る → その sub を spec 順に作って child へ紐付ける → child を Phase 親へ紐付ける
#         → 全 child が済んでから Phase 親をルートへ紐付ける（新設した Phase 親が子の無いまま実装候補にならないように）。
#
# 状態ファイル（JSON Lines。1 行目はヘッダ
#   {"v":3,"root":n,"phase":"N","milestone":"<確定した milestone。無ければ空>","repo":"<host>/<owner>/<repo>"}、
#   以降は作成・採用した issue を 1 件ずつ追記）:
#   {"key": "phase" | "c<i>" | "c<i>.s<j>", "number": n, "created": true|false, "spec": <作成に使った要素>,
#    "subs": <作成時の sub の件数。c<i> だけ>}
#   再実行時は記録済みの番号を再利用して作り直さない。ただし番号は人が足した行（"number":0 の置き換え）のことがあるので
#   信用しない: 別のキーと同じ番号・ルートと同じ番号は gh を呼ぶ前に止め、記録済みの番号は最初の書き込みの前に実物を読んで、
#   対象リポジトリの open な issue（PR ではない）で、題名が spec の要素と一致する（前後の空白は除いて比べる。GitHub が
#   題名の前後の空白を落とすため）ことを確かめる（再利用した既存の Phase 親は題名が spec に無いので、ルート直下の open な
#   issue であることだけを確かめる）。
#   root・phase・milestone・repo の違い、spec に無いキー（要素数が減った等）、
#   作成済み要素の内容の変更（並べ替えを含む）、作成済みの子の sub の件数の変更（葉の子に sub を足す・sub を足す・
#   減らす）は食い違いとして止める（葉で作った子は Phase 親へ紐付け済みのことがあり、後から sub を足すと sub の
#   揃わない子がルートから辿れてしまうため）。許すのは未作成の要素の変更と、children の末尾への子の追加、
#   未作成の子の sub の追加だけ。repo は別の checkout や GH_REPO の変更で別リポジトリの同じ番号の issue を
#   再利用・紐付けしないために記録する。
#
# 不変条件:
#   - 対象リポジトリは開始時に gh repo view で 1 回だけ決め、以降の gh 呼び出しはすべて --repo <host>/<owner>/<repo>
#     か `gh api --hostname <host> repos/<owner>/<repo>/…` で明示する（{owner}/{repo} の解決に頼らない）。
#   - 題名・本文はシェルで再評価しない（jq から一時ファイルへ直接書き、gh へは引数 1 つ / --body-file で渡す）。
#   - 依存は `## 前提` 節を本文の末尾に足して行頭 `- #N` で全件書く。番号は状態ファイルに記録した検証済みの番号
#     （depends_on のパスで引く）と depends_on_issues だけから組み立てる。本文のコードフェンスが閉じていない spec と、implement-issue-tree が本文から
#     依存として拾う表現（依存の見出し・`Depends on #N`・兄弟のタスク ID の完了後・前の工程 等）を持つ spec は拒否する。
#     抽出規則は implement-issue-tree の scripts/lib/declared-deps-lib.sh の写しを下の jq に持つ（同居しない配置が
#     あるので source しない。規則を変えたら両方を直す。dev/create-issue-tree/tests が実物と突き合わせる）。
#   - 再開時は状態ファイルの Phase 親を今回の読み取りで確かめ直す（閉じられた・ルートから外された親へは起票しない）。
#   - 紐付けの前に親の sub_issues（全ページ）を読み、紐付いていれば POST しない。POST が失敗しても直後に読み直して
#     紐付いていれば成功とみなす（保存後に応答が途切れた場合）。sub_issues の要素は repository_url の末尾が
#     /repos/<owner>/<repo>（大小を区別しない）のものだけを対象リポジトリの issue とする（別リポジトリの sub-issue は
#     番号が同じでも別物。紐付けの確認・Phase 親の判定から除く。repository_url の無い要素も除く）。
#   - ラベル名は GitHub と同じく大小を区別せずに比べる（既存の Phase:1 を phase:1 として確保・判定する）。
#   - ラベル（phase:N と spec が使う owner ラベル）は最初の issue 作成より前に確保する。確保できなければ issue を作らない。
#   - milestone を付ける場合は、open な milestone に同じ題名（完全一致）があることを最初の書き込みの前に確かめる。
#   - depends_on_issues の番号は、最初の書き込みの前に重複を除いて 1 件ずつ読み、対象リポジトリの issue（PR ではない。
#     open・closed は問わない）であることを確かめる。番号の決まっている祖先は読む前に拒否する（implement-issue-tree は
#     祖先〔ルート・Phase 親・親 child〕への依存を無視するので、前提に書いても完了条件として効かない）。祖先はルート、確定済みの
#     Phase 親（状態ファイル・--phase-number・探索で決まった番号）、sub に対しては状態ファイルに作成済みと記録された親 child
#     （再開で、未作成の sub が親 child の番号を書いた場合）。番号がまだ無い祖先（Phase 親を新設する初回の実行の Phase 親、
#     新規実行の親 child〔sub より先に同じ実行で作るので、spec を書く時点で番号が無い〕）は比べない。
#     対象は状態ファイルに記録の無い（この実行で作る）要素の分で、どの要素もそれを作る実行で先に確かめる
#     （この検査より前の版で作った要素は確かめられていない）。
#   - 再利用する Phase 親の milestone は未設定のときだけ spec の値を設定し、別の値が付いていれば上書きしない。
#   - 一時ファイルと自分が取ったロックは成否によらず削除する（trap は mktemp・ロックより先に登録）。
#   - ルートは対象リポジトリの確定直後、最初の書き込みより前に読み、open な issue（PR ではない）でなければ止める
#     （SKILL.md Step 2.5 の確認を経ずに Step 5 だけを再実行しても、閉じたルートや PR の下へ起票しない）。
#   - 状態ファイル・ロック・一時ファイルは利用者専用にする（umask 077。既存の状態ファイルはグループ・他者の権限を外す）。
#     状態ファイルは spec の題名・本文を写して持つため、共有の一時ディレクトリでも他の利用者に読ませない。
#
# 終了コード: 0 完了 / 1 引数・spec・状態ファイルの不正、ロックが既にある、状態ファイルと対象リポジトリの食い違い、
#            ルートが open な issue でない（closed・PR）、Phase 親が不適（指定した親・状態ファイルの親が閉じている・
#            ルート直下に無い）、状態ファイルの番号が実物と合わない（題名の違い・closed・PR・別リポジトリ）、milestone が
#            open で実在しない、depends_on_issues の番号が PR・別リポジトリの issue（repository_url の無い応答を含む）・
#            ルート・確定済みの Phase 親・sub の作成済みの親 child（この実行では GitHub へ書き込んでいない）/
#            2 前提不備（gh・jq が使えない）/
#            3 GitHub 操作の失敗（対象リポジトリの確定・読み取り〔depends_on_issues の番号を読めない・応答の形が崩れている
#              〔対象リポジトリの issue だが番号が違う等〕場合を含む。このときは書き込み前〕・ラベル・作成・番号の抽出・紐付け・
#              状態ファイルへの追記。
#              途中まで作成済みのことがあるので、同じ spec と状態ファイルで再実行する。作成と記録の間の失敗は下記）/
#            129・130・143 HUP・INT・TERM で中断（途中まで作成済みのことがある。issue の作成中だった場合は下記）。
# 成功時の stdout 最終行: result=created root=<n> phase=<N> phase_number=<n> children=<k>。エラーは stderr へ出す。
#
# 作成と記録の間の失敗（exit 3。stderr に状態ファイルへ記録する 1 行の JSON を出す）:
#   - 番号が分かる追記の失敗: 作った issue の番号入りの行を出す。書けない原因を直し、状態ファイルの末尾の書きかけの行
#     （最後の改行より後ろ）を消してからその行を足し、再実行する。
#   - 番号が分からない失敗（作成の失敗・番号の抽出の失敗）: "number":0 の行を出す。実物を確かめ、作られていれば 0 を
#     その番号に置き換えて足し、作られていなければ何も足さずに再実行する（0 のまま足すと解析できない行として止まる。
#     別の要素の番号・無関係な issue の番号を書くと、再実行が上の照合で止まる）。
# issue の作成中の HUP・INT・TERM（exit 129・130・143）: bash は gh の終了を待ってから trap を動かすので、gh が URL を
#   出していれば番号を取り出して状態ファイルへ記録し、記録したことを stderr に書いて終える（記録できなければ番号入りの行を
#   出す）。gh も止められて番号が分からなければ、上と同じ "number":0 の行と案内を出す。
# 既知の限界: 記録せずに再実行すると同じ要素をもう一度作り、先に作られた issue はどこにも紐付かないまま残る
#   （ルートから辿れないので implement-issue-tree の対象にはならない。見つけたら閉じる）。issue の作成が成功してから
#   追記するまでの間にプロセスが trap を通らずに落ちた場合（SIGKILL 等）は stderr の行が出ないので、同じく実物を確かめて記録する。
#   SIGKILL 等で trap が動かずに終わるとロックが残る（次の実行が exit 1 で知らせるので、実行中でないことを確かめて消す）。
#   兄弟のタスク ID の規則は、実際の兄弟の ID（既存の子・別 Phase の親を含む）ではなく ID の形全般で判定するので、
#   実物の抽出より広く拒否する（拒否された表現は言い換える）。
#
# set -e は使わない。各所の `|| die ...` で失敗を明示処理する。単体実行用であり source しない。
# bash 3.2（macOS 標準）で動くよう、連想配列・mapfile・${var,,}・flock を使わない。
set -uo pipefail
# 作るファイル・ディレクトリ（状態ファイル・ロック・一時ファイル）を利用者専用にする（呼び出し元の umask に依らない）
umask 077

die() {
  local code="$1"
  shift
  echo "エラー: $*" >&2
  exit "${code}"
}

USAGE='使い方: create-phase-issues.sh --root <n> --phase <N> --spec <spec.json> --state <状態ファイル> [--phase-number <n>] [--milestone <名前>]'

ROOT_NUMBER=''
PHASE=''
SPEC=''
STATE=''
PHASE_NUMBER_ARG=''
MILESTONE_ARG=''
HAVE_MILESTONE_ARG=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root | --phase | --spec | --state | --phase-number | --milestone)
      [[ $# -ge 2 ]] || die 1 "$1 に値がありません。${USAGE}"
      case "$1" in
        --root) ROOT_NUMBER="$2" ;;
        --phase) PHASE="$2" ;;
        --spec) SPEC="$2" ;;
        --state) STATE="$2" ;;
        --phase-number) PHASE_NUMBER_ARG="$2" ;;
        --milestone)
          MILESTONE_ARG="$2"
          HAVE_MILESTONE_ARG=1
          ;;
      esac
      shift 2
      ;;
    *) die 1 "未知の引数: $1。${USAGE}" ;;
  esac
done
[[ "${ROOT_NUMBER}" =~ ^[1-9][0-9]*$ ]] || die 1 "--root は正整数で指定してください。${USAGE}"
[[ "${PHASE}" =~ ^[1-9][0-9]*$ ]] || die 1 "--phase は先頭 0 の無い正整数で指定してください（01・0 は不可）。${USAGE}"
[[ -z "${PHASE_NUMBER_ARG}" || "${PHASE_NUMBER_ARG}" =~ ^[1-9][0-9]*$ ]] \
  || die 1 "--phase-number は正整数で指定してください。"
[[ -n "${SPEC}" ]] || die 1 "--spec がありません。${USAGE}"
[[ -n "${STATE}" ]] || die 1 "--state がありません。${USAGE}"
# ハイフンで始まる相対パスは ./ を付けて 1 回だけ正規化する。以降は jq・chmod・dirname・mkdir・rmdir・rm（ロックは
# <状態ファイル>.lock）に位置引数で渡すので、そのままではオプションとして解釈される（jq は Unknown option、mkdir・dirname は
# illegal option）。`--` はコマンドと版によって効くか確かでないので頼らない
[[ "${SPEC}" == -* ]] && SPEC="./${SPEC}"
[[ "${STATE}" == -* ]] && STATE="./${STATE}"
[[ -f "${SPEC}" && -r "${SPEC}" ]] || die 1 "spec を読めません: ${SPEC}"
# 状態ファイルはシンボリックリンクを辿らない（共有の一時ディレクトリで、他人が置いたリンク先へ書き込まないように）
if [[ -L "${STATE}" ]]; then
  die 1 "状態ファイルにシンボリックリンクは使えません: ${STATE}"
elif [[ -e "${STATE}" ]]; then
  [[ -f "${STATE}" ]] || die 1 "状態ファイルが通常のファイルではありません: ${STATE}"
  [[ "${SPEC}" -ef "${STATE}" ]] && die 1 "spec と状態ファイルに同じファイルは使えません。"
  # 前回の実行や利用者が作った既存のファイルは umask の対象外なので、グループ・他者の権限を外してから使う
  # （空のファイルも。ヘッダを > で書いてもモードは変わらない）。所有者が違う等で外せなければ使わない
  chmod go-rwx "${STATE}" 2>/dev/null || die 1 "状態ファイルのグループ・他者の権限を外せません（所有者が違う等）: ${STATE}"
  [[ -r "${STATE}" && -w "${STATE}" ]] || die 1 "状態ファイルを読み書きできません: ${STATE}"
else
  STATE_DIR=$(dirname "${STATE}")
  [[ -d "${STATE_DIR}" && -w "${STATE_DIR}" ]] || die 1 "状態ファイルを作るディレクトリに書けません: ${STATE_DIR}"
fi

# gh・jq の欠落は前提不備（exit 2）にする
command -v gh >/dev/null 2>&1 || die 2 "gh が見つかりません。"
command -v jq >/dev/null 2>&1 || die 2 "jq が見つかりません。"

# trap を mktemp・ロックより先に登録する（2 回目の mktemp 失敗でも作成済みファイルを削除するため）。
# ロックは自分が作った場合（LOCK_HELD=1）だけ消す（拒否された 2 本目が 1 本目のロックを消さないように）。
# INT・TERM・HUP は on_signal で作成中の issue の記録を済ませてから exit し、EXIT の trap を通す。
# 一時ファイルの場所は雛形で明示する（macOS の mktemp は引数無しだと TMPDIR を無視して /var/folders に作り、テストが
# TMPDIR を見ても残存を検出できない）
SPEC_SNAP=''
BODY_FILE=''
OUT_FILE=''
LOCK_DIR="${STATE}.lock"
LOCK_HELD=0
# 作成中の要素のキー（gh issue create を呼ぶ直前に入れ、状態ファイルへ記録したら空にする）と、取り出した番号
CUR_KEY=''
CUR_NUMBER=''
cleanup() {
  rm -f "${SPEC_SNAP}" "${BODY_FILE}" "${OUT_FILE}"
  if [[ "${LOCK_HELD}" == 1 ]]; then
    rm -f "${LOCK_DIR}/owner"
    rmdir "${LOCK_DIR}" 2>/dev/null
  fi
}
# HUP・INT・TERM の処理。作成中の要素（CUR_KEY）があれば、gh の出力（OUT_FILE）から番号を取り出して状態ファイルへ記録し、
# 記録できたかを stderr に書く。番号が分からなければ "number":0 の記録用の行と案内を出す。state_add・die は使わない
# （それらの exit 3 でシグナルの終了コードを置き換えないため）。ここで使う関数（state_num 等）は後で定義するが、
# CUR_KEY が入るのはそれらの定義の後なので、空のときは呼ばない
on_signal() {
  local code="$1" name="$2" key="${CUR_KEY}" n="${CUR_NUMBER}" line=''
  trap '' HUP INT TERM
  CUR_KEY=''
  if [[ -z "${key}" ]]; then
    echo "中断: ${name} を受けた（issue の作成中ではない）。同じ spec と状態ファイルで再実行する。" >&2
    exit "${code}"
  fi
  [[ -n "${n}" ]] || n=$(number_from_out) || n=''
  if [[ -n "$(state_num "${key}" 2>/dev/null)" ]]; then
    echo "中断: ${name} を受けた。作成中だった ${key} は状態ファイルに記録済み。同じ spec と状態ファイルで再実行する。" >&2
  elif [[ -n "${n}" ]]; then
    if line=$(state_line "${key}" "${n}" true) && printf '%s\n' "${line}" >>"${STATE}"; then
      echo "中断: ${name} を受けた。作成中だった ${key} → #${n} を状態ファイル ${STATE} へ記録した。同じ spec と状態ファイルで再実行する。" >&2
    else
      {
        echo "中断: ${name} を受けた。${key} → #${n} は作成済みで、状態ファイル ${STATE} へ記録できなかった（このまま再実行すると二重に作る）。"
        echo "復旧: 書けない原因を直し、状態ファイルの末尾に書きかけの行（最後の改行より後ろ）があれば消してから、次の 1 行を末尾に足して同じ spec と状態ファイルで再実行する（題名・本文を含むので、シェルへ埋め込まずファイルの編集で足す）:"
        if [[ -n "${line}" ]]; then printf '%s\n' "${line}"; else echo "  （記録する行を組み立てられなかった。#${n} を閉じてから再実行する）"; fi
      } >&2
    fi
  else
    report_unknown_number "${key}" "中断: ${name} を受けた。${key} の issue 作成の途中で、作られたかは分からない。"
  fi
  exit "${code}"
}
trap cleanup EXIT
trap 'on_signal 129 HUP' HUP
trap 'on_signal 130 INT' INT
trap 'on_signal 143 TERM' TERM
SPEC_SNAP=$(mktemp "${TMPDIR:-/tmp}/cpi.XXXXXX") || die 1 "一時ファイルを作成できません。中止します。"
BODY_FILE=$(mktemp "${TMPDIR:-/tmp}/cpi.XXXXXX") || die 1 "一時ファイルを作成できません。中止します。"
OUT_FILE=$(mktemp "${TMPDIR:-/tmp}/cpi.XXXXXX") || die 1 "一時ファイルを作成できません。中止します。"

# spec は開始時に 1 回だけ読み、以降は複製を使う（実行中にファイルが書き換わっても検証済みの内容で起票する）
jq . "${SPEC}" >"${SPEC_SNAP}" 2>/dev/null || die 1 "spec を JSON として解析できません: ${SPEC}"
[[ "$(jq -s 'length' "${SPEC_SNAP}")" == 1 ]] || die 1 "spec は JSON の値を 1 つだけ持つファイルにしてください。"

# ---- jq の共通定義 ----
# fenceof・stripcode と dd* は implement-issue-tree の scripts/lib/declared-deps-lib.sh（DD_JQ_HEAD・DD_JQ_TAIL）の写し。
# そちらが本文から依存を拾う。implement-issue-tree が同居しない配置でも同じ判定をするため source せずに持つ。
# 写した部分は、名前に dd を付けたことと、表（兄弟のタスク ID）を ddme に固定したこと、依存の見出しの有無（.dep）を
# 記録することだけが違う。ddme はタスク ID の形全般（A-1 等）を 1 つの兄弟とし、前の工程も有るとみなす
# （spec の時点では兄弟の番号も、再利用する Phase 親の既存の子の ID も分からないため、拾われ得る表現を広く拒否する）。
IFS= read -r -d '' JQ_DEFS <<'JQ' || true
def fenceof: (capture("^[ ]{0,3}(?<f>\\x60{3,}|~{3,})") | .f) // null;
def stripcode: . as $s | [match("\\x60+"; "g") | {o: .offset, l: .length}] as $r
| def go($i): if $i >= ($r | length) then []
else (first(range($i + 1; $r | length) | select($r[.].l == $r[$i].l)) // null) as $j
| if $j == null then go($i + 1) else [[$r[$i].o, $r[$j].o + $r[$j].l]] + go($j + 1) end end;
go(0) as $c | reduce ($c | reverse[]) as $x ($s; .[:$x[0]] + ($s[$x[0]:$x[1]] | gsub("[^\n]"; "")) + .[$x[1]:]);
def ddrefs: [scan("#([0-9]+)") | .[0] | tonumber];
def ddrun: "(#[0-9]+(?:(?:[ \t,、/]|and|および|及び)+#[0-9]+)*)";
def ddinline: gsub("(?i)(?:\\b(?:not|no longer|never)\\b|n\\x27t)(?:[ \t]+[A-Za-z]+)?[ \t]+(?:depends on|blocked by)[ \t]*:?[ \t]*" + ddrun; "") | [scan("(?i)(?:depends on|blocked by)[ \t]*:?[ \t]*" + ddrun) | .[0] | ddrefs[]];
def ddneg: test("(?i)関連|参考|参照|任意|なし|不要|\\b(related|see also|optional|none|no longer|not|never|unnecessary)\\b|n\\x27t");
def ddlinehead: [scan("^[ \t]*(?:(?:[-*+]|[0-9]+[.)])[ \t]+)?(?:\\[[ xX]\\][ \t]+)?" + ddrun) | .[0] | ddrefs[]];
def ddidend: "(?:[^A-Za-z0-9_-]|$)";
def ddsibpat($id): "(?:前の?工程|前段)[ \t]*[（(][ \t]*" + $id + "[ \t]*[）),、]"
+ "|(?:^|[^A-Za-z0-9_-])" + $id + "[ \t]*[）)]?[ \t]*(?:の(?:結果|成果物|成果|実装|出力|内容)?を(?:踏まえ|前提|受け)|を(?:踏まえ|前提)|の(?:完了)?後|(?:完了|マージ)後|が(?:完了|マージ)(?:して|した|され))"
+ "|(?i:after|depends on|blocked by|builds on|requires)[ \t]+" + $id + ddidend;
def ddsibhead($id): "^[ \t]*(?:(?:[-*+]|[0-9]+[.)])[ \t]+)?(?:\\[[ xX]\\][ \t]+)?" + $id + ddidend;
def ddprevword: test("前の?工程(?:[ \t]*$|[ \t]*[^ \t（(])");
def ddsibrefs($me; $insec; $prev): . as $t | if ($me.s | length) == 0 or ($t | ddneg) then [] else
[$me.s[] | select(. as $e | $t | test(ddsibpat($e[0]) + (if $insec then "|" + ddsibhead($e[0]) else "" end))) | .[1]] as $hit
| $hit + (if $prev and $me.p > 0 and ($t | ddprevword) then [$me.p] else [] end) end;
def ddextract($t): (if .in and ($t | ddneg | not) then .d += ($t | ddlinehead) else . end) | .d += ($t | ddinline) | .in as $insec | .me as $me | .d += ($t | ddsibrefs($me; $insec; true));
def ddflush: if (.buf | length) == 0 then . else (.buf | join("\n") | stripcode | split("\n")) as $ls | reduce $ls[] as $t (.; ddextract($t)) | .buf = [] end;
def ddme: {s: [["[A-Z][A-Z0-9]{0,19}(?:-[0-9]{1,6}){1,4}", 1]], p: 1};
# 本文 1 つを declared-deps-lib.sh と同じ手順で走査し、閉じていないフェンス・依存の見出しの有無・拾われる依存を返す
def scanbody: split("\n")
| (reduce .[] as $raw ({in: false, fence: null, buf: [], d: [], me: ddme, dep: false};
  ($raw | sub("\r$"; "")) as $l
  | ($l | fenceof) as $f
  | if .fence != null then
    (if $f != null and ($f[0:1] == .fence[0:1]) and (($f | length) >= (.fence | length)) and ($l | test("^[ ]{0,3}[\\x60~]+[ \t]*$")) then .fence = null else . end)
  elif $f != null then ddflush | .fence = $f
  elif ($l | test("^[ ]{0,3}>")) or ($l | test("^[ \t]*$")) then ddflush
  elif ($l | test("^[ ]{0,3}#{1,6}([ \t]|$)")) then ddflush
    | .in = ($l | stripcode | test("^[ ]{0,3}#{1,6}[ \t]*(依存|依存関係|前提|Depends on|Dependencies|Blocked by)[ \t]*:?[ \t]*$"; "i"))
    | .dep = (.dep or .in)
    | .d += ($l | stripcode | ddinline)
    | .me as $me | .d += ($l | stripcode | ddsibrefs($me; false; false))
  elif ($l | test("^[ \t]*(?:[-*+]|[0-9]+[.)])[ \t]")) then ddflush | .buf = [$l]
  else .buf += [$l]
  end) | ddflush)
| {fence, dep, deps: (.d | map(select(. > 0)) | unique)};
def ctrl: explode | any(.[]; . < 32 or . == 127);
def ownerof: if has("owner") then .owner else "" end;
# 配列のキーは省略だけを空配列にする（`// []` は false・null も置き換えてしまう。値の型は spec の検証で配列に限る）
def arr($k): if has($k) then .[$k] else [] end;
def norm: {title, body, owner: ownerof, depends_on: arr("depends_on"), depends_on_issues: arr("depends_on_issues")};
# 要素のパス（"c<i>" / "c<i>.s<j>"。状態ファイルのキーと同じ）を作成順の位置 [i, j] にする（child の j は -1）。
# 作成順は phase → c0 → c0.s0 → c0.s1 → … → c1 → … なので、位置の辞書順の大小がそのまま作成の前後になる。形式が違えば null
def refpos: if type != "string" then null
  else (capture("^c(?<i>0|[1-9][0-9]{0,5})(\\.s(?<j>0|[1-9][0-9]{0,5}))?$") // null)
    | if . == null then null else [(.i | tonumber), (if .j == null then -1 else (.j | tonumber) end)] end end;
def elem($k):
  if $k == "phase" then (.phase | {title, body})
  else ($k | capture("^c(?<i>[0-9]+)(\\.s(?<j>[0-9]+))?$")) as $c
    | if $c.j == null then (.children[$c.i | tonumber] | norm)
      else (.children[$c.i | tonumber].subs[$c.j | tonumber] | norm) end
  end;
# 子（c<i>）の sub の件数。norm は sub を比較に含めないため、状態ファイルへ別に記録して照合する
def nsubs($k): .children[$k[1:] | tonumber] | arr("subs") | length;
JQ

# ---- spec の全件検証（gh を呼ぶ前に、エラーをすべて列挙して止める）----
IFS= read -r -d '' JQ_VALIDATE <<'JQ' || true
def keyerr($p; $allowed): [keys[] | select(. as $k | any($allowed[]; . == $k) | not)
  | "\($p).\(.): 未知のキー" + (if . == "depends_on_children" then "（廃止。依存は depends_on に要素のパス \"c<i>\" / \"c<i>.s<j>\" で書く）" else "" end)];
def chk_title($p):
  if type != "string" then ["\($p).title: 文字列ではない"]
  elif test("\\A\\s*\\z") then ["\($p).title: 空"]
  elif ctrl then ["\($p).title: 改行・制御文字を含む"]
  elif length > 256 then ["\($p).title: 256 文字を超える"]
  else [] end;
def chk_body($p):
  if type != "string" then ["\($p).body: 文字列ではない"]
  elif test("\\A\\s*\\z") then ["\($p).body: 空"]
  elif (explode | any(.[]; . == 0)) then ["\($p).body: NUL を含む"]
  elif length > 60000 then ["\($p).body: 60000 文字を超える"]
  else scanbody as $r
    | (if $r.fence != null then ["\($p).body: コードフェンス（\($r.fence)）が閉じていない"] else [] end)
    + (if $r.dep then ["\($p).body: 依存の見出し（前提・依存・Depends on 等）を本文に書かない。依存は depends_on / depends_on_issues に書く"] else [] end)
    + (if ($r.deps | length) > 0 then ["\($p).body: implement-issue-tree に依存として読まれる表現がある（Depends on #N・blocked by #N・タスク ID〔A-1 等〕の完了後・after A-1・前の工程 等。コード・引用の外）。言い換え、依存は depends_on / depends_on_issues に書く"] else [] end)
  end;
def chk_owner($p):
  ownerof as $o
  | if $o == "owner:joint" then ["\($p).owner: \($joint_msg)"]
    elif ($o | type) != "string" or (any(("", "owner:human"); . == $o) | not)
    then ["\($p).owner: \"\" / \"owner:human\" のどちらかにする"] else [] end;
# depends_on: 要素のパスの配列。$self は自分の位置（refpos と同じ形）、$root は spec 全体。
# 指せるのは作成順で自分より先に作られる要素（同じ親の先行 sub・先行する child とその sub）だけ。sub から自分の親 child は
# 作成順では先だが祖先なので拒否する（implement-issue-tree は祖先への依存を無視するので、書いても完了条件として効かない）
# 参照先の child の sub-issue の数。参照先そのものが不正（オブジェクトでない・subs が配列でない）なら 0 とみなし、
# jq を止めずに「spec に無い要素」として一覧へ出す（参照先の不正は参照先の位置でも別に報告される）
def nsub_safe: if type == "object" and ((.subs // []) | type) == "array" then (.subs // [] | length) else 0 end;
def chk_deps($p; $self; $root):
  if (has("depends_on") | not) then []
  elif (.depends_on | type) != "array" then ["\($p).depends_on: 配列ではない"]
  else [.depends_on[] as $r | ($r | refpos) as $q
    | if $q == null then "\($p).depends_on: \($r | tojson) は要素のパス（\"c<i>\" / \"c<i>.s<j>\"。例 \"c0\"・\"c1.s0\"）ではない（整数の添字は使えない）"
      elif $q[0] >= ($root.children | length) or ($q[1] >= 0 and $q[1] >= ($root.children[$q[0]] | nsub_safe))
      then "\($p).depends_on: \($r | tojson) は spec に無い要素"
      elif $q[0] == $self[0] and $q[1] == -1 and $self[1] >= 0 then "\($p).depends_on: \($r | tojson) は自分の親 child（祖先）は指せない（implement-issue-tree は祖先への依存を無視するので、書いても完了条件として効かない）"
      elif ($q < $self | not) then "\($p).depends_on: \($r | tojson) は自分か、作成順で自分より後に作られる要素（自己参照・前方参照・自分の sub）"
      else empty end]
  end;
# issue 番号の配列。数値は表記でも検査する。jq 1.7 は入力の表記を保つため、`5.0`・`1e1` は `. == floor` を通るのに tostring が
# "5.0"・"1E+1" のままで、本文（- #5.0）に表記ごと出てしまう
# （`1e0` のように整数へ正規化される表記と、表記を保たない jq 1.6 では整数として通り、出力も整数になる）
def chk_issues($p):
  if (has("depends_on_issues") | not) then []
  elif (.depends_on_issues | type) != "array" then ["\($p).depends_on_issues: 配列ではない"]
  else [.depends_on_issues[] | select((type != "number") or ((tostring | test("^[1-9][0-9]*$")) | not) or (. > 9007199254740991))
    | "\($p).depends_on_issues: \(tojson) は正整数の整数表記ではない"] end;
# $self は自分の位置（child は [i, -1]、sub は [i, j]）、$root は spec 全体
def chk_node($p; $self; $child; $root):
  if type != "object" then ["\($p): オブジェクトではない"]
  else keyerr($p; ["title", "body", "owner", "depends_on", "depends_on_issues"] + (if $child then ["subs"] else [] end))
    + (.title | chk_title($p)) + (.body | chk_body($p))
    + chk_owner($p) + chk_deps($p; $self; $root) + chk_issues($p)
    + (if $child and has("subs") then
      (if (.subs | type) != "array" then ["\($p).subs: 配列ではない"]
      else (if (.subs | length) > 0 and ownerof != "" then ["\($p).owner: sub-issue を持つ子には owner を付けない（\"\" にし、sub ごとに付ける）"] else [] end)
        + ([.subs | to_entries[] | .key as $j | .value | chk_node("\($p).subs[\($j)]"; [$self[0], $j]; false; $root)] | add // [])
      end)
    else [] end)
  end;
. as $root
| if type != "object" then ["spec: オブジェクトではない"]
else keyerr("spec"; ["milestone", "phase", "children"])
  + (if has("milestone") then
    (.milestone | if type != "string" then ["milestone: 文字列ではない"]
      elif ctrl then ["milestone: 改行・制御文字を含む"] elif length > 255 then ["milestone: 255 文字を超える"] else [] end)
  else [] end)
  + (if $has_ms == "1" and has("milestone") and .milestone != $ms then ["milestone: --milestone と spec の値が違う"] else [] end)
  + (if $has_ms == "1" then ($ms | if ctrl then ["--milestone: 改行・制御文字を含む"] elif length > 255 then ["--milestone: 255 文字を超える"] else [] end) else [] end)
  + (if has("phase") then
    (.phase | if type != "object" then ["phase: オブジェクトではない"]
      else keyerr("phase"; ["title", "body"]) + (.title | chk_title("phase")) + (.body | chk_body("phase"))
        + (if (.title | type) == "string" and (.title | startswith("feat(phase-" + $phase + "):") | not)
          then ["phase.title: feat(phase-\($phase)): で始める（再実行・棚卸しで Phase 親として見つけるため）"] else [] end)
      end)
  else [] end)
  + (if (.children | type) != "array" or (.children | length) == 0 then ["children: 1 件以上の配列にする"]
  else ([.children | to_entries[] | .key as $i | .value | chk_node("children[\($i)]"; [$i, -1]; true; $root)] | add // []) end)
end
JQ

# owner:joint を拒否するときの案内（spec の検証と、下の値の検査で同じ文を出す）
JOINT_MSG="owner:joint は新規に発行しない。人とエージェントの作業が混ざるタスクは、準備 → 人の作業（owner:human）→ 反映の 3 段の子 issue に分け、depends_on で繋ぐ（SKILL.md Step 1）。既存ツリーに残る owner:joint は implement-issue-tree の skipLabels で待たせる"
ERRORS=$(jq -r --arg phase "${PHASE}" --arg ms "${MILESTONE_ARG}" --arg has_ms "${HAVE_MILESTONE_ARG}" --arg joint_msg "${JOINT_MSG}" \
  "${JQ_DEFS} ${JQ_VALIDATE} | .[]" "${SPEC_SNAP}") \
  || die 1 "spec の検証に失敗しました（jq の異常終了）。gh は呼んでいません。"
if [[ -n "${ERRORS}" ]]; then
  printf '%s\n' "${ERRORS}" | sed 's/^/spec: /' >&2
  die 1 "spec が不正です（上の一覧）。gh は呼んでいません。"
fi

# spec が使う owner ラベルを集め、値を確かめる（spec の検証でも拒否しているが、ラベルの確保は書き込みなので、
# 色を決められない値はロック・gh より前に止める）
OWNER_LIST=()
OWNERS=$(jq -r '[.children[] | (if has("owner") then .owner else "" end), ((if has("subs") then .subs else [] end)[] | (if has("owner") then .owner else "" end))]
  | map(select(. != "")) | unique | .[]' "${SPEC_SNAP}") || die 1 "spec の owner を読めません。gh は呼んでいません。"
while IFS= read -r OWNER; do
  [[ -n "${OWNER}" ]] || continue
  case "${OWNER}" in
    owner:human) OWNER_LIST+=("${OWNER}") ;;
    owner:joint) die 1 "${JOINT_MSG}。gh は呼んでいません。" ;;
    *) die 1 "未知の owner ラベル: ${OWNER}。gh は呼んでいません。" ;;
  esac
done <<<"${OWNERS}"

MILESTONE=$(jq -r --arg ms "${MILESTONE_ARG}" --arg has_ms "${HAVE_MILESTONE_ARG}" \
  'if $has_ms == "1" then $ms else (.milestone // "") end' "${SPEC_SNAP}") \
  || die 1 "milestone を読めません。"
N_CHILDREN=$(jq '.children | length' "${SPEC_SNAP}") || die 1 "children の件数を読めません。"

# ---- ロック（状態ファイルを読む前に取る）----
# 同じ状態ファイルで 2 本が並行に動くと、両方が「未作成」と読んで同じ要素を二重に作る。mkdir は既存なら失敗する
# 原子的な操作なので、状態ファイルの隣のディレクトリをロックにする（macOS に flock は無い）
if mkdir "${LOCK_DIR}" 2>/dev/null; then
  LOCK_HELD=1
  printf 'pid=%s\n' "$$" >"${LOCK_DIR}/owner" 2>/dev/null || true
elif [[ -e "${LOCK_DIR}" || -L "${LOCK_DIR}" ]]; then
  die 1 "ロック ${LOCK_DIR} が既にあります。別の実行中か、異常終了の残り。中身を確かめて消す（owner に書いた pid の実行が動いていなければ、ロックを消してから再実行する）。gh は呼んでいません。"
else
  die 1 "ロック ${LOCK_DIR} を作れません（ディレクトリに書けない等）。gh は呼んでいません。"
fi

# ---- 状態ファイルの照合（食い違いは gh を呼ぶ前に止める。リポジトリの照合は確定後に行う）----
IFS= read -r -d '' JQ_STATE <<'JQ' || true
$sp[0] as $s
| if ($st | length) == 0 then []
  else ($st[0] | if type == "object" and .v == 3 and .root == $root and .phase == $phase and (.repo | type) == "string"
      and (keys == ["milestone", "phase", "repo", "root", "v"]) then []
    else ["ヘッダが今回の --root \($root) / --phase \($phase) と一致しない、または形式（v3）が違う（別のツリー・Phase の状態ファイル、依存を添字で書いていた旧形式）"] end)
    + ($st[0] | if type == "object" and has("milestone") and .milestone != $milestone
      then ["ヘッダの milestone \(.milestone | tojson) が今回の milestone \($milestone | tojson) と違う（作成済みの issue と milestone が混在する）"] else [] end)
    + ([$st[1:][] | select(type != "object" or (.key | type) != "string"
        or ((.key | test("^(phase|c(0|[1-9][0-9]*)(\\.s(0|[1-9][0-9]*))?)$")) | not)
        or ((.number | type) != "number") or ((.number | tostring | test("^[1-9][0-9]*$")) | not)
        or ((.created | type) != "boolean"))
      | "解析できない行: \(tojson)"])
    + ([$st[1:][] | select(type == "object") | .key] | group_by(.) | map(select(length > 1) | "同じキーが複数ある: \(.[0])"))
    + ([$st[1:][] | select(type == "object" and (.key | type) == "string" and (.number | type) == "number")]
      | group_by(.number) | map(select((map(.key) | unique | length) > 1)
        | "別の要素に同じ番号 #\(.[0].number) が記録されている: \(map(.key) | unique | join("・"))（番号の打ち間違い。1 つの issue は 1 つの要素にだけ記録する）"))
    + ([$st[1:][] | select(type == "object" and .number == $root) | "\(.key) の番号 #\(.number) はルートと同じ"])
    + ([$st[1:][] | select(type == "object" and (.key | type) == "string" and (.key | test("^c[0-9]+(\\.s[0-9]+)?$")))
      | . as $e | ($e.key | capture("^c(?<i>[0-9]+)(\\.s(?<j>[0-9]+))?$")) as $c
      | if ($c.i | tonumber) >= ($s.children | length)
        or ($c.j != null and ($c.j | tonumber) >= ($s.children[$c.i | tonumber] | arr("subs") | length))
        then "spec に無い要素の記録: \($e.key) → #\($e.number)（要素数が減った）"
        elif $e.created and ($e.spec != ($s | elem($e.key))) then "作成済みの \($e.key)（#\($e.number)）の内容が spec と違う（並べ替え・書き換え）"
        elif $e.created and $c.j == null and ($e.subs != ($s | nsubs($e.key)))
        then "作成済みの \($e.key)（#\($e.number)）の sub の件数が記録（\($e.subs | tojson)）と spec（\($s | nsubs($e.key))）で違う（作成済みの子に sub を足す・減らすことはできない）"
        else empty end])
    + ([$st[1:][] | select(type == "object" and .key == "phase" and .created == true)
      | if ($s | has("phase") | not) then "Phase 親 #\(.number) を新設した記録があるのに spec に phase が無い"
        elif .spec != ($s | elem("phase")) then "新設した Phase 親 #\(.number) の内容が spec の phase と違う"
        else empty end])
  end
JQ

STATE_EXISTS=0
if [[ -s "${STATE}" ]]; then
  STATE_EXISTS=1
  STATE_ERRORS=$(jq -rn --slurpfile st "${STATE}" --slurpfile sp "${SPEC_SNAP}" \
    --argjson root "${ROOT_NUMBER}" --arg phase "${PHASE}" --arg milestone "${MILESTONE}" \
    "${JQ_DEFS} ${JQ_STATE} | .[]" 2>/dev/null) \
    || die 1 "状態ファイルを解析できません（途中で切れた行がないか確認する）: ${STATE}"
  if [[ -n "${STATE_ERRORS}" ]]; then
    printf '%s\n' "${STATE_ERRORS}" | sed 's/^/状態ファイル: /' >&2
    die 1 "状態ファイルが spec・引数と食い違います（上の一覧）。gh は呼んでいません。"
  fi
fi

# ---- 対象リポジトリの確定（最初の gh 呼び出し）----
# 以降の gh はすべてこの値を明示して呼ぶ。実行の途中や再実行の間に、カレントディレクトリや GH_REPO の解決が
# 変わっても別のリポジトリへ書き込まない。値は gh の応答（非信頼）なので、パスと引数へ入れる前に形式を確かめる
# （owner の _ は Enterprise Managed Users の login〔name_shortcode〕のため許す）
REPO_JSON=$(gh repo view --json nameWithOwner,url) \
  || die 3 "対象リポジトリを決められません（gh repo view が失敗）。書き込みは行っていません。"
REPO=$(printf '%s' "${REPO_JSON}" | jq -r '
  (.nameWithOwner // "") as $s
  | ([.url | strings | capture("^https://(?<h>[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?(:[0-9]{1,5})?)/(?<s>[^/]+/[^/]+)$")] | .[0]) as $c
  | if ($s | type) == "string" and ($s | test("^[A-Za-z0-9]([A-Za-z0-9_-]{0,62}[A-Za-z0-9])?/[A-Za-z0-9._-]{1,100}$"))
      and ($s | test("/\\.{1,2}$") | not) and $c != null and (($c.s | ascii_downcase) == ($s | ascii_downcase))
    then "\($c.h)/\($s)" else "" end' 2>/dev/null) || REPO=''
[[ "${REPO}" =~ ^[A-Za-z0-9.-]+(:[0-9]{1,5})?/[A-Za-z0-9_-]+/[A-Za-z0-9._-]+$ ]] \
  || die 3 "gh repo view の応答から対象リポジトリ（host/owner/repo）を読めません。書き込みは行っていません。"
REPO_HOST="${REPO%%/*}"
REPO_SLUG="${REPO#*/}"
echo "対象リポジトリ: ${REPO}"
if [[ "${STATE_EXISTS}" == 1 ]]; then
  STATE_REPO=$(jq -rs '.[0].repo' "${STATE}") || die 1 "状態ファイルのリポジトリを読めません: ${STATE}"
  # GitHub の owner・repo 名は大文字小文字を区別しない
  jq -en --arg a "${STATE_REPO}" --arg b "${REPO}" '($a | ascii_downcase) == ($b | ascii_downcase)' >/dev/null \
    || die 1 "状態ファイルは ${STATE_REPO} のもので、今回の対象リポジトリ ${REPO} と違います（別の checkout・GH_REPO の変更等）。書き込みは行っていません。状態ファイルのリポジトリで実行し直す。"
fi

# 状態ファイルから key の番号を引く。無ければ空文字
state_num() {
  [[ "${STATE_EXISTS}" == 1 ]] || return 0
  jq -rs --arg k "$1" '[.[1:][] | select(.key == $k) | .number][0] // empty' "${STATE}"
}

# key の記録の行（1 行の JSON）を出す。番号が分からない失敗の案内では number に 0 を入れる
# （状態ファイルの照合が正整数でない番号を拒否するので、置き換え忘れのまま足しても再実行が止まる）
state_line() {
  local key="$1" number="$2" created="$3"
  jq -c --arg k "${key}" --argjson n "${number}" --argjson c "${created}" \
    "${JQ_DEFS} {key: \$k, number: \$n, created: \$c} + (if \$c then {spec: elem(\$k)} else {} end)
      + (if \$c and (\$k | test(\"^c[0-9]+\$\")) then {subs: nsubs(\$k)} else {} end)" \
    "${SPEC_SNAP}"
}

# 作成・採用した issue を状態ファイルへ 1 件追記する。行を先に組み立ててから書く（組み立ての失敗と書き込みの失敗を
# 分け、書き込みに失敗しても足すべき行を出せるように）。作った issue が記録されないまま再実行すると二重に作るので、
# 失敗は exit 3 で止め、状態ファイルの直し方と足す行を stderr に出す
state_add() {
  local key="$1" number="$2" created="$3" line
  line=$(state_line "${key}" "${number}" "${created}") \
    || die 3 "状態ファイルへ記録する ${key} → #${number} の行を組み立てられません（jq の異常終了。状態ファイルは変えていない）。$([[ "${created}" == true ]] && echo "作成済みの #${number} は記録されていないので、閉じてから再実行する（再実行で作り直す）。")"
  if ! printf '%s\n' "${line}" >>"${STATE}"; then
    # ここから先は自分で記録の行を出すので、中断の trap に同じ行をもう一度出させない
    CUR_KEY=''
    if [[ "${created}" == true ]]; then
      {
        echo "エラー: 状態ファイル ${STATE} へ ${key} → #${number} を追記できません。#${number} は作成済みで記録されていない（このまま再実行すると二重に作る）。"
        echo "復旧: 書けない原因（権限・空き容量等）を直し、状態ファイルの末尾に書きかけの行（最後の改行より後ろ）があれば消してから、次の 1 行を末尾に足して同じ spec と状態ファイルで再実行する（題名・本文を含むので、シェルへ埋め込まずファイルの編集で足す）:"
        printf '%s\n' "${line}"
      } >&2
      exit 3
    fi
    die 3 "状態ファイル ${STATE} へ ${key} → #${number}（既存の issue の採用）を追記できません。書けない原因を直し、末尾に書きかけの行（最後の改行より後ろ）があれば消してから再実行する（既存の issue なので作り直されない）。"
  fi
  STATE_EXISTS=1
}

# 番号が分からない失敗（作成の失敗・作成の応答から番号を取り出せない・作成中の中断）の報告。作られていることがあるので、
# 実物を確かめて記録する手順と、記録する行（番号は 0）を stderr に出す。$2 は 1 行目（エラー・中断の内容）
report_unknown_number() {
  local key="$1" why="$2" line
  line=$(state_line "${key}" 0 true) || line=''
  {
    echo "${why}"
    echo "復旧: ${REPO} で ${key} の issue が作られたかを確かめる（直近に作られた issue を一覧し、題名を spec の ${key} と目で比べる。どこにも紐付いていない）。"
    echo "  作られていれば、状態ファイル ${STATE} の末尾に次の 1 行を足し、\"number\":0 をその issue の番号に置き換えてから、同じ spec と状態ファイルで再実行する（題名・本文を含むので、シェルへ埋め込まずファイルの編集で足す）。"
    echo "  作られていなければ、何も足さずに再実行する（0 のまま足すと、再実行が解析できない行として止まる）:"
    if [[ -n "${line}" ]]; then printf '%s\n' "${line}"; else echo "  （記録する行を組み立てられなかった。作られていれば閉じてから再実行する）"; fi
  } >&2
}

die_unknown_number() {
  CUR_KEY=''
  report_unknown_number "$1" "エラー: $2"
  exit 3
}

# gh issue create の出力（OUT_FILE）の最後の issue URL の末尾から番号を取り出す（--json 非対応のため）。無ければ非ゼロ
number_from_out() {
  local n
  [[ -n "${OUT_FILE}" && -s "${OUT_FILE}" ]] || return 1
  n=$(grep -E '/issues/[0-9]+[[:space:]]*$' "${OUT_FILE}" | tail -n 1 | grep -oE '[0-9]+[[:space:]]*$' | tr -d '[:space:]')
  [[ "${n}" =~ ^[1-9][0-9]*$ ]] || return 1
  printf '%s' "${n}"
}

# 対象リポジトリの issue を読む（$1 = 番号）
get_issue() {
  gh api --hostname "${REPO_HOST}" "repos/${REPO_SLUG}/issues/$1"
}

# 指定 issue の sub-issues のうち、対象リポジトリの issue（JSON 配列）を全ページ取得する。失敗時は非ゼロ。
# tree-lib.sh の list_subs と同じ処理で、リポジトリを {owner}/{repo} の解決ではなく確定した値で明示する。
# sub-issue は別リポジトリの issue でもよく、番号はリポジトリごとなので、repository_url の末尾が /repos/<owner>/<repo>
# （大小を区別しない）の要素だけを残す（API の URL は github.com と GHES〔/api/v3〕で前が違うので末尾で比べる）。
# ページの終わりの判定は絞り込む前の件数で行う
list_subs_in_repo() {
  local n="$1" page=1 res all='[]'
  while true; do
    res=$(gh api --hostname "${REPO_HOST}" "repos/${REPO_SLUG}/issues/${n}/sub_issues?per_page=100&page=${page}") || return 1
    # 本文を含む全 JSON を --argjson の引数に載せると ARG_MAX を超え得るため、printf（組み込み）経由の stdin で jq へ渡す
    all=$({ printf '%s' "${all}"; printf '%s' "${res}"; } | jq -s --arg slug "${REPO_SLUG}" '
      .[0] + [.[1][] | select((.repository_url // "") | ascii_downcase | endswith("/repos/" + ($slug | ascii_downcase)))]') || return 1
    [ "$(printf '%s' "${res}" | jq 'length')" -lt 100 ] && break
    page=$((page + 1))
  done
  printf '%s' "${all}"
}

# 指定 issue の sub-issues に number が含まれるか（$1 = sub_issues の JSON、$2 = 番号）
has_sub() {
  printf '%s' "$1" | jq -e --argjson n "$2" 'any(.[]; .number == $n)' >/dev/null
}

# ---- ルートの確認（最初の書き込みより前）----
# SKILL.md Step 2.5 も確かめるが、Step 5 だけを直接（再）実行した場合に備えてここでも確かめる。issues API は PR も返すので
# pull_request の有無で区別する（PR を親にした sub-issue のツリーは implement-issue-tree が辿らない）
ROOT_JSON=$(get_issue "${ROOT_NUMBER}") \
  || die 3 "ルート #${ROOT_NUMBER} を読めません。書き込みは行っていません。"
ROOT_KIND=$(printf '%s' "${ROOT_JSON}" | jq -r 'if type != "object" then "" elif has("pull_request") then "pr" else (.state // "" | strings) end' 2>/dev/null) \
  || ROOT_KIND=''
case "${ROOT_KIND}" in
  open) ;;
  pr) die 1 "ルート #${ROOT_NUMBER} は PR です（issue ではない）。書き込みは行っていません。--root にトラッキング issue の番号を渡す。" ;;
  '') die 3 "ルート #${ROOT_NUMBER} の状態を読めません。書き込みは行っていません。" ;;
  *) die 1 "ルート #${ROOT_NUMBER} が open ではありません（${ROOT_KIND}）。書き込みは行っていません。ルートを開き直すか、open なルートで起票し直す。" ;;
esac

# ---- 読み取り: ルート直下と Phase 親の決定 ----
ROOT_SUBS=$(list_subs_in_repo "${ROOT_NUMBER}") \
  || die 3 "ルート #${ROOT_NUMBER} の sub-issues を取得できません。書き込みは行っていません。"

PHASE_NUMBER=$(state_num phase)
PHASE_CREATED=0
if [[ -n "${PHASE_NUMBER}" ]]; then
  [[ -z "${PHASE_NUMBER_ARG}" || "${PHASE_NUMBER_ARG}" == "${PHASE_NUMBER}" ]] \
    || die 1 "--phase-number ${PHASE_NUMBER_ARG} が状態ファイルの Phase 親 #${PHASE_NUMBER} と違います。"
  PHASE_CREATED=$(jq -rs '[.[1:][] | select(.key == "phase") | .created][0] | if . then 1 else 0 end' "${STATE}") \
    || die 1 "状態ファイルの Phase 親の記録を読めません。"
  # 記録は前回の実行時点の判断。その後に閉じられた・ルートから外された親の下へ起票しないよう、今回の読み取りで確かめ直す。
  # ルート直下にあれば open であること。無ければ、再利用した既存の親は外されたので不可、新設した親（ルートへの紐付けは
  # 最後なので未紐付けがあり得る）は実体を読んで open であること
  ROOT_STATE=$(printf '%s' "${ROOT_SUBS}" | jq -r --argjson n "${PHASE_NUMBER}" '[.[] | select(.number == $n) | .state][0] // ""') \
    || die 3 "ルート直下の Phase 親 #${PHASE_NUMBER} を判定できません。書き込みは行っていません。"
  if [[ -n "${ROOT_STATE}" ]]; then
    [[ "${ROOT_STATE}" == open ]] \
      || die 1 "状態ファイルの Phase 親 #${PHASE_NUMBER} はルート #${ROOT_NUMBER} 直下にあるが open ではありません（${ROOT_STATE}）。書き込みは行っていません。親を開き直すか、新しい状態ファイルで起票し直す。"
  elif [[ "${PHASE_CREATED}" == 0 ]]; then
    die 1 "状態ファイルの Phase 親 #${PHASE_NUMBER}（再利用した既存の親）がルート #${ROOT_NUMBER} の直下にありません（外された）。書き込みは行っていません。"
  else
    P_JSON=$(get_issue "${PHASE_NUMBER}") \
      || die 3 "状態ファイルの Phase 親 #${PHASE_NUMBER} を読めません。書き込みは行っていません。"
    P_STATE=$(printf '%s' "${P_JSON}" | jq -r '.state // ""') \
      || die 3 "Phase 親 #${PHASE_NUMBER} の状態を読めません。書き込みは行っていません。"
    [[ "${P_STATE}" == open ]] \
      || die 1 "状態ファイルの Phase 親 #${PHASE_NUMBER}（この起票で新設した親）が open ではありません（${P_STATE:-不明}）。書き込みは行っていません。"
  fi
  echo "状態ファイルの Phase 親を使う: #${PHASE_NUMBER}"
elif [[ -n "${PHASE_NUMBER_ARG}" ]]; then
  printf '%s' "${ROOT_SUBS}" | jq -e --argjson n "${PHASE_NUMBER_ARG}" \
    'any(.[]; .number == $n and .state == "open")' >/dev/null \
    || die 1 "--phase-number #${PHASE_NUMBER_ARG} はルート #${ROOT_NUMBER} 直下の open な issue ではありません。"
  PHASE_NUMBER="${PHASE_NUMBER_ARG}"
  echo "指定された Phase 親を再利用: #${PHASE_NUMBER}"
else
  # closed な親は再利用しない（closed 親の下に open issue を残置しない運用）。phase ラベルだけでは同ラベルの
  # 一般 issue に誤マッチするため、Phase 親の題名規約（feat(phase-N): 接頭辞）でも絞り込む
  PHASE_NUMBER=$(printf '%s' "${ROOT_SUBS}" | jq -r --arg p "${PHASE}" \
    '[.[] | select(.state == "open" and ((.title // "") | startswith("feat(phase-" + $p + "):"))
      and any(.labels[]?; (.name // "" | ascii_downcase) == "phase:" + $p))][0].number // empty') \
    || die 3 "ルート直下の Phase 親を判定できません。書き込みは行っていません。"
  if [[ -n "${PHASE_NUMBER}" ]]; then
    echo "既存の Phase 親を再利用: #${PHASE_NUMBER}"
  else
    [[ "$(jq 'has("phase")' "${SPEC_SNAP}")" == true ]] \
      || die 1 "ルート直下に Phase ${PHASE} の open な親が無く、新設に使う spec の phase もありません。gh への書き込みは行っていません。"
    PHASE_CREATED=1
  fi
fi
if [[ -n "${PHASE_NUMBER}" && "${PHASE_CREATED}" == 0 ]] && [[ "$(jq 'has("phase")' "${SPEC_SNAP}")" == true ]]; then
  echo "Phase 親を再利用するので spec の phase は使わない。"
fi
# 再利用する Phase 親の現在の milestone を書き込みの前に読む（付いていない場合だけ後で設定し、付いていれば変えない）
SET_PHASE_MILESTONE=0
if [[ -n "${PHASE_NUMBER}" && "${PHASE_CREATED}" == 0 && -n "${MILESTONE}" ]]; then
  PHASE_JSON=$(get_issue "${PHASE_NUMBER}") \
    || die 3 "Phase 親 #${PHASE_NUMBER} を読めません。書き込みは行っていません。"
  CUR_MS=$(printf '%s' "${PHASE_JSON}" | jq -r '.milestone.title // ""') \
    || die 3 "Phase 親 #${PHASE_NUMBER} の milestone を読めません。書き込みは行っていません。"
  if [[ -z "${CUR_MS}" ]]; then
    SET_PHASE_MILESTONE=1
  elif [[ "${CUR_MS}" != "${MILESTONE}" ]]; then
    echo "注意: Phase 親 #${PHASE_NUMBER} には別の milestone が付いているので変えない（子・sub-issue には spec の milestone を付ける）。"
  fi
fi

# ---- 状態ファイルの番号の実物との照合（最初の書き込みより前）----
# 状態ファイルの番号は人が足した行（"number":0 の置き換え）のことがあり、打ち間違えると無関係な issue をツリーへ紐付け、
# 依存の番号にも使ってしまう。spec の要素として作った記録（c<i>・c<i>.s<j>・新設した Phase 親）は、実物が対象リポジトリの
# open な issue（PR ではない）で、題名が spec と同じ（前後の空白は除いて比べる。GitHub が題名の前後の空白を落とすため）で
# あることを確かめる。再利用した既存の Phase 親は題名が spec に無いので、上の Phase 親の確認（ルート直下の open）に任せる
if [[ "${STATE_EXISTS}" == 1 ]]; then
  RECORDS=$(jq -rs '.[1:][] | select(.key != "phase" or .created == true) | "\(.key) \(.number)"' "${STATE}") \
    || die 1 "状態ファイルの記録を読めません: ${STATE}"
  # ループの本体は here-string を stdin として継ぐので、gh には </dev/null を渡す（stdin を読む版の gh が残りの行を
  # 食べると、照合が途中で黙って終わる）。照合した件数を記録の行数と突き合わせ、途中で抜けたら止める
  R_EXPECT=$(awk 'NF { n++ } END { print n + 0 }' <<<"${RECORDS}") || die 1 "状態ファイルの記録を数えられません: ${STATE}"
  R_DONE=0
  while read -r R_KEY R_NUM; do
    [[ -n "${R_KEY}" ]] || continue
    R_JSON=$(get_issue "${R_NUM}" </dev/null) \
      || die 3 "状態ファイルの ${R_KEY} → #${R_NUM} を読めません。書き込みは行っていません。"
    R_WHY=$({ printf '%s' "${R_JSON}"; } | jq -r --arg k "${R_KEY}" --arg slug "${REPO_SLUG}" --slurpfile sp "${SPEC_SNAP}" "${JQ_DEFS}"'
      def trim: sub("^[ \t\r\n]+"; "") | sub("[ \t\r\n]+$"; "");
      ($sp[0] | elem($k).title) as $want
      | if type != "object" then "issue として読めない"
        elif has("pull_request") then "PR である"
        elif ((.repository_url // "") | ascii_downcase | endswith("/repos/" + ($slug | ascii_downcase)) | not)
        then "対象リポジトリの issue ではない（\(.repository_url // "repository_url が無い")）"
        elif .state != "open" then "open ではない（\(.state // "不明")）"
        elif ((.title // "") | trim) != ($want | trim) then "題名が spec と違う（実物: \((.title // "") | tojson)・spec: \($want | tojson)）"
        else "" end' 2>/dev/null) \
      || die 3 "状態ファイルの ${R_KEY} → #${R_NUM} の内容を読めません。書き込みは行っていません。"
    [[ -z "${R_WHY}" ]] \
      || die 1 "状態ファイルの ${R_KEY} → #${R_NUM} が実物と合いません: ${R_WHY}。書き込みは行っていません。番号の打ち間違いなら状態ファイルの行を正しい番号に直し、作られていなかったならその行を消して再実行する。"
    R_DONE=$((R_DONE + 1))
  done <<<"${RECORDS}"
  [[ "${R_DONE}" == "${R_EXPECT}" ]] \
    || die 3 "状態ファイルの番号の照合が途中で終わりました（${R_DONE}/${R_EXPECT} 件）。書き込みは行っていません。"
fi

# milestone を付ける場合は、open な milestone に同じ題名があることを書き込みの前に確かめる（無ければ issue の作成が
# 途中で失敗し、作成済みの要素だけが残る。closed な milestone には付けない）
if [[ -n "${MILESTONE}" ]]; then
  MS_FOUND=0
  MS_PAGE=1
  while true; do
    MS_RES=$(gh api --hostname "${REPO_HOST}" "repos/${REPO_SLUG}/milestones?state=open&per_page=100&page=${MS_PAGE}") \
      || die 3 "milestone の一覧を取得できません。書き込みは行っていません。"
    MS_HIT=$(printf '%s' "${MS_RES}" | jq -r --arg m "${MILESTONE}" 'if type != "array" then error("not array") else (any(.[]; .title == $m) | tostring) + " " + (length | tostring) end') \
      || die 3 "milestone の一覧を読めません。書き込みは行っていません。"
    if [[ "${MS_HIT%% *}" == true ]]; then MS_FOUND=1; break; fi
    [[ "${MS_HIT#* }" -lt 100 ]] && break
    MS_PAGE=$((MS_PAGE + 1))
  done
  [[ "${MS_FOUND}" == 1 ]] \
    || die 1 "milestone「${MILESTONE}」が ${REPO} の open な milestone にありません（無い・closed）。書き込みは行っていません。milestone を作る・開き直すか、spec・--milestone の値を直す。"
fi

# ---- depends_on_issues の参照先の確認（最初の書き込みより前）----
# spec の検証は番号の形しか見ないので、PR の番号を書いても起票・紐付けは通り、`## 前提` に PR が載る（SKILL.md Step 7 の
# 人の作業の表は前提が PR だと止まり、表全体が出なくなる）。参照先を 1 件ずつ読み、対象リポジトリの issue（PR ではない）で
# あることを確かめる。state（open・closed）は問わない（closed の issue を前提にするのは正当）。
# 確かめるのは状態ファイルに記録の無い（この実行で作る）要素の分だけ。本文の `## 前提` を書くのは作成時だけなので、
# どの要素もそれを作る実行で先に確かめられている。作成済みの要素の分は確かめ直さない（後から参照先が読めなくなっても、
# 作成済みの要素の spec は状態ファイルの照合で変えられないため、確かめ直すと再開が進めなくなる）。
# 番号は重複を除いて 1 回ずつ読み、止めるときはその番号を書いた spec の要素のパスをすべて出す（1 つの要素に同じ番号を
# 2 回書いてもパスは 1 回。要素の中で重複を落としてから group_by〔安定〕で束ねるので、パスは spec の順に並ぶ）。
# 祖先の番号は、読む前に spec の誤りとして止める（implement-issue-tree は祖先〔ルート・Phase 親・親 child〕への依存を無視するので、
# 前提に書いても完了条件として効かない）。比べるのは番号の決まっている祖先だけ: ルート、確定済みの Phase 親（状態ファイル・
# --phase-number・探索で決まった番号）、未作成の sub に対しては状態ファイルに作成済みと記録された親 child。Phase 親を新設する
# 初回の実行の Phase 親と、新規実行（親 child がまだ作られていない）の親 child は番号が無いので比べない
if [[ "${STATE_EXISTS}" == 1 ]]; then
  DONE_KEYS=$(jq -cs '[.[1:][] | .key]' "${STATE}") || die 1 "状態ファイルの記録を読めません: ${STATE}"
  DONE_NUMS=$(jq -cs '[.[1:][] | {(.key): .number}] | add // {}' "${STATE}") || die 1 "状態ファイルの記録を読めません: ${STATE}"
else
  DONE_KEYS='[]'
  DONE_NUMS='{}'
fi
DEP_REFS=$(jq -r --argjson created "${DONE_KEYS}" '[.children | to_entries[] | .key as $i | .value
    | ({k: "c\($i)", p: "children[\($i)]", n: (if has("depends_on_issues") then .depends_on_issues else [] end)},
      ((if has("subs") then .subs else [] end) | to_entries[]
        | {k: "c\($i).s\(.key)", p: "children[\($i)].subs[\(.key)]", n: (.value | if has("depends_on_issues") then .depends_on_issues else [] end)}))
    | select(.k as $k | any($created[]; . == $k) | not)
    | .p as $p | (.n | unique)[] | {n: ., p: $p}]
  | group_by(.n)[] | "\(.[0].n) \(map("\(.p).depends_on_issues") | join("・"))"' "${SPEC_SNAP}") \
  || die 3 "spec の depends_on_issues を読めません（jq の異常終了）。書き込みは行っていません。"
ANCESTOR_WHY="ルート・Phase 親・親 child（祖先）は子孫の前提にできない（implement-issue-tree は祖先〔ルート・Phase 親・親 child〕への依存を無視するので、前提に書いても完了条件として効かない）。depends_on_issues から外す。"
# 未作成の sub の親 child が作成済み（状態ファイルに記録がある）なら、その番号も祖先として読む前に止める。番号で束ねた
# DEP_REFS では親 child かどうかが要素ごとに違う（別の child の sub・child 自身には祖先でない）ので、要素ごとに照合し、
# 違反した sub のパスだけを親 child の番号ごとに束ねて出す（パスは spec の順）。親 child が未作成なら番号が無いので比べない
D_PARENT=$(jq -r --argjson created "${DONE_KEYS}" --argjson m "${DONE_NUMS}" '
  [.children | to_entries[] | .key as $i | $m["c\($i)"] as $pn | select($pn != null)
    | (if .value | has("subs") then .value.subs else [] end) | to_entries[]
    | "c\($i).s\(.key)" as $k | select(any($created[]; . == $k) | not)
    | select(.value | (if has("depends_on_issues") then .depends_on_issues else [] end) | any(.[]; . == $pn))
    | {n: $pn, c: "c\($i)", p: "children[\($i)].subs[\(.key)].depends_on_issues"}]
  | group_by(.n)[] | "\(.[0].n) \(.[0].c) \(map(.p) | join("・"))"' "${SPEC_SNAP}") \
  || die 3 "spec の depends_on_issues と状態ファイルの親 child の番号を照合できません（jq の異常終了）。書き込みは行っていません。"
if [[ -n "${D_PARENT}" ]]; then
  read -r D_NUM D_KEY D_PATHS <<<"${D_PARENT%%$'\n'*}"
  die 1 "spec: ${D_PATHS}: #${D_NUM} は親 child（${D_KEY}）です。書き込みは行っていません。${ANCESTOR_WHY}"
fi
# ループの本体は here-string を stdin として継ぐので、gh には </dev/null を渡す。確かめた件数を番号の行数と突き合わせ、
# 途中で抜けたら（残りの番号を確かめないまま書き込みへ進まないよう）止める
D_EXPECT=$(awk 'NF { n++ } END { print n + 0 }' <<<"${DEP_REFS}") \
  || die 3 "spec の depends_on_issues の番号を数えられません。書き込みは行っていません。"
D_DONE=0
while read -r D_NUM D_PATHS; do
  [[ -n "${D_NUM}" ]] || continue
  [[ "${D_NUM}" =~ ^[1-9][0-9]*$ ]] \
    || die 3 "spec: ${D_PATHS}: 番号 ${D_NUM} を読めません。書き込みは行っていません。"
  [[ "${D_NUM}" != "${ROOT_NUMBER}" ]] \
    || die 1 "spec: ${D_PATHS}: #${D_NUM} はルートです。書き込みは行っていません。${ANCESTOR_WHY}"
  [[ -z "${PHASE_NUMBER}" || "${D_NUM}" != "${PHASE_NUMBER}" ]] \
    || die 1 "spec: ${D_PATHS}: #${D_NUM} はこの Phase の親です。書き込みは行っていません。${ANCESTOR_WHY}"
  D_JSON=$(get_issue "${D_NUM}" </dev/null) \
    || die 3 "spec: ${D_PATHS}: #${D_NUM} を ${REPO} の issue として読めません（存在しない番号・権限・通信の失敗）。書き込みは行っていません。番号の誤りなら spec を直して再実行する。"
  # 結果は「ok」か「<種別> <理由>」。種別 pr・foreign は spec の誤り（exit 1）、shape は応答を判定できない（exit 3）。
  # 判定は PR → 別リポジトリ → 番号の食い違いの順（別リポジトリへ移管された issue は、gh が移管先へのリダイレクトを辿って
  # 別の番号の応答を返すので、番号より先にリポジトリを見て spec の誤りとして扱う）。repository_url の無い・文字列でない
  # 応答は、状態ファイルの番号の照合と同じく対象リポジトリの issue と確かめられないとして foreign にする
  D_WHY=$({ printf '%s' "${D_JSON}"; } | jq -r --argjson n "${D_NUM}" --arg slug "${REPO_SLUG}" '
    if type != "object" then "shape issue として読めない"
    elif has("pull_request") then "pr PR である（issue ではない）"
    elif (.repository_url | type) != "string" or (.repository_url | ascii_downcase | endswith("/repos/" + ($slug | ascii_downcase)) | not)
    then "foreign 対象リポジトリの issue ではない（\(.repository_url // "repository_url が無い" | tostring)）"
    elif (.number | type) != "number" or .number != $n then "shape 応答の番号が違う（\(.number | tojson)）"
    else "ok" end' 2>/dev/null) \
    || die 3 "spec: ${D_PATHS}: #${D_NUM} の応答を読めません（JSON でない等）。書き込みは行っていません。"
  case "${D_WHY}" in
    ok) ;;
    "pr "* | "foreign "*)
      die 1 "spec: ${D_PATHS}: #${D_NUM} は ${D_WHY#* }。書き込みは行っていません。depends_on_issues には ${REPO} の issue の番号を書く（spec の要素なら depends_on にパスで書く）。" ;;
    "shape "*)
      die 3 "spec: ${D_PATHS}: #${D_NUM} の応答を判定できません（${D_WHY#* }）。書き込みは行っていません。" ;;
    *) die 3 "spec: ${D_PATHS}: #${D_NUM} の判定結果を読めません。書き込みは行っていません。" ;;
  esac
  D_DONE=$((D_DONE + 1))
done <<<"${DEP_REFS}"
[[ "${D_DONE}" == "${D_EXPECT}" ]] \
  || die 3 "depends_on_issues の参照先の確認が途中で終わりました（${D_DONE}/${D_EXPECT} 件）。書き込みは行っていません。"

# ---- 書き込みの開始: 状態ファイルのヘッダ・ラベル ----
if [[ "${STATE_EXISTS}" == 0 ]]; then
  jq -cn --argjson root "${ROOT_NUMBER}" --arg phase "${PHASE}" --arg milestone "${MILESTONE}" --arg repo "${REPO}" \
    '{v: 3, root: $root, phase: $phase, milestone: $milestone, repo: $repo}' >"${STATE}" \
    || die 1 "状態ファイルを作れません: ${STATE}"
  STATE_EXISTS=1
fi
if [[ -n "${PHASE_NUMBER}" && -z "$(state_num phase)" ]]; then
  state_add phase "${PHASE_NUMBER}" false
fi

# ラベルを確保する。作成に失敗したら（既にある場合を含む）実在を読んで確かめ、無ければ失敗にする
ensure_label() {
  local name="$1" color="$2" err enc res
  if err=$(gh label create "${name}" --color "${color}" --repo "${REPO}" 2>&1 >/dev/null); then
    echo "ラベルを作成: ${name}"
    return 0
  fi
  enc=$(jq -rn --arg s "${name}" '$s | @uri') || return 1
  res=$(gh api --hostname "${REPO_HOST}" "repos/${REPO_SLUG}/labels/${enc}" 2>/dev/null) || {
    printf '%s\n' "${err}" >&2
    return 1
  }
  printf '%s' "${res}" | jq -e --arg s "${name}" '(.name // "" | ascii_downcase) == ($s | ascii_downcase)' >/dev/null || {
    printf '%s\n' "${err}" >&2
    return 1
  }
}

ensure_label "phase:${PHASE}" "0075ca" \
  || die 3 "ラベル phase:${PHASE} を確保できません。issue は作っていません。"
# OWNER_LIST は spec の検証の直後に値を確かめてある（owner:human だけ）。ここの未知の値の拒否は、
# 上の検査が崩れたときの二重の防御。owner:joint は新規に発行しないので、ここに色を持たない（#905）。
# bash 3.2 の set -u は空の配列の "${a[@]}" を未定義として止めるので、${a[@]+...} で展開する
for OWNER in ${OWNER_LIST[@]+"${OWNER_LIST[@]}"}; do
  case "${OWNER}" in
    owner:human) COLOR="d93f0b" ;;
    *) die 1 "未知の owner ラベル: ${OWNER}" ;;
  esac
  # owner ラベル無しで作ると人の作業が自動実装に回るため、確保できなければ issue を作らずに止める
  ensure_label "${OWNER}" "${COLOR}" \
    || die 3 "ラベル ${OWNER} を確保できません。issue は作っていません。"
done

# 再利用する Phase 親が milestone 未設定の場合はここで揃える（新規の子だけに milestone が付く不整合を防ぐ）。
# 既に付いている milestone は上書きしない（人が付け替えた値を消さない）
if [[ "${SET_PHASE_MILESTONE}" == 1 ]]; then
  gh issue edit "${PHASE_NUMBER}" --milestone "${MILESTONE}" --repo "${REPO}" >/dev/null \
    || die 3 "Phase 親 #${PHASE_NUMBER} に milestone を設定できません。"
fi

# ---- 起票 ----
# key の issue を作る（状態ファイルに記録があれば再利用する）。結果の番号は NODE_NUMBER に入れる
# （コマンド置換で呼ぶと die がサブシェルで終わるため、戻り値は変数で返す）
NODE_NUMBER=''
create_node() {
  local key="$1" owner="$2" title deps rc out
  NODE_NUMBER=$(state_num "${key}") || die 3 "状態ファイルを読めません。"
  if [[ -n "${NODE_NUMBER}" ]]; then
    echo "作成済みを再利用: ${key} → #${NODE_NUMBER}"
    return 0
  fi
  title=$(jq -r --arg k "${key}" "${JQ_DEFS} elem(\$k).title" "${SPEC_SNAP}") || die 3 "${key} の題名を読めません。"
  # 依存の番号は状態ファイルに記録した作成済みの番号と depends_on_issues だけから組み立てる。depends_on のパスは
  # 状態ファイルのキーと同じ形で、検証で作成順の先行要素に限ってあるので、再開時も前回の記録から引ける
  deps=$(jq -c --arg k "${key}" --slurpfile st "${STATE}" "${JQ_DEFS}"'
    ([$st[1:][] | {(.key): .number}] | add // {}) as $m
    | elem($k) as $e
    | if $k == "phase" then [] else [$e.depends_on[] | $m[.]] + $e.depends_on_issues end
    | if any(.[]; type != "number") then error("依存先の番号が状態ファイルに無い") else unique end' "${SPEC_SNAP}") \
    || die 3 "${key} の依存先の番号を組み立てられません。"
  jq -j --arg k "${key}" --argjson deps "${deps}" "${JQ_DEFS}"'
    elem($k).body as $b
    | $b + (if ($deps | length) == 0 then ""
      else (if ($b | endswith("\n")) then "\n" else "\n\n" end) + "## 前提\n\n" + ($deps | map("- #\(.)") | join("\n")) + "\n" end)' \
    "${SPEC_SNAP}" >"${BODY_FILE}" || die 3 "${key} の本文を書き出せません。"

  local -a args=(--repo "${REPO}" --title "${title}" --body-file "${BODY_FILE}" --label "phase:${PHASE}")
  [[ -n "${owner}" ]] && args+=(--label "${owner}")
  [[ -n "${MILESTONE}" ]] && args+=(--milestone "${MILESTONE}")
  # 出力はコマンド置換ではなくファイルへ受ける。bash は前景の子（gh）の終了を待ってから trap を動かすので、作成中に
  # HUP・INT・TERM を受けても、on_signal が gh の出した URL から番号を取り出して記録できる
  : >"${OUT_FILE}" || die 3 "一時ファイルを空にできません。issue は作っていません。"
  CUR_NUMBER=''
  CUR_KEY="${key}"
  rc=0
  gh issue create "${args[@]}" >"${OUT_FILE}" || rc=$?
  out=$(cat "${OUT_FILE}") || out=''
  if [[ "${rc}" != 0 ]]; then
    printf 'gh の出力: %s\n' "${out}" >&2
    die_unknown_number "${key}" "${key} の issue 作成が失敗しました（exit ${rc}）。失敗の応答でも作られていることがある。"
  fi
  # gh issue create は --json 非対応。最後の issue URL の末尾から番号を取る
  NODE_NUMBER=$(number_from_out) || NODE_NUMBER=''
  if [[ -z "${NODE_NUMBER}" ]]; then
    printf 'gh の出力: %s\n' "${out}" >&2
    die_unknown_number "${key}" "${key} の issue 番号を作成の応答から取り出せません。"
  fi
  CUR_NUMBER="${NODE_NUMBER}"
  state_add "${key}" "${NODE_NUMBER}" true
  CUR_KEY=''
  CUR_NUMBER=''
  echo "作成: ${key} → #${NODE_NUMBER}"
}

# child を parent の sub-issue にする。紐付け済みなら POST しない。POST 失敗後も読み直して紐付いていれば成功とする
ensure_linked() {
  local parent="$1" child="$2" subs res id
  subs=$(list_subs_in_repo "${parent}") || die 3 "#${parent} の sub-issues を取得できません。"
  if has_sub "${subs}" "${child}"; then
    echo "紐付け済み: #${child} → #${parent}"
    return 0
  fi
  # sub_issue_id は issue 番号ではなく database id（GitHub sub-issues API の仕様）
  res=$(get_issue "${child}") || die 3 "#${child} の id を取得できません。"
  id=$(printf '%s' "${res}" | jq -r '.id // empty') || die 3 "#${child} の id を読めません。"
  [[ "${id}" =~ ^[1-9][0-9]*$ ]] || die 3 "#${child} の id が正整数ではありません。"
  if gh api --hostname "${REPO_HOST}" --method POST "repos/${REPO_SLUG}/issues/${parent}/sub_issues" -F "sub_issue_id=${id}" >/dev/null; then
    echo "紐付け: #${child} → #${parent}"
    return 0
  fi
  subs=$(list_subs_in_repo "${parent}") || die 3 "#${parent} の sub-issues を取得できません（紐付けの確認）。"
  if has_sub "${subs}" "${child}"; then
    echo "紐付け: #${child} → #${parent}（POST は失敗したが紐付いていることを確認）"
    return 0
  fi
  die 3 "#${child} を #${parent} へ紐付けられません。"
}

if [[ "${PHASE_CREATED}" == 1 && -z "${PHASE_NUMBER}" ]]; then
  create_node phase ""
  PHASE_NUMBER="${NODE_NUMBER}"
fi

for ((i = 0; i < N_CHILDREN; i++)); do
  OWNER=$(jq -r --argjson i "${i}" '.children[$i] | if has("owner") then .owner else "" end' "${SPEC_SNAP}") \
    || die 3 "children[${i}] の owner を読めません。"
  create_node "c${i}" "${OWNER}"
  CHILD_NUMBER="${NODE_NUMBER}"
  N_SUBS=$(jq --argjson i "${i}" '.children[$i] | (if has("subs") then .subs else [] end) | length' "${SPEC_SNAP}") \
    || die 3 "children[${i}] の sub 件数を読めません。"
  for ((j = 0; j < N_SUBS; j++)); do
    OWNER=$(jq -r --argjson i "${i}" --argjson j "${j}" '.children[$i].subs[$j] | if has("owner") then .owner else "" end' "${SPEC_SNAP}") \
      || die 3 "children[${i}].subs[${j}] の owner を読めません。"
    create_node "c${i}.s${j}" "${OWNER}"
    ensure_linked "${CHILD_NUMBER}" "${NODE_NUMBER}"
  done
  # child はその sub がすべて紐付いてから Phase 親へ紐付ける（途中で止まった child をツリーに出さない）
  ensure_linked "${PHASE_NUMBER}" "${CHILD_NUMBER}"
done

# 新設した Phase 親は全 child が済んでから最後にルートへ紐付ける（子の無い Phase 親を実装候補にしない）
ensure_linked "${ROOT_NUMBER}" "${PHASE_NUMBER}"

echo "result=created root=${ROOT_NUMBER} phase=${PHASE} phase_number=${PHASE_NUMBER} children=${N_CHILDREN}"
