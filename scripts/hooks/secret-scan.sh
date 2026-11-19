#!/usr/bin/env bash
# 秘密情報の混入検査（TASK-6。規則の定義点はこのファイル 1 か所）。
#
# 呼び出し:
#   secret-scan.sh            staged 差分の追加行を走査する（lefthook pre-commit から呼ばれる）
#   secret-scan.sh --tracked  追跡済みファイル全体を走査する（`make secret-scan`。CI の secret-scan ジョブ）
#   それ以外の引数は usage を出して終了コード 2。検査を外す引数・環境変数・除外リストは置かない。
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
# 走査範囲: 追加行のみ（削除行は漏洩済み値の除去コミットを妨げないため対象外）。
# .agents/skills/・.claude/skills/・*.lock は対象外（skills-lock.json 管理の外部取得物・ハッシュ列）。
# 範囲外で検出力が無いもの: コミット履歴・submodule の中・バイナリ・規則に無い形（長い 16 進や base64 の塊単体）。
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

mode=staged
case "$#:${1:-}" in
  0:) ;;
  1:--tracked) mode=tracked ;;
  *)
    echo "usage: secret-scan.sh [--tracked]" >&2
    exit 2
    ;;
esac

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
      echo "secret-scan: regex evaluation failed (rc=${rc}); aborting" >&2
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
  echo "secret-scan: self-test failed (regex engine / locale mismatch?); aborting" >&2
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
if ! repo_root="$(git rev-parse --show-toplevel)" || [ -z "${repo_root}" ]; then
  echo "secret-scan: not inside a git work tree; aborting" >&2
  exit 2
fi
cd "${repo_root}"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/secret-scan.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

# 利用者の git 設定で出力形式が変わると追加行・ファイル名を取りこぼすため固定する。
#   --no-color: color.diff / color.ui = always で行頭に色コードが付き `+` 行を検出できなくなる
#   --no-ext-diff / --no-textconv: diff.external・textconv ドライバが本文を差し替える
#   --no-renames: リネームを削除 + 追加として扱い、移動先の全行を走査する（除外パスからの移動も含む）
#   --src-prefix / --dst-prefix: diff.noprefix・diff.mnemonicPrefix でヘッダが `+++ b/` 以外になる
# --tracked では空の tree と index の差分を取る。追跡ファイルの全行が追加行になるため、
# 名前の検査・除外パス・解析ループ・規則の判定を staged モードと共用できる（解析の二重実装を作らない）。
# bash 3.2 は set -u 下で空配列の展開・長さ参照が unbound variable になるため、
# 空になりうる配列を使わず mode で分岐し、tracked のときだけ empty_tree を渡す。
empty_tree=""
if [ "${mode}" = tracked ]; then
  if ! empty_tree="$(git hash-object -t tree --stdin < /dev/null)" || [ -z "${empty_tree}" ]; then
    echo "secret-scan: failed to resolve the empty tree; aborting" >&2
    exit 2
  fi
fi
scan_diff() {
  if [ "${mode}" = tracked ]; then
    git diff --cached --no-color --no-ext-diff --no-textconv --no-renames \
      --src-prefix=a/ --dst-prefix=b/ "${empty_tree}" "$@"
  else
    git diff --cached --no-color --no-ext-diff --no-textconv --no-renames \
      --src-prefix=a/ --dst-prefix=b/ "$@"
  fi
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
if ! scan_diff --name-only -z --diff-filter=ACMR > "${tmp_dir}/names"; then
  echo "secret-scan: failed to list files; aborting" >&2
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
  if ! git ls-files -z > "${tmp_dir}/ls"; then
    echo "secret-scan: failed to list tracked files; aborting" >&2
    exit 2
  fi
  if [ -s "${tmp_dir}/ls" ]; then
    echo "secret-scan: tracked files exist but none were selected for scanning; aborting" >&2
    exit 2
  fi
fi

# 2. 追加行のみを走査する（削除行を対象にすると漏洩済み値の除去コミットを妨げる）
if ! scan_diff -U0 --diff-filter=ACMR -- . \
  ':(exclude).agents/skills/**' ':(exclude).claude/skills/**' ':(exclude)*.lock' \
  > "${tmp_dir}/diff"; then
  echo "secret-scan: failed to read diff; aborting" >&2
  exit 2
fi

# `+++ b/<path>` のファイルヘッダは `diff --git` から最初の `@@` までの間でだけ解釈する。
# hunk 内の追加行の本文が `++ b/...`・`++...` で始まると diff 上は `+++...` になるため、
# 行頭だけで判定するとヘッダと誤認して走査から漏れる。ヘッダ区間以外の `+` 行はすべて走査する。
in_header=0
current_file=""
while IFS= read -r line || [ -n "${line}" ]; do
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
      if classify_line "${line}"; then
        rule_hits[hit]=$((rule_hits[hit] + 1))
        add_flagged "${current_file:-(unknown)}"
      fi
      ;;
  esac
done < "${tmp_dir}/diff"

# 合計件数で判定する。出力は規則 ID ごとの件数・合計・ファイル名だけで、値は出さない。
total="${env_hits}"
for ((i = 0; i < rule_count; i++)); do
  total=$((total + rule_hits[i]))
done

if [ "${total}" -gt 0 ]; then
  if [ "${mode}" = tracked ]; then
    echo "secret-scan: findings in tracked files" >&2
  else
    echo "commit blocked: staged changes look like they contain a secret" >&2
  fi
  if [ "${env_hits}" -gt 0 ]; then
    echo "secret-scan: env-file: ${env_hits} file(s)" >&2
  fi
  for ((i = 0; i < rule_count; i++)); do
    if [ "${rule_hits[i]}" -gt 0 ]; then
      echo "secret-scan: ${rule_ids[i]}: ${rule_hits[i]} line(s)" >&2
    fi
  done
  echo "secret-scan: total: ${total} finding(s) in ${flagged_count} file(s)" >&2
  printf '%s' "${flagged_files}" >&2
  echo "replace values with a placeholder / env var reference, or unstage the file (\`git restore --staged <file>\`)." >&2
  exit 1
fi

if [ "${mode}" = tracked ]; then
  echo "secret-scan: total: 0 finding(s) (${checked_files} tracked file(s) checked)"
fi
exit 0
