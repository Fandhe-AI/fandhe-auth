#!/usr/bin/env bash
# 秘密情報の混入検査（TASK-6。規則の定義点はこのファイル 1 か所）。
#
# 呼び出し:
#   secret-scan.sh            staged 差分の追加行を走査する（lefthook pre-commit から呼ばれる）
#   secret-scan.sh --tracked  追跡済みファイル全体を走査する（`make secret-scan`）
#   secret-scan.sh --tracked --summary
#                             上記と同じ走査で、出力から該当ファイル名を除く（規則 ID・件数だけ。公開ログ用。
#                             パスに値が含まれ得る・改行を含むパスが出力行を偽装し得るため。SECR-23）。
#                             CI は scripts/secret-scan-ci.sh 経由でこの形だけを使う
#   それ以外の引数は usage を出して終了コード 2。検査を外す引数・環境変数は置かない。
#   本文の走査の除外は下の「除外」の 1 種類だけで、引数・環境変数で足す手段は無い。
#
# 規則表（規則 ID は暫定。確定は人が行う）:
#   env-file         .env 系ファイル（.env.example / .sample / .template は雛形として除外）
#   vendor-token     ベンダー固有トークン形式（sk-・ghp_・github_pat_・AKIA・xox?-・AIza）
#   private-key      秘密鍵ヘッダー（RSA・EC・DSA・OPENSSH・PGP・ENCRYPTED を含む）
#   jwt              JWT の形の文字列（ヘッダ・ペイロードの 2 区画が eyJ で始まる）
#   jwk-private      JWK の秘密成分 "d" に base64url の長い値を入れた形
#   credential-url   認証情報入り接続文字列（scheme の後に user:pass 形式の認証情報が付く形）
#   assignment       api key・secret・token・password・passphrase・recovery code の語への、
#                    引用符付きの長い値（base64・base64url・16 進を含む 16 文字以上）の代入
# 復旧コードの値の形式は spec で未定（SECR-4 は秘密部 128 ビットのみ）。値の形では検出せず、語と代入の形で検出する。
# 形式が決まったら規則を足す。
#
# 出力の契約（SECR-23・値を出さず件数で判定する）: 規則 ID ごとの件数・合計件数・該当ファイル名だけを出す。
# 一致した行・値・値の一部・長さは出さない。0 件なら終了コード 0、1 件以上なら 1。
#
# 走査範囲: 追加行のみ（削除行は漏洩済み値の除去コミットを妨げないため対象外）。例外として、登録を外した名前の
# `.agents/skills/<名前>/` は staged でも固定した index の全行を走査する（空の tree との差分。#6054）。
# 本文は `git diff --text` で受けるため、.gitattributes の binary・-diff 属性や NUL を含むファイルも走査する。
# NUL は取り除いた版と空白へ置き換えた版の両方で照合する。照合できるのは上の規則表の形式（行内の文字列）だけで、
# 圧縮・エンコード（base64 等）・複数行に分割された値・規則に無い形の値は検出できない。
# 除外（本文の走査だけに効く。env-file の名前の検査は除外より前に全パスへ掛かる。fail-closed）:
#   このスクリプトのある repo の skills-lock.json（<このファイル>/../../skills-lock.json）に登録された名前の
#   `.agents/skills/<名前>/` の下だけを本文の走査から外す（外部取得物の参考文書）。
#   除外に使う名前は検出器の側の一覧から読み、走査対象（staged・追跡ファイル・PR の head）の skills-lock.json から
#   名前を足さない。走査対象の index の一覧は「除外を縮める」ためだけに使い、検出器の一覧と対象の index の一覧の共通部分
#   だけを除外にする（PR が登録を削除した名前は走査される。対象の index に一覧が無ければ除外は 0 件）。
#   PR が自分の変更で一覧に名前を足して検査を免れる経路を作らないため（CI は保護された main の検出器を使う。
#   #51・#6000）。パスの形（*.lock・.claude/skills/・未登録の .agents/skills/ の下）では除外しない。
#   名前は `[a-z0-9-]` の規則に合うものだけを通し、literal の pathspec にする（glob・`..`・`/` を持ち込ませない）。
#   検出器の skills-lock.json が無いときの挙動は 3 つ: (1) 別の checkout なら除外なしで全部走査する。(2) 同じ checkout なら
#   HEAD の一覧を検証する（壊れている・通常ファイルでないときは 2。有効なら除外なしで走査する）。(3) 未作成ブランチ・
#   HEAD に一覧が無いときは除外なし。一覧があるときは、ファイル全体を（正しい UTF-8 かを確かめてから）JSON として厳格に解析し、読めない・形が想定と違う場合は終了コード 2（fail-closed）。
#   検出器と走査対象が同じ checkout（ローカルの pre-commit・make secret-scan）のときは、HEAD・作業ツリー・index の
#   3 つすべてに登録された名前（共通部分）だけが除外に使われる。一部にしか無い名前は除外しない
#   （未コミットの編集で除外を足せない）。
#   HEAD にまだ無い名前は、
#   コミットされるまで走査される（PR では CI が保護された main の検出器で改めて走査する）。
#   残る限界（TASK-6・#6000 で追跡。解消は人が方式を決める）: 除外はディレクトリ単位のため、登録済みの skill のディレクトリでは
#   既存ファイルへの値の追記・新しく置いたファイル・他のパスからのリネームによる移動のいずれも、本文の走査から外れる。
#   登録名の下の変更は、人のレビューで確かめる（内容に結び付けた除外は未実装）。
#   一覧は大きさに上限を設け（暫定。超えたら 2）、走査の結果を返す前に HEAD を取り直して固定した ID と違えば 2 にする。
#   index は最初に 1 回だけ複写して固定し、登録の確認・パス一覧・本文の走査はすべてその複写から行う。走査の後に元の
#   index の内容が固定時と違えば 2 にする（登録の確認と本文が別の index を見る経路を作らない）。
#   固定時にあった index が終了時に無いときも 2（登録 0 件でも）。index がシンボリックリンクなら、開始時も終了時も辿らず 2。
#   複写の前に通常ファイルであることを確かめる（FIFO 等は複写せず 2。確認と cp の間の差し替えは窓を狭めるだけで、止まり得る。0 は返さない）。
#   外部コマンド（git・readlink・mktemp・rm・cp・head・wc・sleep・tr・cmp）は使う前に確かめ、欠けていれば 2。
# 範囲外で検出力が無いもの: コミット履歴・submodule の中・規則に無い形（長い 16 進や base64 の塊単体）。
# 本文のバイト列は照合するが、圧縮・エンコードの展開と、複数行に分かれた値の復元はしない。
# 保守的なヒューリスティックであり網羅的なスキャナの代替ではない。AAUD-30 の実行時の登録簿による走査とも別物。
#
# 誤検知時はダミー値・環境変数参照へ書き換える。`--no-verify` によるバイパスは行わない。
#
# 終了コード: 0 = 検出なし / 1 = 検出 / 2 = 検査を完了できない（fail-closed）。
# 回帰テストは scripts/hooks/test-hooks.sh（CI の test-hooks ジョブ）。
set -euo pipefail

# パターンは ASCII のみ。UTF-8 ロケールのままだと非 UTF-8 のバイト列を含む行で bash の
# [[ =~ ]] が一致しなくなり（macOS・glibc とも）、同じ行のトークンを見逃すため C ロケールで照合する。
# bash は LC_ALL への代入時に setlocale し直すため、以降の照合すべてに効く。
export LC_ALL=C

# fd 4: 元の標準エラー。固定の文言（診断・集計）はすべてここへ出す。--summary では fd 2 を捨てるため、
# 以降の文言は `>&4` で出す（シェル・外部コマンドの生の診断は fd 2 に残り、--summary では出ない）。
exec 4>&2

# 想定外の失敗（set -e 相当）は検出（1）と取り違えないよう、すべて検査不能（2）として止める（fail-closed）。
set -E
trap 'echo "secret-scan: internal error; aborting" >&4; exit 2' ERR

# 置換参照（refs/replace/）でオブジェクトの中身が差し替わると、検査対象と実際の内容が食い違う。無効にする。
export GIT_NO_REPLACE_OBJECTS=1
# git の呼び出しは、設定で外部プログラムを起動し得る経路（hooks・fsmonitor）を無効にして通す。
g() {
  git -c core.hooksPath=/dev/null -c core.fsmonitor=false "$@"
}

mode=staged
summary=0
case "$#:${1:-}:${2:-}" in
  0::) ;;
  1:--tracked:) mode=tracked ;;
  2:--tracked:--summary)
    mode=tracked
    summary=1
    ;;
  *)
    echo "usage: secret-scan.sh [--tracked [--summary]]" >&4
    exit 2
    ;;
esac

# --summary（公開ログ用）では、シェル自身と外部コマンド（git・cd・mktemp 等）の診断を含む標準エラー全体を
# /dev/null へ向ける（診断はパス・値を含み得る。SECR-23）。固定の文言だけを、退避した元の標準エラー（fd 4）へ出す。
# 失敗は終了状態で検知し、固定の文言と終了コードで伝える。通常モードは受け継いだ標準エラーをそのまま使う。
if [ "${summary}" -eq 1 ]; then
  exec 2> /dev/null
fi

# 外部コマンドは使う前にすべて確かめる（makefile.md の契約。欠けていれば検査不能（2）。fail-closed）。
# 確認より前に外部コマンドを呼ばない（`command -v` はシェルの組み込み）。lib.sh は検出器の複製だけで動くよう読み込まない。
# 欠けたコマンドは「失敗の吸収」になり得る（例: 監視の sleep を起動できず、待ち時間なしで読み取りを打ち切る）。
# 固定の文言は fd 4 へ出す（--summary でも出る）。使うコマンドを足したら、この一覧にも足す（試験が踏む経路の漏れを検出する。踏まない経路は検出できない）。
for _c in git readlink mktemp rm cp head wc sleep tr cmp; do
  if ! command -v "${_c}" > /dev/null 2>&1; then
    echo "secret-scan: required command not found on PATH: ${_c}; aborting" >&4
    exit 2
  fi
done
unset _c

# 除外の一覧の読み元は、このスクリプト自身の実体の場所から決める（走査対象の repo ではない）。
# cd は既定で `..` を論理的（シンボリックリンクを辿らず文字列として）に処理するため、scripts や hooks がリンクだと
# 実体と異なるルートを選び得る。スクリプト自身のリンクを先に解決し、実体のディレクトリを物理パス（pwd -P）で取ってから
# 親を切り出す。リンクを解決できない起動・階層が足りない配置は検査不能（2）にする。
# cd が CDPATH で別のディレクトリを選ばないよう先に外す。文字列の加工はシェルの展開だけで行う
# （dirname・readlink のコマンド置換は末尾の改行を落とす。末尾の改行は番兵（x）で保つ）。
unset CDPATH
detector_fail() {
  echo "secret-scan: cannot resolve the detector location; aborting" >&4
  exit 2
}
_src="${BASH_SOURCE[0]}"
_hops=0
while [ -L "${_src}" ]; do
  _hops=$((_hops + 1))
  [ "${_hops}" -le 40 ] || detector_fail
  _link="$(readlink -- "${_src}" && printf x)" || detector_fail
  _link="${_link%x}"
  _link="${_link%$'\n'}"
  [ -n "${_link}" ] || detector_fail
  # 相対リンクは常にリンクの親ディレクトリを基準に解決する（リンク名に / が無ければ親は .）。
  case "${_link}" in
    /*) _src="${_link}" ;;
    *)
      case "${_src}" in
        */*) _link_dir="${_src%/*}" ;;
        *) _link_dir="." ;;
      esac
      [ -n "${_link_dir}" ] || _link_dir="/"
      _src="${_link_dir}/${_link}"
      ;;
  esac
done
case "${_src}" in
  */*) _src_dir="${_src%/*}" ;;
  *) _src_dir="." ;;
esac
[ -n "${_src_dir}" ] || _src_dir="/"
if ! _real_dir="$(cd -P -- "${_src_dir}" && pwd -P && printf x)"; then
  detector_fail
fi
_real_dir="${_real_dir%x}"
_real_dir="${_real_dir%$'\n'}"
# 実体のディレクトリ（<root>/scripts/hooks）から 2 階層上がルート。階層が足りない配置は検査不能（2）。
case "${_real_dir}" in
  /*/*) ;;
  *) detector_fail ;;
esac
detector_root="${_real_dir%/*}"
detector_root="${detector_root%/*}"
[ -n "${detector_root}" ] || detector_root="/"
unset _src _src_dir _hops _link _link_dir _real_dir
detector_lock="${detector_root}/skills-lock.json"

# 呼び出し側の環境変数で pathspec の解釈（literal・glob・大文字小文字無視）が変わると、除外の範囲がずれる。
unset GIT_LITERAL_PATHSPECS GIT_GLOB_PATHSPECS GIT_NOGLOB_PATHSPECS GIT_ICASE_PATHSPECS

# 行に対する規則（bash 3.2 互換のため添字配列を並べる）。nocase_from 以降は大文字小文字を区別しない。
rule_ids=(vendor-token private-key jwt jwk-private credential-url assignment)
rule_pats=(
  'sk-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{35}'
  'BEGIN (RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY'
  'eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.'
  '"d"[[:space:]]*:[[:space:]]*"[A-Za-z0-9_-]{40,}"'
  # 本ファイル自身が自分の規則に一致しないよう、`://` を分割して書く
  '[a-zA-Z][a-zA-Z0-9+.-]*:''//[^[:space:]/@:]+:[^[:space:]/@]+@'
  '(api[_-]?key|secret|token|password|passphrase|recovery[_-]?codes?)["'"'"']?[[:space:]]*[:=][[:space:]]*["'"'"'][A-Za-z0-9_+/=-]{16,}'
)
nocase_from=5
rule_count="${#rule_ids[@]}"

# classify_line <line>: 一致した最初の規則の添字を hit に入れて 0・どれにも一致しなければ 1。
# 判定は bash の [[ =~ ]]（POSIX ERE）で行い、grep の実装差による fail-open 経路を持たない。
# [[ =~ ]] の 2（正規表現のコンパイル失敗）を「一致なし」と同一視すると検査できないまま
# 合格するため、その場で終了コード 2 で止める。diff の追加行ごとに呼ばれるため、
# プロセス起動（サブシェル・コマンド置換）を使わない。
hit=-1
classify_line() {
  local i rc
  hit=-1
  for ((i = 0; i < rule_count; i++)); do
    if [ "${i}" -eq "${nocase_from}" ]; then
      shopt -s nocasematch
    fi
    rc=0
    # shellcheck disable=SC2319 # 取りたいのは [[ =~ ]] 自体の終了状態（0 / 1 / 2）
    [[ "$1" =~ ${rule_pats[i]} ]] || rc=$?
    if [ "${rc}" -eq 0 ]; then
      hit="${i}"
      break
    elif [ "${rc}" -gt 1 ]; then
      echo "secret-scan: regex evaluation failed (rc=${rc}); aborting" >&4
      exit 2
    fi
  done
  shopt -u nocasematch
  [ "${hit}" -ge 0 ]
}

# 自己テスト: 正規表現が実装差・ロケールで機能しない環境では検査結果を信用できないため止める。
# 一致させたい例示値は実行時に組み立て、本ファイル自体のコミットが検知されないようにする。
q='"' at='@' dash='-----' hi=$'\xe9'
jwk_d="Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij""Abcdefghij"
selftest_fail() {
  echo "secret-scan: self-test failed (regex engine / locale mismatch?); aborting" >&4
  exit 2
}
# expect_rule <規則 ID> <行>: その規則に分類されること
expect_rule() {
  classify_line "$2" || selftest_fail
  [ "${rule_ids[hit]}" = "$1" ] || selftest_fail
}
# expect_none <行>: どの規則にも一致しないこと
expect_none() {
  if classify_line "$1"; then selftest_fail; fi
}
expect_rule vendor-token "+caf${hi} gh""p_abcdefghijklmnopqrstuvwx"
expect_rule private-key "+${dash}BEGIN OPENSSH PRIVATE"" KEY${dash}"
expect_rule private-key "+${dash}BEGIN ENCRYPTED PRIVATE"" KEY${dash}"
expect_rule jwt "+x = eyJ""hbGciOiJFUzI1NiJ9.eyJ""zdWIiOiJ1c2VyMSJ9.abc"
expect_rule jwk-private "+{${q}d${q}: ${q}${jwk_d}${q}}"
expect_rule credential-url "+db: postgres://user:pass${at}host:5432/db"
expect_rule assignment "+password = ${q}Hunter2Hunter2Hunter2${q}"
expect_rule assignment "+API_KEY: ${q}abcdefghijklmnop1234${q}"
expect_rule assignment "+recovery_codes = ${q}Abcd1234-Efgh5678-Ijkl${q}"
expect_rule assignment "+${q}passphrase${q}: ${q}abc_DEF+ghi/JKL=mno-pqr${q}"
# shellcheck disable=SC2016 # ${DB_PASSWORD} は展開させない例示値
expect_none '+password = "${DB_PASSWORD}"'
expect_none '+token = "<placeholder-value-here>"'
expect_none '+see https://example.com/path'
expect_none "+  sha256: $(printf 'ab%.0s' {1..32})"
expect_none "+password = ${q}short${q}"

# サブディレクトリから直接実行しても走査範囲が狭まらないよう、リポジトリルートで実行する。
# rev-parse の失敗を `cd "$(...)"` に埋めると cd "" になり失敗が隠れるため、先に代入で受ける。
# `$(...)` は末尾の改行を落とすため、番兵（x）を付けて受けてから外す（改行で終わるパスも保つ）。
if ! repo_root="$(g rev-parse --show-toplevel && printf x)"; then
  echo "secret-scan: not inside a git work tree; aborting" >&4
  exit 2
fi
repo_root="${repo_root%x}"
repo_root="${repo_root%$'\n'}"
if [ -z "${repo_root}" ]; then
  echo "secret-scan: not inside a git work tree; aborting" >&4
  exit 2
fi
if ! cd "${repo_root}"; then
  echo "secret-scan: cannot enter the repository root; aborting" >&4
  exit 2
fi

if ! tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/secret-scan.XXXXXX")" || [ -z "${tmp_dir}" ]; then
  echo "secret-scan: mktemp failed; aborting" >&4
  exit 2
fi
# EXIT の trap から呼ぶ（読み取りの監視を fork する間だけ外して戻すため、関数名 1 つで張り直せる形にする）
# shellcheck disable=SC2329 # trap からの間接の呼び出し
cleanup_tmp() {
  rm -rf "${tmp_dir}"
}
trap cleanup_tmp EXIT

# index を 1 回だけ複写して固定し、以降の登録の確認・パス一覧・本文の走査はすべてその複写（GIT_INDEX_FILE）に対して行う。
# 別々に index を開くと、間に index が書き換わったとき、登録の確認とパス一覧・本文が別の内容を見る（#6000）。
# 走査の後に、元の index の内容（git ls-files -s と intent-to-add の印。下の index_listing）が固定時と同じことを確かめ、違えば検査不能（2）にする
# （stat 情報だけの更新では差が出ない）。index が無い（空）ときは存在しない複写のパスを指し、git は空として扱う。
# 元の index のパスは、GIT_INDEX_FILE の有無にかかわらず git 自身の解決（rev-parse --git-path index）で絶対パスにして受ける。
# git は相対パスの GIT_INDEX_FILE を、起動時の作業ディレクトリではなく作業ツリーのルートを基準に読む（git が先にルートへ移るため）。
# 自前で基準を決めると、git が読む index と別のファイルを固定し得る（無ければ空として 0 を返す fail-open。#6011）。
# `$(...)` は末尾の改行を落とすため、番兵（x）を付けて受けてから外す。
if ! orig_index="$(g rev-parse --path-format=absolute --git-path index && printf x)"; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
orig_index="${orig_index%x}"
orig_index="${orig_index%$'\n'}"
case "${orig_index}" in
  /*) ;;
  *)
    echo "secret-scan: cannot snapshot the index; aborting" >&4
    exit 2
    ;;
esac
snap_index="${tmp_dir}/index-snap"
# index のパスがシンボリックリンクなら、辿らずに検査不能（2）で止める。--path-format=absolute は git がリンクを実体へ解いた
# パスを返し、実体で固定すると走査中のリンクの付け替えに気づけない。解く前のパス（--path-format の指定なし。作業ツリーの
# ルートで呼ぶため、相対パスはルート基準で、git が読む場所と同じ）を -L で見る。通常の運用で index はリンクにならない。
# 開始時と終了時の両方で同じ形で確かめる（index_is_link）。パスを出力しない。
if ! raw_index="$(g rev-parse --git-path index && printf x)"; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
raw_index="${raw_index%x}"
raw_index="${raw_index%$'\n'}"
if [ -z "${raw_index}" ]; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
index_is_link() {
  [ -L "${raw_index}" ] || [ -L "${orig_index}" ]
}
if index_is_link; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
# 固定時に index があったか（終了時に無ければ、登録が 0 件でも検査不能（2）。固定時の有無が実装と規約の一致の要）。
# リンクは上で止めている。通常ファイルでない（FIFO・デバイス等）ものは cp が読み手の到着待ちで止まり得るため、複写せず検査不能（2）にする。
# 確認と cp の間に差し替えられる窓は残る（窓を狭めるだけで、止まり得る。0 は返さない。監視付きの複写にはしない）。
index_pinned=0
if [ -e "${orig_index}" ]; then
  if [ ! -f "${orig_index}" ]; then
    echo "secret-scan: cannot snapshot the index; aborting" >&4
    exit 2
  fi
  index_pinned=1
  if ! cp -- "${orig_index}" "${snap_index}" 2> /dev/null; then
    echo "secret-scan: cannot snapshot the index; aborting" >&4
    exit 2
  fi
fi
export GIT_INDEX_FILE="${snap_index}"
# 空の tree の ID は常に解決する。未作成ブランチの staged も、基準を暗黙にせず空の tree を明示して固定する
# （基準を省くと、走査中にブランチが作られたとき名前の検査と本文の検査の基準がずれ得る）。
# index の比較と intent-to-add の判定（空の tree との cached の差分）にも使うため、ここで先に解決する。
empty_tree=""
if ! empty_tree="$(g hash-object -t tree --stdin < /dev/null)" || [ -z "${empty_tree}" ]; then
  echo "secret-scan: failed to resolve the empty tree; aborting" >&4
  exit 2
fi
# index_listing <出力先>: GIT_INDEX_FILE の指す index の内容を、走査前後の比較用に書き出す。
# ls-files -s はモード・blob ID・stage・パスだけで、intent-to-add の印を含まない（git add -N の空 blob と、実際に
# ステージした空ファイルが同じ行になる）。intent-to-add の項目を除く cached の差分（空の tree との比較。
# 作業ツリー・stat 情報を見ない）のパス一覧を後ろに足し、印の変化も比べる（intent-to-add の .env が走査中に
# 実際にステージされた場合を逃さない。#6011）。区切りの NUL 2 つは、どちらの一覧の項目にも現れない。
index_listing() {
  g ls-files -s -z > "$1" || return 1
  printf '\000\000' >> "$1" || return 1
  g diff --cached --name-only -z --no-renames --ita-invisible-in-index "${empty_tree}" -- >> "$1" || return 1
}
if ! index_listing "${tmp_dir}/index-listing-before"; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi

# 本文の走査から外す名前を、検出器の skills-lock.json から読む。形は
#   { "version": 2, "skills": { "<名前>": { ... }, ... } }
# で、ファイル全体を JSON として厳格に解析する（bash だけ。jq・python に依存しない）。
#   - 文法: 値は 1 つだけで後続なし・カンマの欠落と末尾カンマは不可・文字列のエスケープを解釈する。
#   - 意味: トップは object で、キーは version（数値 2 だけ）と skills（空でない object）の各 1 回だけ。
#     skills の名前は重複不可・エスケープなしで `[a-z0-9-]` の規則に合うもの・値は object。
# 文字列中の { } は構造として数えない。診断には名前・パスを出さない（SECR-23）。
skill_names=()
lock_fail() {
  echo "secret-scan: skills-lock.json is unreadable or has an unexpected shape; aborting" >&4
  exit 2
}
# 解析の状態。js は全文で、解析は小さな窓 jb（jn: 窓の長さ・jp: 窓の中の位置）と未読の残り jrest だけを触る。
# bash の ${var:i:n} は変数の長さに比例して遅く（全文への添字アクセスは入力の大きさのほぼ 2 乗になる）、
# 窓を小さく保てば 1 回の添字アクセスは窓の長さだけで済む（#6011）。サブシェルを起こさず、失敗はその場で lock_fail で止める。
js="" jn=0 jp=0 jb="" jrest="" jmore=0 json_run_skip=0 json_flat_skip=0 str_raw="" str_esc=0 ws_rest="" ws_lead=""
json_ws_pat=$'[! \t\n\r]*'
# 早道の正規表現: エスケープ・制御文字を含まず、窓の中で閉じる文字列（U+0001〜U+001F・引用符・バックスラッシュ以外）。
json_str_re=$'^"([^"\\\\\x01-\x1f]*)"'
# 早道の正規表現: 値がすべて文字列・数・true・false・null の 1 段の object（入れ子・エスケープ・制御文字なし）。
# 文法は逐次の読みと同じで、一致すれば逐次の読みと同じ位置まで進む（一致しなければ逐次の読みに任せる）。
json_re_ws=$'[ \t\n\r]*'
# 文字列（エスケープを含む。逐次の読みと同じ文法: 制御文字は不可・エスケープは \" \\ \/ \b \f \n \r \t \uXXXX）。
json_re_str="\"([^\"\\\\"$'\x01-\x1f'"]|\\\\([\"\\\\/bfnrt]|u[0-9A-Fa-f]{4}))*\""
json_re_num='-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?'
json_re_val="(${json_re_str}|${json_re_num}|true|false|null)"
json_re_mem="${json_re_str}${json_re_ws}:${json_re_ws}${json_re_val}"
json_re_obj="^\\{${json_re_ws}(\\}|${json_re_mem}(${json_re_ws},${json_re_ws}${json_re_mem})*${json_re_ws}\\})"
json_re_arr="^\\[${json_re_ws}(\\]|${json_re_val}(${json_re_ws},${json_re_ws}${json_re_val})*${json_re_ws}\\])"
# skills の 1 件（名前・コロン・値が 1 段の object）を 1 回の照合で読む。名前は規則に合うエスケープなしのもの（group 1）。
json_re_skill="^${json_re_ws}\"([a-z0-9]([a-z0-9-]*[a-z0-9])?)\"${json_re_ws}:${json_re_ws}\\{${json_re_ws}(\\}|${json_re_mem}(${json_re_ws},${json_re_ws}${json_re_mem})*${json_re_ws}\\})"
# 窓の中の連続する要素・メンバを 1 回の照合で読み飛ばす正規表現（完全な「値 + 空白 + カンマ」の単位だけに一致する。最後の要素・閉じ括弧・
# 窓の終わりで切れた単位は一致しないため、従来の逐次の読みが続きを読む）。文字列の本体（エスケープを含む）の連続も同様。
# 連続の単位の値は、スカラー・空の object・空の配列・値がそれらだけの 1 段の object・配列（深さは最大 2 段下がる。呼び出し側で上限を確かめる）。
json_re_v1="(${json_re_val}|\\{${json_re_ws}\\}|\\[${json_re_ws}\\])"
json_re_m1="${json_re_str}${json_re_ws}:${json_re_ws}${json_re_v1}"
json_re_v2="(${json_re_v1}|\\{${json_re_ws}${json_re_m1}(${json_re_ws},${json_re_ws}${json_re_m1})*${json_re_ws}\\}|\\[${json_re_ws}${json_re_v1}(${json_re_ws},${json_re_ws}${json_re_v1})*${json_re_ws}\\])"
json_re_m2="${json_re_str}${json_re_ws}:${json_re_ws}${json_re_v2}"
json_re_arrrun="^(${json_re_ws}${json_re_v2}${json_re_ws},)+"
json_re_objrun="^(${json_re_ws}${json_re_m2}${json_re_ws},)+"
json_re_sbody="^(([^\"\\\\"$'\x01-\x1f'"])|(\\\\([\"\\\\/bfnrt]|u[0-9A-Fa-f]{4})))*"
# json_refill: 読み終えた分を窓から捨て、未読の残りから 1024 バイトを窓へ足す（jp は 0 に戻る）。
json_refill() {
  jb="${jb:jp}${jrest:0:1024}"
  jrest="${jrest:1024}"
  # 未読が残るかは、変数を展開せずに済む旗で持つ（"${jrest}" の展開は残りの長さに比例して遅く、字句ごとに行うと 2 乗になる。#6011）。
  if [ -n "${jrest}" ]; then
    jmore=1
  else
    jmore=0
  fi
  jn="${#jb}"
  jp=0
}
# json_fill: 窓に先読み 64 バイトが無く、未読が残っていれば足す。以降 64 バイト先までの添字は有効
# （入力の終わりを除く）。窓を足すと jp が変わるため、呼び出しの前に読んだ位置を持ち越さない。
json_fill() {
  if [ $((jn - jp)) -lt 64 ] && [ "${jmore}" -eq 1 ]; then
    json_refill
  fi
}
# json_ws: 空白（空白・タブ・改行・復帰）を読み飛ばす。空白が無い呼び出しは窓の先頭の 1 文字だけで戻る早道を置く
# （命令数を減らす。#6011）。
json_ws() {
  if [ $((jn - jp)) -lt 64 ] && [ "${jmore}" -eq 1 ]; then
    json_refill
  fi
  case "${jb:jp:1}" in
    ' ' | $'\t' | $'\n' | $'\r') ;;
    *) return 0 ;;
  esac
  while :; do
    if [ $((jn - jp)) -lt 64 ] && [ "${jmore}" -eq 1 ]; then
      json_refill
    fi
    ws_rest="${jb:jp}"
    # shellcheck disable=SC2295 # json_ws_pat はパターンとして使う（引用すると文字列の一致になる）
    ws_lead="${ws_rest%%${json_ws_pat}}"
    jp=$((jp + ${#ws_lead}))
    # 空白以外の文字に当たったか、入力の終わりなら止める（窓の終わりまで空白なら続きを足して読む）。
    if [ "${jp}" -lt "${jn}" ] || [ "${jmore}" -eq 0 ]; then
      break
    fi
  done
}
# json_string: 位置の文字列を読み、引用符の中の生の文字列を str_raw（エスケープを含めば str_esc=1）に入れる。
# 引数 skip のときは str_raw を組み立てない（値・入れ子のキーでは使わない。長い文字列で連結が 2 乗になるのを避ける）。
json_string() {
  local rest chunk c e hex keep=1 m
  [ "${1:-}" != skip ] || keep=0
  str_raw="" str_esc=0
  json_fill
  [ "${jb:jp:1}" = '"' ] || lock_fail
  # 早道: 窓の中で閉じ、エスケープ・制御文字を含まない文字列は 1 回の照合で読む。
  # 一致しなければ（窓をまたぐ・エスケープ・不正）下の逐次の読みに任せ、判定は変わらない。
  if [[ "${jb:jp}" =~ ${json_str_re} ]]; then
    str_raw="${BASH_REMATCH[1]}"
    jp=$((jp + ${#str_raw} + 2))
    return 0
  fi
  jp=$((jp + 1))
  while :; do
    json_fill
    rest="${jb:jp}"
    # 窓の中の有効な文字・エスケープの連続は 1 回の照合で読む（残りは下の逐次の読みが判定する）。
    if [[ "${rest}" =~ ${json_re_sbody} ]] && [ -n "${BASH_REMATCH[0]}" ]; then
      m="${BASH_REMATCH[0]}"
      if [ "${keep}" -eq 1 ]; then
        str_raw="${str_raw}${m}"
      fi
      if [[ "${m}" == *\\* ]]; then
        str_esc=1
      fi
      jp=$((jp + ${#m}))
      rest="${jb:jp}"
    fi
    chunk="${rest%%[\"\\]*}"
    # JSON でエスケープが必須なのは U+0001〜U+001F（NUL は読み取りの検査で拒否済み）。U+007F は有効な文字。
    [[ "${chunk}" != *[$'\x01'-$'\x1f']* ]] || lock_fail
    if [ "${keep}" -eq 1 ]; then
      str_raw="${str_raw}${chunk}"
    fi
    jp=$((jp + ${#chunk}))
    if [ "${chunk}" = "${rest}" ]; then
      # 窓の中に終わりの引用符・バックスラッシュが無い。未読が残っていれば続きを足して読み、無ければ閉じていない文字列。
      [ "${jmore}" -eq 1 ] || lock_fail
      json_refill
      continue
    fi
    json_fill
    c="${jb:jp:1}"
    if [ "${c}" = '"' ]; then
      jp=$((jp + 1))
      return 0
    fi
    str_esc=1
    e="${jb:$((jp + 1)):1}"
    case "${e}" in
      '"' | \\ | / | b | f | n | r | t)
        if [ "${keep}" -eq 1 ]; then
          str_raw="${str_raw}\\${e}"
        fi
        jp=$((jp + 2))
        ;;
      u)
        hex="${jb:$((jp + 2)):4}"
        [[ "${hex}" =~ ^[0-9A-Fa-f]{4}$ ]] || lock_fail
        if [ "${keep}" -eq 1 ]; then
          str_raw="${str_raw}\\u${hex}"
        fi
        jp=$((jp + 6))
        ;;
      *) lock_fail ;;
    esac
  done
}
# json_value <深さ>: 任意の JSON 値 1 つを文法どおりに読み進める（値は使わない）。
json_value() {
  local c rest
  [ "$1" -le 32 ] || lock_fail
  json_ws
  c="${jb:jp:1}"
  case "${c}" in
    '{')
      # 空の object は先頭 2 文字で読む（以降の照合は式の組み立てが重いため、字句ごとの呼び出しを減らす）。
      if [ "${jb:jp:2}" = '{}' ]; then
        jp=$((jp + 2))
        return 0
      fi
      # 早道: 1 段の object が窓の中で閉じていれば 1 回の照合で読み飛ばす（深さの上限は逐次の読みと同じ: 値は深さ + 1）。
      # 一致しなかった後の 8 回は試さない（式の組み立てが重く、一致しない形が続くと逆に遅くなる）。
      if [ "${json_flat_skip}" -gt 0 ]; then
        json_flat_skip=$((json_flat_skip - 1))
      elif [ "$1" -lt 32 ]; then
        if [[ "${jb:jp}" =~ ${json_re_obj} ]]; then
          jp=$((jp + ${#BASH_REMATCH[0]}))
          return 0
        fi
        json_flat_skip=8
      fi
      jp=$((jp + 1))
      json_ws
      if [ "${jb:jp:1}" = '}' ]; then
        jp=$((jp + 1))
        return 0
      fi
      while :; do
        # 窓の中の連続するメンバ（キーと文字列・数・true・false・null の値）を 1 回の照合で読み飛ばす（値は深さ + 1）。
        # 一致しなかった後の 8 回は（入れ子の中も含めて）試さない（式の組み立てが重く、一致しない形が続くと逆に遅くなる）。
        if [ "${json_run_skip}" -gt 0 ]; then
          json_run_skip=$((json_run_skip - 1))
        elif [ "$1" -lt 31 ]; then
          json_fill
          if [[ "${jb:jp}" =~ ${json_re_objrun} ]]; then
            jp=$((jp + ${#BASH_REMATCH[0]}))
          else
            json_run_skip=8
          fi
        fi
        json_ws
        json_string skip
        json_ws
        [ "${jb:jp:1}" = ':' ] || lock_fail
        jp=$((jp + 1))
        json_value $(($1 + 1))
        json_ws
        c="${jb:jp:1}"
        jp=$((jp + 1))
        case "${c}" in
          ',') ;;
          '}') return 0 ;;
          *) lock_fail ;;
        esac
      done
      ;;
    '[')
      if [ "${jb:jp:2}" = '[]' ]; then
        jp=$((jp + 2))
        return 0
      fi
      # 早道: 値がすべて文字列・数・true・false・null の 1 段の配列が窓の中で閉じていれば 1 回の照合で読み飛ばす。
      if [ "${json_flat_skip}" -gt 0 ]; then
        json_flat_skip=$((json_flat_skip - 1))
      elif [ "$1" -lt 32 ]; then
        if [[ "${jb:jp}" =~ ${json_re_arr} ]]; then
          jp=$((jp + ${#BASH_REMATCH[0]}))
          return 0
        fi
        json_flat_skip=8
      fi
      jp=$((jp + 1))
      json_ws
      if [ "${jb:jp:1}" = ']' ]; then
        jp=$((jp + 1))
        return 0
      fi
      while :; do
        # 窓の中の連続する要素（文字列・数・true・false・null）を 1 回の照合で読み飛ばす（要素は深さ + 1）。
        # 一致しなかった後の 8 回は（入れ子の中も含めて）試さない（式の組み立てが重く、一致しない形が続くと逆に遅くなる）。
        if [ "${json_run_skip}" -gt 0 ]; then
          json_run_skip=$((json_run_skip - 1))
        elif [ "$1" -lt 31 ]; then
          json_fill
          if [[ "${jb:jp}" =~ ${json_re_arrrun} ]]; then
            jp=$((jp + ${#BASH_REMATCH[0]}))
          else
            json_run_skip=8
          fi
        fi
        json_value $(($1 + 1))
        json_ws
        c="${jb:jp:1}"
        jp=$((jp + 1))
        case "${c}" in
          ',') ;;
          ']') return 0 ;;
          *) lock_fail ;;
        esac
      done
      ;;
    '"') json_string skip ;;
    *)
      json_fill
      while :; do
        rest="${jb:jp}"
        if [[ "${rest}" =~ ^(-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?) ]] && [ -n "${BASH_REMATCH[1]}" ]; then
          # 窓の終わりに近い一致は、続きがあれば長くなり得る（数字の続き・指数部は最大 3 文字先まで）。足して読み直す。
          if [ $((jp + ${#BASH_REMATCH[1]} + 3)) -gt "${jn}" ] && [ "${jmore}" -eq 1 ]; then
            json_refill
            continue
          fi
          jp=$((jp + ${#BASH_REMATCH[1]}))
        elif [ "${rest:0:4}" = true ] || [ "${rest:0:4}" = null ]; then
          jp=$((jp + 4))
        elif [ "${rest:0:5}" = false ]; then
          jp=$((jp + 5))
        else
          lock_fail
        fi
        break
      done
      ;;
  esac
}
# json_seen_add: 名前（name）が既出なら lock_fail、初出なら記録する。名前は [a-z0-9-] だけ（'-' を '_' に直しても衝突しない:
# 名前に '_' は現れない）。名前ごとの変数で持ち、既出の確認は名前の数に依らない（文字列に連ねて検索すると 2 乗になる。#6011）。
# 変数名の接頭辞 JSEEN_ は大文字で、このスクリプトの他の変数（小文字）と名前空間が重ならない（名前 vars 等と衝突しない）。
# 環境から継いだ JSEEN_* も含め、parse_lock の先頭で消す。
json_seen_add() {
  local sv="JSEEN_${name//-/_}"
  [ -z "${!sv:-}" ] || lock_fail
  printf -v "${sv}" '%s' 1
}
# parse_skills: skills の object を読み、検証した名前を parsed_names へ足す。
parse_skills() {
  local name c
  json_ws
  [ "${jb:jp:1}" = '{' ] || lock_fail
  jp=$((jp + 1))
  json_ws
  if [ "${jb:jp:1}" = '}' ]; then
    jp=$((jp + 1))
    return 0
  fi
  while :; do
    json_fill
    # 早道: 名前・コロン・1 段の object の値を 1 回の照合で読む。一致しなければ（エスケープ・規則外の名前・入れ子・窓をまたぐ等）
    # 下の逐次の読みが同じ規則で判定する（名前の規則・重複・値の文法は同じ）。
    if [[ "${jb:jp}" =~ ${json_re_skill} ]]; then
      name="${BASH_REMATCH[1]}"
      jp=$((jp + ${#BASH_REMATCH[0]}))
      json_seen_add
      parsed_names+=("${name}")
      parsed_count=$((parsed_count + 1))
    else
      json_ws
      json_string
      [ "${str_esc}" -eq 0 ] || lock_fail
      name="${str_raw}"
      [[ "${name}" =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?$ ]] || lock_fail
      json_seen_add
      parsed_names+=("${name}")
      parsed_count=$((parsed_count + 1))
      json_ws
      [ "${jb:jp:1}" = ':' ] || lock_fail
      jp=$((jp + 1))
      json_ws
      [ "${jb:jp:1}" = '{' ] || lock_fail
      json_value 2
    fi
    json_ws
    c="${jb:jp:1}"
    jp=$((jp + 1))
    case "${c}" in
      ',') ;;
      '}') return 0 ;;
      *) lock_fail ;;
    esac
  done
}
# parse_lock <ファイル>: 全体を解析し、検証した名前を parsed_names に入れる。不正は lock_fail（終了コード 2）。
# 一覧の大きさの上限（暫定。bash の解析は遅いため小さく置く。値の確定は人が行う。#6000）。超えたら検査不能（2）。
lock_max_bytes=65536
lock_too_large() {
  echo "secret-scan: skills-lock.json is too large; aborting" >&4
  exit 2
}
parsed_names=()
parsed_count=0
lock_cache_valid=0
lock_cache_names=()
lock_cache_count=0
parse_lock() {
  local size key c utf8_t seen_version=0 seen_skills=0 snap
  parsed_names=()
  parsed_count=0
  # 名前ごとの変数（前の解析・環境から継いだものを含む）を消す。`${!JSEEN_@}` は接頭辞に一致する変数名の一覧で、
  # 一致が無ければ空（set -u でも失敗しない）。compgen は programmable completion を外した bash に無く、黙って空になるため使わない。
  for _jseen_name in ${!JSEEN_@}; do
    unset "${_jseen_name}"
  done
  json_run_skip=0
  json_flat_skip=0
  js=""
  # 読み取りとサイズの取得が同じパスを別々に開くと、間にファイルが差し替わり得る。一度だけ一時ファイルへ複写して固定し、
  # 以降の read・wc はその複写に対してだけ行う（一時ファイルは終了時に trap で消える）。
  snap="${tmp_dir}/lock-snap"
  # 上限 + 1 バイトだけ複写し、超えていれば固定の診断で止める（巨大な一覧でフックが長時間止まらないようにする）。
  if ! head -c $((lock_max_bytes + 1)) -- "$1" > "${snap}"; then
    lock_fail
  fi
  # サイズは複写に対して一度だけ取得する。失敗・数字以外は検査不能（2）。代入の失敗を set -e に任せない
  # （検出の終了コード 1 と取り違えないため）。
  if ! size="$(wc -c < "${snap}")"; then
    lock_fail
  fi
  size="${size//[[:space:]]/}"
  [[ "${size}" =~ ^[0-9]+$ ]] || lock_fail
  if [ "${size}" -gt "${lock_max_bytes}" ]; then
    lock_too_large
  fi
  # 直前に解析に成功した一覧と内容が同じなら、解析結果を再利用する（作業ツリー・index・HEAD の一覧が同じ内容のとき、
  # 解析を 1 回で済ませる。#6011）。解析は内容だけで決まり、上のサイズの検査は通過済みのため、判定は同じになる。
  # cmp の終了状態が 0（同一）以外（差異・読み取りの失敗）は、いつも通り解析する。
  if [ "${lock_cache_valid}" -eq 1 ] && cmp -s -- "${snap}" "${tmp_dir}/lock-parsed" 2> /dev/null; then
    parsed_names=(${lock_cache_names[@]+"${lock_cache_names[@]}"})
    parsed_count="${lock_cache_count}"
    return 0
  fi
  IFS= read -r -d '' js < "${snap}" || true
  # NUL を含むと bash の読み取りがそこで止まり、後ろが未検証になる。読めたバイト数とサイズを突き合わせる。
  [ "${#js}" -eq "${size}" ] || lock_fail
  # 構文解析の前に、入力全体が正しい UTF-8 であることを確かめる（C ロケールのバイト単位の照合。iconv 等に依存しない）。
  # 正しい多バイト列（最短形・サロゲート不可・U+10FFFF 以下）を ASCII の 1 文字へ置き換え、80 以上のバイトが残れば不正。
  # 空へ置き換えると、不正なバイトの両側が接して新しい列になり得るため、必ず ASCII の文字へ置き換える。
  utf8_t="${js//[$'\xf0'][$'\x90'-$'\xbf'][$'\x80'-$'\xbf'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xf1'-$'\xf3'][$'\x80'-$'\xbf'][$'\x80'-$'\xbf'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xf4'][$'\x80'-$'\x8f'][$'\x80'-$'\xbf'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xe0'][$'\xa0'-$'\xbf'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xe1'-$'\xec'$'\xee'$'\xef'][$'\x80'-$'\xbf'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xed'][$'\x80'-$'\x9f'][$'\x80'-$'\xbf']/A}"
  utf8_t="${utf8_t//[$'\xc2'-$'\xdf'][$'\x80'-$'\xbf']/A}"
  [[ "${utf8_t}" != *[$'\x80'-$'\xff']* ]] || lock_fail
  # 解析は窓 jb と未読の残り jrest で行う（全文 js は添字アクセスしない）。
  jb=""
  jrest="${js}"
  if [ -n "${jrest}" ]; then
    jmore=1
  else
    jmore=0
  fi
  jn=0
  jp=0
  json_ws
  [ "${jb:jp:1}" = '{' ] || lock_fail
  jp=$((jp + 1))
  while :; do
    json_ws
    json_string
    [ "${str_esc}" -eq 0 ] || lock_fail
    key="${str_raw}"
    json_ws
    [ "${jb:jp:1}" = ':' ] || lock_fail
    jp=$((jp + 1))
    json_ws
    case "${key}" in
      version)
        [ "${seen_version}" -eq 0 ] || lock_fail
        seen_version=1
        [ "${jb:jp:1}" = 2 ] || lock_fail
        jp=$((jp + 1))
        ;;
      skills)
        [ "${seen_skills}" -eq 0 ] || lock_fail
        seen_skills=1
        parse_skills
        ;;
      *) lock_fail ;;
    esac
    json_ws
    c="${jb:jp:1}"
    jp=$((jp + 1))
    case "${c}" in
      ',') ;;
      '}') break ;;
      *) lock_fail ;;
    esac
  done
  json_ws
  [ "${jp}" -eq "${jn}" ] || lock_fail
  if [ "${seen_version}" -ne 1 ] || [ "${seen_skills}" -ne 1 ] || [ "${parsed_count}" -eq 0 ]; then
    lock_fail
  fi
  # 成功した解析の内容と結果を覚える（次の一覧が同じ内容なら再利用する）。複写の失敗は再利用をやめるだけにする。
  lock_cache_valid=0
  if cp -- "${snap}" "${tmp_dir}/lock-parsed" 2> /dev/null; then
    lock_cache_names=(${parsed_names[@]+"${parsed_names[@]}"})
    lock_cache_count="${parsed_count}"
    lock_cache_valid=1
  fi
}
# HEAD のコミット ID を 1 回だけ確定する（head_id。未作成ブランチは空）。以降の ls-tree・cat-file・staged の差分の基準は
# すべてこの ID を使い、途中で HEAD が動いても検証した種別・読む blob・差分の基準がずれないようにする。
# 未作成ブランチ（HEAD の指す参照そのものが無い）だけが空の ID で通る。参照があるのにコミットとして解決できない
# （オブジェクトの欠損等）・detached HEAD の解決失敗・参照の判定の失敗は検査不能（2）。
head_id=""
head_resolved=0
resolve_head() {
  local rc head_ref ref_line ref_found
  head_resolved=1
  head_id=""
  rc=0
  # 'HEAD^{commit}' は短縮参照名の規則で解決され、未作成ブランチでは refs/tags/HEAD 等に誤って当たる。
  # シンボリック HEAD は先に参照先の完全名（refs/...）を取り、その完全名だけで厳密に解決する。
  # コマンド置換の中で終了状態 1（detached HEAD）を返すと、bash 3.2（macOS）は ERR トラップをサブシェルで実行して
  # 2 に化かす。置換を使わず、一時ファイルへ書き出して終了状態をそのまま受ける。
  head_ref=""
  g symbolic-ref -q HEAD > "${tmp_dir}/head-ref" 2> /dev/null || rc=$?
  if [ "${rc}" -eq 0 ]; then
    IFS= read -r head_ref < "${tmp_dir}/head-ref" || true
  fi
  if [ "${rc}" -eq 1 ]; then
    # detached HEAD（$GIT_DIR/HEAD が直接コミットを指す）。
    if head_id="$(g rev-parse -q --verify 'HEAD^{commit}' 2> /dev/null)" && [[ "${head_id}" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]]; then
      return 0
    fi
    head_id=""
  elif [ "${rc}" -eq 0 ] && [ -n "${head_ref}" ]; then
    case "${head_ref}" in
      refs/*)
        # rev-parse --verify は refs/ で始まる名前にも候補の展開（refs/refs/…・refs/tags/refs/… 等）を行い、
        # 完全名と別の参照へ解決し得る。show-ref --verify は完全名の一致だけを受けるため、先にそれでオブジェクト ID を取り、
        # その ID（40・64 桁の 16 進）だけをコミットとして検証する。
        rc=0
        g show-ref --verify -q "${head_ref}" > /dev/null 2>&1 || rc=$?
        if [ "${rc}" -eq 0 ]; then
          ref_line="$(g show-ref --verify "${head_ref}" 2> /dev/null)" || rc=$?
        fi
        if [ "${rc}" -eq 0 ] && [[ "${ref_line}" =~ ^([0-9a-f]{40}([0-9a-f]{24})?)[[:space:]] ]]; then
          if head_id="$(g rev-parse -q --verify "${BASH_REMATCH[1]}^{commit}" 2> /dev/null)" && [ "${head_id}" = "${BASH_REMATCH[1]}" ]; then
            return 0
          fi
          rc=2
        elif [ "${rc}" -eq 0 ]; then
          rc=2
        fi
        head_id=""
        ;;
      *) rc=2 ;;
    esac
    # show-ref の終了コード 1 は、参照が無い場合と壊れた参照（内容が不正）の両方で返り得る。
    # for-each-ref（packed-refs も見る）で、一致する参照が 0 件・警告なし・読み取りの成功を確かめたときだけ
    # 未作成ブランチとして扱う。壊れた参照は警告（標準エラー）か完全一致の行として現れ、検査不能（2）にする。
    if [ "${rc}" -eq 1 ]; then
      ref_found=0
      if g for-each-ref --format='%(refname)' "${head_ref}" > "${tmp_dir}/ref-list" 2> "${tmp_dir}/ref-err" && [ ! -s "${tmp_dir}/ref-err" ]; then
        while IFS= read -r ref_line || [ -n "${ref_line}" ]; do
          if [ "${ref_line}" = "${head_ref}" ]; then
            ref_found=1
          fi
        done < "${tmp_dir}/ref-list"
        if [ "${ref_found}" -eq 0 ]; then
          return 0
        fi
      fi
    fi
  fi
  echo "secret-scan: cannot read HEAD; aborting" >&4
  exit 2
}
# parse_git_lock <head|index> <オブジェクト指定> <複写先>: git のオブジェクトの skills-lock.json を大きさの確認後に複写して解析する。
# 第 1 引数は読み取り元（HEAD のコミット済みの一覧か、固定した index の一覧か）で、失敗の文言だけを切り替える
# （固定の文言。オブジェクト指定・パスは出さない）。取得の失敗・非数値・上限超過は検査不能（2）。解析結果は parsed_names に入る。
parse_git_lock() {
  local size side="$1" what
  shift
  case "${side}" in
    head) what="committed" ;;
    index) what="staged" ;;
    *) lock_fail ;;
  esac
  if ! size="$(g cat-file -s "$1" 2> /dev/null)" || [[ ! "${size}" =~ ^[0-9]+$ ]]; then
    echo "secret-scan: cannot read the ${what} skills-lock.json; aborting" >&4
    exit 2
  fi
  if [ "${size}" -gt "${lock_max_bytes}" ]; then
    lock_too_large
  fi
  if ! g cat-file blob "$1" > "$2" 2> /dev/null; then
    echo "secret-scan: cannot read the ${what} skills-lock.json; aborting" >&4
    exit 2
  fi
  parse_lock "$2"
}
# load_index_lock: 走査対象の index の skills-lock.json を検証し、名前を index_names（" a b " 形式）へ入れる。
# 一覧が index に無ければ index_names は空（" "）。通常ファイル（100644・100755）・単一の stage 0 以外は検査不能（2）。
index_names=" "
load_index_lock() {
  local entry mode rest oid ita_head ita_path ita_more ita_re
  index_names=" "
  # 標準エラーは抑止しない（--summary では fd 2 が捨てられる。通常モードは git の診断を残す）。
  if ! g ls-files -s -z -- skills-lock.json > "${tmp_dir}/index-entry"; then
    echo "secret-scan: cannot read the staged skills-lock.json; aborting" >&4
    exit 2
  fi
  if [ ! -s "${tmp_dir}/index-entry" ]; then
    return 0
  fi
  entry=""
  {
    IFS= read -r -d '' entry || true
    # 2 件目があれば（競合の stage 1〜3）検査不能。
    IFS= read -r -d '' rest || true
    [ -z "${rest}" ] || lock_fail
  } < "${tmp_dir}/index-entry"
  # 形は "<mode> <oid> <stage>\t<path>"。stage 0 の 1 件だけを受ける（競合・複数件は検査不能）。
  mode="${entry%% *}"
  rest="${entry#* }"
  oid="${rest%% *}"
  case "${mode}" in
    100644 | 100755) ;;
    *) lock_fail ;;
  esac
  # git add -N（intent-to-add）の項目は、ls-files -s が空 blob の stage 0 として返す。実際には登録されていないため
  # 未登録（一覧なし）として扱う。判定は固定した index だけで行い、作業ツリーを見ない（add -N の後に作業ツリーから
  # 消した・書き換えた場合も結果を変えない。#6000 の index の固定・#6011）。空の tree との cached の差分で、
  # intent-to-add の項目は --ita-invisible-in-index では現れず、--ita-visible-in-index では元の側が mode 000000 の
  # 追加として現れる。実際にステージされた空ファイルは前者にも現れるため、従来どおり解析へ進んで拒否する。
  # 両者の結果が食い違う（どちらにも現れない等）ときは検査不能（2）にする。
  if [ "${oid}" = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391" ] \
    || [ "${oid}" = "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813" ]; then
    if ! g diff --cached --raw -z --no-abbrev --no-renames --ita-invisible-in-index "${empty_tree}" -- skills-lock.json > "${tmp_dir}/index-ita"; then
      echo "secret-scan: cannot read the staged skills-lock.json; aborting" >&4
      exit 2
    fi
    if [ ! -s "${tmp_dir}/index-ita" ]; then
      if ! g diff --cached --raw -z --no-abbrev --no-renames --ita-visible-in-index "${empty_tree}" -- skills-lock.json > "${tmp_dir}/index-ita"; then
        echo "secret-scan: cannot read the staged skills-lock.json; aborting" >&4
        exit 2
      fi
      ita_head=""
      ita_path=""
      ita_more=""
      {
        IFS= read -r -d '' ita_head || true
        IFS= read -r -d '' ita_path || true
        IFS= read -r -d '' ita_more || true
      } < "${tmp_dir}/index-ita"
      # --no-abbrev で oid を全桁に固定する（core.abbrev・GIT_PRINT_SHA1_ELLIPSIS で出力の形を変えない）。
      ita_re='^:000000 100(644|755) 0{40}(0{24})? [0-9a-f]{40}([0-9a-f]{24})? A$'
      if [[ "${ita_head}" =~ ${ita_re} ]] \
        && [ "${ita_path}" = skills-lock.json ] && [ -z "${ita_more}" ]; then
        return 0
      fi
      lock_fail
    fi
  fi
  idx_re='^[0-9a-f]{40}([0-9a-f]{24})?[[:space:]]0'
  idx_re="${idx_re}"$'\t''skills-lock\.json$'
  [[ "${rest}" =~ ${idx_re} ]] || lock_fail
  parse_git_lock index "${oid}" "${tmp_dir}/index-lock"
  local n
  for n in ${parsed_names[@]+"${parsed_names[@]}"}; do
    index_names="${index_names}${n} "
  done
}
# 登録を外した名前（#6054。staged の本文の走査だけで使う）。除外の側（skill_names）には入れず、走査を足す側にだけ使う。
# staged の本文は HEAD との差分の追加行だけなので、skills-lock.json だけの変更では、除外から外れた名前の既存の本文が
# 差分に出ない（fail-open）。外れた名前の `.agents/skills/<名前>/` は、固定した index の全行を空の tree との差分で別に走査する。
dropped_names=()
dropped_cand=" "
# set_dropped: 候補（dropped_cand。空白区切り）のうち、除外（skill_names）に残っていない名前を dropped_names に集める。
# 名前は parse_lock で `[a-z0-9-]` に検証済みのものだけ。重複は除く。
set_dropped() {
  local c in_skill
  dropped_names=()
  for c in ${dropped_cand}; do
    in_skill=0
    for n in ${skill_names[@]+"${skill_names[@]}"}; do
      if [ "${n}" = "${c}" ]; then
        in_skill=1
      fi
    done
    if [ "${in_skill}" -eq 0 ]; then
      case " ${dropped_names[*]+${dropped_names[*]}} " in
        *" ${c} "*) ;;
        *) dropped_names+=("${c}") ;;
      esac
    fi
  done
}
load_skill_names() {
  local repo_real rc name n head_names=" " keep head_ref head_entry head_mode work_present=0 lock_pid lock_wd lock_rc
  local -a work_names
  work_names=()
  if [ -e "${detector_lock}" ] || [ -L "${detector_lock}" ]; then
    # シンボリックリンクは -f・-r がリンク先を検査するため明示的に拒否する（読み取り元を検出器の repo の
    # 通常ファイルに限る。.claude/rules/git-hooks.md の (f)）。
    if [ -L "${detector_lock}" ] || [ ! -f "${detector_lock}" ] || [ ! -r "${detector_lock}" ]; then
      lock_fail
    fi
    # 確認と open の間の差し替えへの対策。FIFO への差し替えで open が読み手の到着待ちで止まり続けないよう、
    # 読み取りを別プロセス（head）で行い、時間を区切って打ち切る（待ち時間の制限。超過・失敗は 2）。
    # open はその別プロセスの中で行うため、止まっても kill で終わらせられる。読み取り専用のため権限・読み取り専用マウントも問題ない。
    # 読み終えた後に、パスがリンクでない通常ファイルのままであることを再確認する（違反は 2）。
    # /dev/fd/N による同一性の確認は、macOS の /dev/fd が fd の実体を指さず使えないため置かない。
    # 上限 + 1 バイトだけ複写する（超過は parse_lock が固定の診断で止める）。
    head -c $((lock_max_bytes + 1)) 2> /dev/null < "${detector_lock}" > "${tmp_dir}/work-lock" &
    lock_pid=$!
    # 監視は TERM を受けたら起動済みの sleep も終わらせて抜ける（読み終えた後に sleep が最大 5 秒残らないようにする）。
    # 監視の起動の間だけ EXIT の trap（一時ディレクトリの削除）を外す。起動直後（子が trap を整える前）に TERM が届くと、
    # 子は親から継いだ EXIT の trap を実行し、走査中の一時ディレクトリを消す（有効な一覧が検査不能（2）になる。#6011）。
    # 子の側で外すのでは間に合わない（子のコードが動く前に届き得る）ため、fork の前に親で外し、fork の直後に戻す。
    trap - EXIT
    (
      trap - ERR
      sleep 5 &
      _sleep_pid=$!
      trap 'kill "${_sleep_pid}" > /dev/null 2>&1 || true; wait "${_sleep_pid}" > /dev/null 2>&1 || true; exit 0' TERM
      wait "${_sleep_pid}" || true
      # 打ち切りの印を先に残す（読み取りの失敗と区別して固定の診断を出すため。#6011）。
      : > "${tmp_dir}/lock-timeout" 2> /dev/null || true
      kill "${lock_pid}" > /dev/null 2>&1 || true
    ) > /dev/null 2>&1 < /dev/null &
    lock_wd=$!
    trap cleanup_tmp EXIT
    lock_rc=0
    wait "${lock_pid}" 2> /dev/null || lock_rc=$?
    kill "${lock_wd}" > /dev/null 2>&1 || true
    wait "${lock_wd}" > /dev/null 2>&1 || true
    if [ "${lock_rc}" -ne 0 ]; then
      if [ -e "${tmp_dir}/lock-timeout" ]; then
        # 読み取りの打ち切り（5 秒）。解析の拒否とは別の固定の診断で、検査不能（2）にする（値・パスは出さない）。
        echo "secret-scan: reading skills-lock.json timed out; aborting" >&4
        exit 2
      fi
      lock_fail
    fi
    if [ -L "${detector_lock}" ] || [ ! -f "${detector_lock}" ]; then
      lock_fail
    fi
    parse_lock "${tmp_dir}/work-lock"
    work_names=(${parsed_names[@]+"${parsed_names[@]}"})
    work_present=1
  fi
  # 走査対象の側（index）にも skills-lock.json がある。走査対象の一覧は除外を広げる用途には使わない（PR・未コミットの
  # 編集で名前を足して検査を免れないため）。使うのは次の 2 つだけ。
  #   - 検証: 通常ファイル・大きさ・構文・対応形式を確かめ、違反は検査不能（2）にする（マージ前に壊れた一覧を見つける）。
  #   - 除外を狭める: 同じ checkout では、検査対象の index で登録が消えた名前の除外を取り消す
  #     （登録の削除をステージしたあとに作業ツリーだけへ登録を戻して除外を残す経路を塞ぐ）。
  # 一覧が index に無い（走査対象に skills-lock.json が無い）ときの検証は行わない。
  if ! repo_real="$(pwd -P && printf x)"; then
    echo "secret-scan: cannot resolve the repository root; aborting" >&4
    exit 2
  fi
  repo_real="${repo_real%x}"
  repo_real="${repo_real%$'\n'}"
  load_index_lock
  if [ "${repo_real}" != "${detector_root}" ]; then
    # 別の checkout（CI で PR の head を走査する形）でも、信頼済みの一覧（検出器側）と走査対象の index の一覧の
    # 共通部分だけを除外に使う。PR が登録を削除した名前は、マージ前に本文を走査する（除外が縮む方向にだけ効く）。
    # 対象の index に一覧が無いときは index_names が空のため、除外は 0 件になる。
    skill_names=()
    for name in ${work_names[@]+"${work_names[@]}"}; do
      case "${index_names}" in
        *" ${name} "*) skill_names+=("${name}") ;;
      esac
    done
    # 検出器の一覧にあり、対象の index で外れた名前は全行を走査する（HEAD の実際の除外より広いが、走査を増やす向き）。
    dropped_cand=" ${work_names[*]+${work_names[*]}} "
    set_dropped
    return 0
  fi
  # 検出器と走査対象が同じ checkout（ローカルの pre-commit・make secret-scan）のとき、作業ツリーの
  # skills-lock.json は走査対象の側にもあり、未コミットの編集で除外を足せてしまう（走査対象から読まない規則の
  # ローカル版。.claude/rules/git-hooks.md の (f)）。作業ツリー・HEAD・index の 3 つすべてに登録された名前だけを使う
  # （常に作業ツリーの一覧の部分集合で、除外を広げない）。
  # 作業ツリーに一覧が無くても HEAD の一覧は検証する（壊れていれば 2）。
  # 未作成ブランチ（参照が無い）・HEAD に一覧が無いときは除外なし。git の失敗・参照の解決失敗は検査不能（2）。
  resolve_head
  if [ -z "${head_id}" ]; then
    return 0
  fi
  # HEAD の一覧が読めない・壊れているときは検査不能（2）で止める（除外なしで続行する等の方式は人が決める）。
  if ! g ls-tree -z "${head_id}" -- skills-lock.json > "${tmp_dir}/head-entry" 2> /dev/null; then
    echo "secret-scan: cannot read the committed skills-lock.json; aborting" >&4
    exit 2
  fi
  if [ ! -s "${tmp_dir}/head-entry" ]; then
    return 0
  fi
  # HEAD 側も通常ファイル（100644・100755）だけを受ける。シンボリックリンク・submodule 等は検査不能（2）。
  head_entry=""
  IFS= read -r -d '' head_entry < "${tmp_dir}/head-entry" || true
  head_mode="${head_entry%% *}"
  case "${head_mode}" in
    100644 | 100755) ;;
    *) lock_fail ;;
  esac
  parse_git_lock head "${head_id}:skills-lock.json" "${tmp_dir}/head-lock"
  for n in ${parsed_names[@]+"${parsed_names[@]}"}; do
    head_names="${head_names}${n} "
  done
  # 登録を外した名前の候補は、HEAD の一覧の名前と作業ツリーの一覧の名前（#6054）。
  # 作業ツリーの一覧が無い（git rm 等）ときも、HEAD の名前は全行を走査する。
  dropped_cand="${head_names}${work_names[*]+${work_names[*]}} "
  if [ "${work_present}" -eq 0 ]; then
    set_dropped
    return 0
  fi
  skill_names=()
  for name in ${work_names[@]+"${work_names[@]}"}; do
    keep=0
    case "${head_names}" in
      *" ${name} "*) keep=1 ;;
    esac
    case "${index_names}" in
      *" ${name} "*) ;;
      *) keep=0 ;;
    esac
    if [ "${keep}" -eq 1 ]; then
      skill_names+=("${name}")
    fi
  done
  set_dropped
}
load_skill_names
# staged の差分の基準も同じ ID にする（別の checkout でまだ解決していなければここで解決する）。
if [ "${mode}" = staged ] && [ "${head_resolved}" -eq 0 ]; then
  resolve_head
fi

# 本文の走査の pathspec。`-- .` で始めて空にしない（bash 3.2 は set -u で空配列を展開できない）。
# 名前ごとに literal・top・末尾 `/`（名前ちょうどの blob・前方一致の別名は外さない）で外す。
body_pathspec=(-- .)
for name in ${skill_names[@]+"${skill_names[@]}"}; do
  body_pathspec+=(":(exclude,literal,top).agents/skills/${name}/")
done
# 登録を外した名前（#6054）は staged で HEAD との差分から外し、空の tree との差分（固定した index の全行）で別に走査する
# （二重に数えない）。tracked・未作成ブランチは元々全行の差分なので使わない。
dropped_pathspec=(--)
dropped_count=0
if [ "${mode}" = tracked ] || [ -z "${head_id}" ]; then
  dropped_names=()
fi
for name in ${dropped_names[@]+"${dropped_names[@]}"}; do
  body_pathspec+=(":(exclude,literal,top).agents/skills/${name}/")
  dropped_pathspec+=(":(literal,top).agents/skills/${name}/")
  dropped_count=$((dropped_count + 1))
done

# 利用者の git 設定で出力形式が変わると追加行・ファイル名を取りこぼすため固定する。
#   --no-color: color.diff / color.ui = always で行頭に色コードが付き `+` 行を検出できなくなる
#   --no-ext-diff / --no-textconv: diff.external・textconv ドライバが本文を差し替える
#   --text: .gitattributes の binary・-diff 属性で本文が検査から外れるのを防ぐ（PR 内の属性変更で
#     検査を回避させない。SECR-23）。NUL を含むファイルも本文を走査する（fail-closed）
#   --no-renames: リネームを削除 + 追加として扱い、移動先の全行を走査する（除外パスからの移動も含む）
#   --src-prefix / --dst-prefix: diff.noprefix・diff.mnemonicPrefix でヘッダが `+++ b/` 以外になる
# --tracked では空の tree と index の差分を取る。追跡ファイルの全行が追加行になるため、
# 名前の検査・除外パス・解析ループ・規則の判定を staged モードと共用できる（解析の二重実装を作らない）。
# bash 3.2 は set -u 下で空配列の展開・長さ参照が unbound variable になるため、
# 空になりうる配列を使わず mode で分岐し、tracked のときだけ empty_tree を渡す。
# 空の tree の ID（empty_tree）は index の固定の前に解決済み。
if [ "${mode}" = tracked ] || [ -z "${head_id}" ]; then
  scan_base="${empty_tree}"
else
  scan_base="${head_id}"
fi
scan_diff_base() {
  local base="$1"
  shift
  g diff --cached --no-color --no-ext-diff --no-textconv --text --no-renames \
    --src-prefix=a/ --dst-prefix=b/ "${base}" "$@"
}
scan_diff() {
  scan_diff_base "${scan_base}" "$@"
}

# git の出力は一度ファイルへ書き出してから読む。`done <<< "$(git ...)"` やパイプでは git の失敗が
# set -e / pipefail に掛からず「対象なし」として合格してしまう（fail-open）。

# 検出の集計。規則ごとの件数（rule_hits の添字は rule_ids と同じ）・env-file の件数・該当ファイル。
rule_hits=()
for ((i = 0; i < rule_count; i++)); do
  rule_hits+=(0)
done
env_hits=0
flagged_files=""
flagged_count=0
add_flagged() {
  if [[ "${flagged_files}" != *"  $1"$'\n'* ]]; then
    flagged_files+="  $1"$'\n'
    flagged_count=$((flagged_count + 1))
  fi
}

# 1. .env 系ファイル。-z で受け、非 ASCII を含むパスが core.quotePath で引用符付き・
#    8 進エスケープになり末尾一致しなくなるのを防ぐ。
# リビジョンの後ろは `--` で区切る（リビジョンと同名のパスがあっても、引数をパスと取り違えて止まらない。本文の側は body_pathspec が `--` で始まる）。
if ! scan_diff --name-only -z --diff-filter=ACMR -- > "${tmp_dir}/names"; then
  echo "secret-scan: failed to list files; aborting" >&4
  exit 2
fi

checked_files=0
while IFS= read -r -d '' path; do
  [ -z "${path}" ] && continue
  checked_files=$((checked_files + 1))
  if [[ "${path}" =~ (^|/)\.env(\..+)?$ ]] && ! [[ "${path}" =~ \.(example|sample|template)$ ]]; then
    env_hits=$((env_hits + 1))
    add_flagged "${path}"
  fi
done < "${tmp_dir}/names"

# 取り違えで「対象なし」を合格にしないため、追跡ファイルがあるのに対象が 0 件なら止める。
if [ "${mode}" = tracked ] && [ "${checked_files}" -eq 0 ]; then
  if ! g ls-files -z > "${tmp_dir}/ls"; then
    echo "secret-scan: failed to list tracked files; aborting" >&4
    exit 2
  fi
  if [ -s "${tmp_dir}/ls" ]; then
    echo "secret-scan: tracked files exist but none were selected for scanning; aborting" >&4
    exit 2
  fi
fi

# 2. 追加行のみを走査する（削除行を対象にすると漏洩済み値の除去コミットを妨げる）
if ! scan_diff -U0 --diff-filter=ACMR "${body_pathspec[@]}" > "${tmp_dir}/diff"; then
  echo "secret-scan: failed to read diff; aborting" >&4
  exit 2
fi
# 登録を外した名前の既存の本文（#6054）。外れた名前があるときだけ呼ぶ（正の pathspec が空だと index 全体になる）。
# 区画は `diff --git` で始まるため、上の差分の後ろへ足せば以降の解析・集計を共用できる。
if [ "${dropped_count}" -gt 0 ]; then
  if ! scan_diff_base "${empty_tree}" -U0 --diff-filter=ACMR "${dropped_pathspec[@]}" >> "${tmp_dir}/diff"; then
    echo "secret-scan: failed to read diff; aborting" >&4
    exit 2
  fi
fi

# bash の read は NUL を保持できず（実装により NUL 以降を切る・NUL だけを捨てる）、NUL を含む行で
# 後ろの秘密値や NUL で分割された語を見落とし得る（fail-open）。走査の前に NUL を取り除いた版と
# 空白へ置き換えた版を作り、NUL を含む場合は両方を走査する（行番号は同じ。同じ行は二重に数えない）。
if ! tr -d '\000' < "${tmp_dir}/diff" > "${tmp_dir}/diff.del"; then
  echo "secret-scan: failed to normalize diff; aborting" >&4
  exit 2
fi
if ! tr '\000' ' ' < "${tmp_dir}/diff" > "${tmp_dir}/diff.sp"; then
  echo "secret-scan: failed to normalize diff; aborting" >&4
  exit 2
fi

# scan_pass <file>: `+++ b/<path>` のファイルヘッダは `diff --git` から最初の `@@` までの間でだけ解釈する。
# hunk 内の追加行の本文が `++ b/...`・`++...` で始まると diff 上は `+++...` になるため、
# 行頭だけで判定するとヘッダと誤認して走査から漏れる。ヘッダ区間以外の `+` 行はすべて走査する。
# 再走査が要る（NUL がある）ときだけ、1 回目に検出した行番号を seen に記録する（bash 3.2 の添字配列）。
seen=()
record_seen=0
scan_pass() {
  local in_header=0 current_file="" line lineno=0
  while IFS= read -r line || [ -n "${line}" ]; do
    lineno=$((lineno + 1))
    case "${line}" in
      'diff --git '*)
        in_header=1
        current_file=""
        continue
        ;;
      '@@'*)
        in_header=0
        continue
        ;;
    esac
    if [ "${in_header}" -eq 1 ]; then
      case "${line}" in
        '+++ "b/'*)
          # 制御文字等を含むパスは git が二重引用符で囲む（表示用に外すのみ）
          current_file="${line#+++ \"b/}"
          current_file="${current_file%\"}"
          ;;
        '+++ b/'*)
          # 空白を含むパスには git が末尾にタブを付ける
          current_file="${line#+++ b/}"
          current_file="${current_file%$'\t'}"
          ;;
      esac
      continue
    fi
    case "${line}" in
      '+'*)
        if [ -n "${seen[lineno]:-}" ]; then
          continue
        fi
        if classify_line "${line}"; then
          if [ "${record_seen}" -eq 1 ]; then
            seen[lineno]=1
          fi
          rule_hits[hit]=$((rule_hits[hit] + 1))
          add_flagged "${current_file:-(unknown)}"
        fi
        ;;
    esac
  done < "$1"
}
# cmp の終了状態は 0 = 同じ（NUL なし）/ 1 = 違う（NUL あり）/ 2 以上 = エラー。後者は検査不能として止める。
cmp_rc=0
cmp -s "${tmp_dir}/diff" "${tmp_dir}/diff.del" || cmp_rc=$?
case "${cmp_rc}" in
  0)
    scan_pass "${tmp_dir}/diff.del"
    ;;
  1)
    record_seen=1
    scan_pass "${tmp_dir}/diff.del"
    record_seen=0
    scan_pass "${tmp_dir}/diff.sp"
    ;;
  *)
    echo "secret-scan: failed to compare normalized diff (cmp exit ${cmp_rc}); aborting" >&4
    exit 2
    ;;
esac

# 走査中に元の index の内容が変わっていたら、固定した複写に基づく結果は実際の内容と食い違う。合格を返さず検査不能（2）にする。
# 終了時も元の index を固定時と同じ方法で 1 回だけ複写し、2 つの一覧（ls-files -s と intent-to-add の印）を同じ複写から取る。
# 元の index を一覧ごとに開くと、2 つの呼び出しの間の更新で 2 つの一覧が別の版を見て、どちらの版とも違う組で一致し得る（#6011）。
# git は index を lock ファイルの rename で置き換えるため、cp は 1 つの版を読む。元の index が無いときは存在しない複写のパスを
# 指し（空として扱う）。固定時にあった index が終了時に無ければ、一覧の差とは別に有無の記録（index_pinned）で 2 にする
# （登録 0 件の index は一覧が空のままのため）。複写の失敗は 2。固定したパスが終了時に通常ファイルでなく（FIFO・リンク等に）
# 替わっていれば、複写せず 2。リンクは開始時も終了時も辿らず 2（解く前のパスで判定する）。
after_index="${tmp_dir}/index-after"
if index_is_link; then
  # 走査中にリンクへ替わった（固定時はリンクでなかった）。辿らず 2。
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
if [ -e "${orig_index}" ]; then
  if [ ! -f "${orig_index}" ]; then
    echo "secret-scan: cannot snapshot the index; aborting" >&4
    exit 2
  fi
  if ! cp -- "${orig_index}" "${after_index}" 2> /dev/null; then
    echo "secret-scan: cannot snapshot the index; aborting" >&4
    exit 2
  fi
elif [ "${index_pinned}" -eq 1 ]; then
  # 固定時にあった index が無い。登録 0 件の index は一覧が空のまま変わらないため、有無そのものを見て止める。
  echo "secret-scan: the index changed during the scan; aborting" >&4
  exit 2
fi
if ! GIT_INDEX_FILE="${after_index}" index_listing "${tmp_dir}/index-listing-after" 2> /dev/null; then
  echo "secret-scan: cannot snapshot the index; aborting" >&4
  exit 2
fi
index_cmp_rc=0
cmp -s "${tmp_dir}/index-listing-before" "${tmp_dir}/index-listing-after" || index_cmp_rc=$?
if [ "${index_cmp_rc}" -ne 0 ]; then
  echo "secret-scan: the index changed during the scan; aborting" >&4
  exit 2
fi

# 走査中に HEAD が動いていたら、固定した ID を基準にした結果は古い。合格を返さず検査不能（2）にする
# （未作成ブランチ・空の ID のまま始まり、走査中にブランチが作られた場合も違いに含む）。
# 取り直しは終了時の index の検証より後、集計の直前に置く（検証の途中に動いた HEAD も見落とさない。git-hooks.md の (f)。#6011）。
if [ "${head_resolved}" -eq 1 ]; then
  pinned_head="${head_id}"
  resolve_head
  if [ "${head_id}" != "${pinned_head}" ]; then
    echo "secret-scan: HEAD changed during the scan; aborting" >&4
    exit 2
  fi
fi

# 合計件数で判定する。出力は規則 ID ごとの件数・合計・ファイル名だけで、値は出さない。
total="${env_hits}"
for ((i = 0; i < rule_count; i++)); do
  total=$((total + rule_hits[i]))
done

if [ "${total}" -gt 0 ]; then
  if [ "${mode}" = tracked ]; then
    echo "secret-scan: findings in tracked files" >&4
  else
    echo "commit blocked: staged changes look like they contain a secret" >&4
  fi
  if [ "${env_hits}" -gt 0 ]; then
    echo "secret-scan: env-file: ${env_hits} file(s)" >&4
  fi
  for ((i = 0; i < rule_count; i++)); do
    if [ "${rule_hits[i]}" -gt 0 ]; then
      echo "secret-scan: ${rule_ids[i]}: ${rule_hits[i]} line(s)" >&4
    fi
  done
  echo "secret-scan: total: ${total} finding(s) in ${flagged_count} file(s)" >&4
  if [ "${summary}" -eq 0 ]; then
    printf '%s' "${flagged_files}" >&4
  fi
  echo "replace values with a placeholder / env var reference, or unstage the file (\`git restore --staged <file>\`)." >&4
  exit 1
fi

if [ "${mode}" = tracked ]; then
  echo "secret-scan: total: 0 finding(s) (${checked_files} tracked file(s) checked)"
fi
exit 0
