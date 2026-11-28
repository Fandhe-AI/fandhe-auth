#!/usr/bin/env bash
# human-result-lib.sh — human-result.sh の判定規則と封筒の payload 組み立てだけを持つ純関数と定数（source 専用）
#
# 呼び出し元: human-result.sh（Issue #932）。反映の issue の前提にある人の作業（owner:human）の issue から、
#             「結果:」で始まる報告のコメントを決まった規則で読む。以前は読み方を反映の issue の本文へ文章で書いただけで、
#             Plan が言い換えて要約するため判定の式が一字一句のまま実行される保証が無かった。
# 役割: 1) gh api の --jq に渡すフィルタ（HR_ISSUE_JQ / HR_COMMENTS_JQ）、2) 外部由来の値の検査（番号・login・id）、
#       3) 入力の束縛 inputSha256、4) 封筒の payload（キーを昇順に並べた空白なしの JSON。整数と ASCII の固定文字列だけ）の組み立て。
# エンジンに依存しない規則: gh api の --jq は gojq、手元の jq は 1.7 で、正規表現の文字クラス（[[:space:]] など）に Unicode の空白が
#       入るかどうかが違う。そのためフィルタは正規表現関数・文字クラス・バックスラッシュ記法を使わず、
#       explode / implode によるコードポイントの比較と startswith / rtrimstr / split（文字列リテラルでの分割）だけで書く。
#       tests/human-result-script.test.mjs が、フィルタに禁止の構文が無いことを静的に検査する。
# 検出は緩く、成功の受理は厳密にする: 1 行目が空白・装飾（SP と DEC）を除いて「結果」+ コロンで始まるものはすべて報告とみなし、
#       生の 1 行目が「結果: 成功」「結果: 失敗」「結果: 未実施」と完全一致するときだけそれぞれ success / failure / not-done にする。
#       それ以外（先頭のタブ・Markdown の装飾・BOM・Unicode の空白・全角コロン・末尾の空白・前置きの空行）は malformed。
#       書式の崩れた失敗報告が「報告ではない」と読まれ、より古い成功が採られるのを防ぐ。
# 契約: どの関数も exit せず、gh・ネットワークを呼ばない。結果は stdout と return 値で返す。
#       source 元のシェル状態を壊さないため set -e / set -u / trap / exit は置かない。二重 source は無害。signed-output.sh（so_sha256）を先に source しておくこと。
# 互換: macOS（BSD userland・bash 3.2）と Linux。連想配列・mapfile・GNU 専用フラグを使わない。呼び出し側が LC_ALL=C にしている前提。
#
# このファイルは単体実行しない。run-verified.sh のハッシュ照合は scripts/lib/*.sh も対象に含む。

# 空白の集合 SP（コードポイントを明示する）: U+0009 U+000B U+000C U+0020 U+0085 U+00A0 U+1680 U+2000-U+200D U+2028 U+2029 U+202F U+205F U+2060 U+3000 U+FEFF。
#   改行（U+000A）と CR（U+000D）は行の区切りとして別に扱う。
# 装飾の集合 DEC: > # * _ ` ~ - + |
# コロンの集合: U+003A（:）U+FF1A（：）U+FE55（﹕）U+FE13（︓）
HR_DEFS='def SP: [9,11,12,32,133,160,5760,8192,8193,8194,8195,8196,8197,8198,8199,8200,8201,8202,8203,8204,8205,8232,8233,8239,8287,8288,12288,65279];
def DEC: [62,35,42,95,96,126,45,43,124];
def COLON: [58,65306,65109,65043];
def skipc: . as $x | (SP | any(.[]; . == $x)) or (DEC | any(.[]; . == $x));
def stripl: explode | until(length == 0 or ((.[0] | skipc) | not); .[1:]) | implode;
def lines: split("\n") | map(rtrimstr("\r"));
def kind: (lines | .[0]) as $raw
  | (first(lines[] | select(stripl != "")) | stripl) as $j
  | if ($j | startswith("結果")) and (($j | ltrimstr("結果") | stripl | explode | .[0]) as $c | $c != null and (COLON | any(.[]; . == $c)))
    then (if $raw == "結果: 成功" then "success" elif $raw == "結果: 失敗" then "failure" elif $raw == "結果: 未実施" then "not-done" else "malformed" end)
    else empty end;'

# issue 1 件の要約を「PR か・state・owner:human が付いているか・assignee の login のカンマ区切り」の 1 行（空白区切り）で出す。
# login の列は L: を前置して空でも欄が詰まらないようにする。
# ラベル名は ASCII の大文字小文字を区別しない（GitHub のラベル名の扱いと create-issue-tree の照合に揃える）。
# ascii_downcase は静的検査で禁止しているので、A〜Z だけをコードポイントで a〜z へ畳む。文字列でない名前は一致しない扱い。
HR_ISSUE_JQ='"\(.pull_request != null) \(.state) \(any(.labels[]?; (if type == "object" then .name else . end) | if type == "string" then (explode | map(if . >= 65 and . <= 90 then . + 32 else . end) | implode) == "owner:human" else false end)) L:\([.assignees[]? | .login | select(type == "string")] | join(","))"'

# コメントのうち「報告とみなすもの」だけを「id login association kind」の 1 行（空白区切り）で出す（ページごとに適用される）。
# 書き手の association は OWNER / MEMBER / COLLABORATOR だけを残す（第三者のコメントを候補の数に数えない）。最新の選択はシェルが全ページの行から行う。
HR_COMMENTS_BODY='.[] | select((.body | type) == "string") | select((.user.login | type) == "string")
  | select(.author_association == "OWNER" or .author_association == "MEMBER" or .author_association == "COLLABORATOR")
  | (.body | kind) as $k | "\(.id) \(.user.login) \(.author_association) \($k)"'
HR_COMMENTS_JQ="${HR_DEFS}
${HR_COMMENTS_BODY}"

HR_NUMBERS_MAX=20
HR_NUMBERS_MAX_BYTES=400
HR_ROWS_MAX=500
HR_ID_MAX_LEN=15

# 候補の番号（先頭 0 なしの 1〜9 桁の正の整数）
hr_valid_number() {
  case "$1" in
    '' | *[!0-9]* | 0*) return 1 ;;
  esac
  [ "${#1}" -le 9 ]
}

# --numbers の形式: 番号のカンマ区切りで狭義の昇順・1〜HR_NUMBERS_MAX 件。
hr_valid_numbers_csv() {
  local n prev=0 count=0
  case "$1" in
    '' | *[!0-9,]* | ,* | *, | *,,*) return 1 ;;
  esac
  [ "${#1}" -le "${HR_NUMBERS_MAX_BYTES}" ] || return 1
  while IFS= read -r n; do
    hr_valid_number "${n}" || return 1
    [ "${n}" -gt "${prev}" ] || return 1
    prev=${n}
    count=$((count + 1))
  done <<HR_EOF
$(printf '%s\n' "$1" | tr ',' '\n')
HR_EOF
  [ "${count}" -ge 1 ] && [ "${count}" -le "${HR_NUMBERS_MAX}" ]
}

# GitHub の login（英数字・ハイフン・アンダースコア〔EMU の name_shortcode〕1〜39 文字。App は末尾に [bot]）
hr_valid_login() {
  local l=${1%'[bot]'}
  case "${l}" in
    '' | *[!A-Za-z0-9_-]*) return 1 ;;
  esac
  [ "${#l}" -le 39 ]
}

# コメント id（先頭 0 なしの 1〜HR_ID_MAX_LEN 桁の正の整数。JS の安全な整数に収める）
hr_valid_id() {
  case "$1" in
    '' | *[!0-9]* | 0*) return 1 ;;
  esac
  [ "${#1}" -le "${HR_ID_MAX_LEN}" ]
}

# $1 が $2 より大きい id なら 0（桁数 → 辞書順。どちらも hr_valid_id 済み）
hr_id_gt() {
  [ "${#1}" -gt "${#2}" ] && return 0
  [ "${#1}" -lt "${#2}" ] && return 1
  [ "$1" '>' "$2" ]
}

# 入力の束縛: sha256("human-result\n" + repo + "\n" + numbers)。ホストの humanResultInputSha256 と同じ定義
hr_input_sha() {
  printf 'human-result\n%s\n%s' "$1" "$2" | so_sha256
}

# payload の 1 要素。$1=comment $2=n $3=verdict
hr_entry() {
  printf '{"comment":%s,"n":%s,"verdict":"%s"}' "$1" "$2" "$3"
}

# payload。$1=entries のカンマ連結 $2=inputSha256
hr_payload() {
  printf '{"entries":[%s],"inputSha256":"%s"}' "$1" "$2"
}
