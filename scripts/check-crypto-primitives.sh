#!/usr/bin/env bash
# 暗号の基本処理（ハッシュ・署名・鍵導出・乱数・楕円曲線演算・XML 署名）を自前で実装した箇所が 0 件であることを走査する（TASK-715 (3/4)・LIB-21）。
# DPOL-4・DPOL-47 を緩めない: 暗号は保守されている既存 crate を使い、自作しない。この走査は発見的で、人のレビュー（TASK-83）の代わりにならない（CG-2）。
# 検査を外す引数・環境変数・例外の許可リストは作らない（DPOL-30〜32）。呼び出し元: `make crypto-scan`。
# TODO(TASK-715): 4/4（#5170）で make check 相当の検査・CI から呼ぶ。
#
# 走査の対象: `git ls-files --cached --others --exclude-standard -- crates/` の全ファイル（target/ などの無視された生成物は入らない）。
#   合否に入る範囲は本番メンバー（PROD_DIRS）。試験専用メンバー（TEST_ONLY_DIRS。trial-deps/fuzz を含む）は参考の警告だけで、合否に入れない。
#   crates/ の外（.agents/skills/ の外部取得物・scripts/）は対象外。
#   分類は scripts/check-openssl-vendored.sh の PROD_PKGS・TEST_ONLY_PKGS と同じ（crate 名の接頭辞 fandhe-auth- を除いたディレクトリ名）に保つ。
#
# 規則（合否 = 1 件でも違反）:
#   C1: 既知の暗号定数（SHA-1/2・MD5 の IV と K[0]、ChaCha/Salsa の sigma、黄金比の定数、Poly1305 の clamp、Keccak の RC、P-256 の素数の断片、AES の S-box の先頭）。
#       判定の前に `_` を除き、大小文字を区別しない。
#   C2: インラインアセンブリ・アーキテクチャ固有の命令（asm!・global_asm!・naked_asm!・#[naked]・core::arch・std::arch）
#   C3: Rust 以外のネイティブのソース（*.c *.h *.cc *.cpp *.cxx *.hh *.hpp *.hxx *.inc *.cu *.m *.mm *.s *.S *.asm）が crate の中にあること
#   C4: 暗号の trait の自前の実装（RngCore・CryptoRng・BlockCipher・StreamCipher・Digest・Mac 等への impl ... for）
#   C5: 同じファイルに rotate_left/rotate_right と wrapping_add/sub/mul が併存すること（ハッシュ・ストリーム暗号のラウンドの典型）
# 参考（警告のみ。合否に入れない）:
#   N1: 定義の名前に暗号の基本処理の名前が `_` 区切りの語として入ること（既存 crate を包む関数も当たりうるので人が確かめる）
#   N2: build.rs の存在（ネイティブのコードの組み込みの入口になりうる）。本番メンバーだけ数える
#   試験専用メンバーの検出は、規則に関わらず「test-only」の警告として別に数える。
# 限界: 名前を変えた実装・10 進や別表現の定数・マクロで生成したコードは見落としうる。XML 署名は SAML の Plugin（段階 2）の側で、まだ crate が無い。
#
# 実行のたびに自己試験（陽性対照 S1〜S8・陰性対照 S9・S10）を先に流し、検出器が常に 0 を返す状態を防ぐ。
# 出力は規則 ID・件数・相対パス:行だけ（絶対パスは出さない）。終了コード: 0 = 合格、1 = 違反（または自己試験の失敗）、2 = 検査不能（fail-closed）。
set -euo pipefail
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"
export LC_ALL=C

for c in git grep tr mktemp sed; do
  require_cmd "${c}" || exit 2
done
cd "${ROOT_DIR}"

# 本番対象のディレクトリ（crates/<名前>）。check-openssl-vendored.sh の PROD_PKGS と同じ分類に保つ。
PROD_DIRS=(core secret db-tests client plugin-oidc backend)
# 試験専用のディレクトリ。check-openssl-vendored.sh の TEST_ONLY_PKGS と同じ分類に保つ（trial-deps の配下の別 workspace fuzz を含む）。
TEST_ONLY_DIRS=(hub-stub console-stub it-harness e2e-harness load-harness trial-deps)

CONSTS='6a09e667|bb67ae85|3c6ef372|a54ff53a|510e527f|9b05688c|1f83d9ab|5be0cd19|428a2f98|67452301|efcdab89|98badcfe|10325476|c3d2e1f0|f3bcc908|cbbb9d5d|61707865|3320646e|79622d32|6b206574|9e3779b9|243f6a88|0ffffffc0fffffff|800000000000808a|ffffffff00000001|0x63, *0x7c, *0x77, *0x7b'
ASM='\b(asm|global_asm|naked_asm)!|#\[naked\]|\b(core|std)::arch\b'
TRAITS='RngCore|CryptoRng|CryptoRngCore|SeedableRng|BlockCipher|BlockEncrypt|BlockDecrypt|StreamCipher|KeyInit|KeyIvInit|UniversalHash|Digest|DynDigest|FixedOutput|Mac'
C4RE="^[[:space:]]*(unsafe[[:space:]]+)?impl(<[^>]*>)?[[:space:]]+([A-Za-z0-9_]+::)*(${TRAITS})(<.*>)?[[:space:]]+for\\b"
WORDS='sha1|sha256|sha384|sha512|sha3|md5|hmac|hkdf|pbkdf2|argon2|scrypt|bcrypt|aes|chacha20|poly1305|ed25519|x25519|p256|p384|secp256k1|ecdsa|ecdh|rsa|keccak|blake2|blake3|siphash|xorshift|xoshiro|pcg32|lcg|mt19937|totp|hotp|gcm'
N1RE="\\b(fn|struct|enum|trait|mod|type|const|static|macro_rules!)[[:space:]]+([A-Za-z0-9]+_)*(${WORDS})(_[A-Za-z0-9]+)*\\b"

# run_grep <出力先> <規則> <file> <grep の引数>...: 標準入力を grep し、一致した行を `規則<TAB>file:行` で出力先へ足す。
# grep の終了コードが 0（一致）・1（なし）以外なら検査不能（2）で止める。
run_grep() {
  local out="$1" rule="$2" file="$3" rc=0 g
  shift 3
  # パイプの途中の grep の終了コードはサブシェルに隠れるため、一時ファイルに受けてから整形する。
  g="${out}.grep"
  grep -n "$@" >"${g}" || rc=$?
  if [ "${rc}" -gt 1 ]; then
    rm -f -- "${g}"
    log "error: grep failed (rule ${rule}); cannot inspect"
    exit 2
  fi
  # file は sed のプログラムへ展開しない（'|'・';'・'&'・'\\' 等を含む名前で式が壊れる・e コマンドが実行される経路を断つ）。
  # 行番号だけを sed で取り出し、パスは引用付きの printf の引数で組み立てる。
  local ln
  while IFS= read -r ln || [ -n "${ln}" ]; do
    printf '%s\t%s:%s\n' "${rule}" "${file}" "${ln}" >>"${out}"
  done < <(sed 's/:.*//' "${g}")
  rm -f -- "${g}"
}

# has_match <grep の引数>... <file>: 0（一致）・1（なし）だけを通常として返す。
# 2 以上（読み取り不能・不正な式）は検査不能で、握りつぶさず終了コード 2 で止める。
has_match() {
  local rc=0
  grep -q "$@" || rc=$?
  if [ "${rc}" -gt 1 ]; then
    log "error: grep failed (exit ${rc}); cannot inspect"
    exit 2
  fi
  return "${rc}"
}

# count_lines <式> <file>: 行頭一致の件数を COUNT に入れる。grep -c の 1（0 件）は通常、2 以上は検査不能（2）。
# サブシェルでは exit が親に届かないため、結果は大域変数で返す。
COUNT=0
count_lines() {
  local rc=0
  COUNT="$(grep -c -- "$1" "$2")" || rc=$?
  if [ "${rc}" -gt 1 ]; then
    log "error: grep failed (exit ${rc}); cannot inspect"
    exit 2
  fi
}

# show_lines <式> <file>: 行頭一致の行を整形して stderr へ出す。1（なし）は通常、2 以上は検査不能（2）。
show_lines() {
  local rc=0 g
  g="$(grep -- "$1" "$2")" || rc=$?
  if [ "${rc}" -gt 1 ]; then
    log "error: grep failed (exit ${rc}); cannot inspect"
    exit 2
  fi
  [ -n "${g}" ] && printf '%s\n' "${g}" | sed "$3" >&2
  return 0
}

# scan_rs <出力先> <file>: Rust のソース 1 ファイルに C1・C2・C4・C5 を当てる。
# shellcheck disable=SC2094 # file は読む入力で、出力先とは別のファイル
scan_rs() {
  local out="$1" f="$2"
  tr -d '_' <"${f}" | run_grep "${out}" C1 "${f}" -i -E -- "${CONSTS}"
  run_grep "${out}" C2 "${f}" -E -- "${ASM}" <"${f}"
  run_grep "${out}" C4 "${f}" -E -- "${C4RE}" <"${f}"
  if has_match -E 'rotate_(left|right)' "${f}" && has_match -E 'wrapping_(add|sub|mul)' "${f}"; then
    printf 'C5\t%s:1\n' "${f}" >>"${out}"
  fi
}

# scan_names <出力先> <file>: 参考の N1。
# shellcheck disable=SC2094 # file は読む入力で、出力先とは別のファイル
scan_names() {
  run_grep "$1" N1 "$2" -i -E -- "${N1RE}" <"$2"
}

# scan_native <出力先> <path>: C3。ファイル名の拡張子で判定する。
scan_native() {
  case "$2" in
    *.c | *.h | *.cc | *.cpp | *.cxx | *.hh | *.hpp | *.hxx | *.inc | *.cu | *.m | *.mm | *.s | *.S | *.asm) printf 'C3\t%s:1\n' "$2" >>"$1" ;;
  esac
}

# non_empty <件数>: 合否の範囲の対象が 0 件のまま合格にしない。
non_empty() { [ "$1" -gt 0 ]; }

# in_list <値> <配列の要素>...
in_list() {
  local v="$1" x
  shift
  for x in "$@"; do
    [ "${x}" = "${v}" ] && return 0
  done
  return 1
}

ST_FAIL=0
fail_selftest() {
  log "FAIL selftest $1: $2"
  ST_FAIL=1
}

# expect_count <S 名> <規則> <pos|neg> <出力ファイル>: 規則の検出が 1 件以上（pos）・0 件（neg）であることを確かめる。
expect_count() {
  local n
  count_lines "^$2	" "$4"
  n="${COUNT}"
  case "$3" in
    pos) [ "${n}" -ge 1 ] || fail_selftest "$1" "$2 not detected" ;;
    neg) [ "${n}" -eq 0 ] || fail_selftest "$1" "$2 false positive" ;;
  esac
}

selftest() {
  local t out rc
  t="$(mktemp -d)" || {
    log "error: cannot create selftest dir; cannot inspect"
    exit 2
  }
  out="${t}/out"
  : >"${out}"
  mk() { printf '%s\n' "$2" >"${t}/$1" || { rm -rf -- "${t}"; log "error: cannot write fixture ($1); cannot inspect"; exit 2; }; }
  mk s1.rs 'const IV: u32 = 0x6a09e667;'
  mk s2.rs 'fn f() { unsafe { core::arch::asm!("nop"); } }'
  mk s4.rs 'impl RngCore for Foo { fn next_u32(&mut self) -> u32 { 4 } }'
  mk s5.rs 'fn r(a: u32, b: u32) -> u32 { a.rotate_left(7).wrapping_add(b) }'
  mk s6.rs 'const K: u32 = 0x6A09_E667;'
  mk s7.rs 'fn sha256_digest() {}'
  mk s8.rs 'fn r(a: u32) -> u32 { a.rotate_left(7) }'
  mk 'x|;e id;#&\1 y.rs' 'const IV: u32 = 0x6a09e667;'
  mk s9.rs 'fn clock_reversal_is_error() {} fn unparsable() {} // Signer'
  scan_rs "${out}" "${t}/s1.rs"
  expect_count S1 C1 pos "${out}"
  : >"${out}"
  # S11: 記号・空白を含むファイル名でも、名前が sed の式に入らず、そのままの名前で報告される
  scan_rs "${out}" "${t}/x|;e id;#&\\1 y.rs"
  expect_count S11 C1 pos "${out}"
  grep -qF -- "x|;e id;#&\\1 y.rs:1" "${out}" || fail_selftest S11 "file name not preserved"
  : >"${out}"
  scan_rs "${out}" "${t}/s2.rs"
  expect_count S2 C2 pos "${out}"
  : >"${out}"
  scan_native "${out}" "${t}/x.c"
  expect_count S3 C3 pos "${out}"
  : >"${out}"
  scan_rs "${out}" "${t}/s4.rs"
  expect_count S4 C4 pos "${out}"
  : >"${out}"
  scan_rs "${out}" "${t}/s5.rs"
  expect_count S5 C5 pos "${out}"
  : >"${out}"
  scan_rs "${out}" "${t}/s6.rs"
  expect_count S6 C1 pos "${out}"
  : >"${out}"
  scan_names "${out}" "${t}/s7.rs"
  expect_count S7 N1 pos "${out}"
  : >"${out}"
  scan_rs "${out}" "${t}/s8.rs"
  expect_count S8 C5 neg "${out}"
  : >"${out}"
  scan_rs "${out}" "${t}/s9.rs"
  scan_names "${out}" "${t}/s9.rs"
  scan_native "${out}" "${t}/s9.rs"
  [ ! -s "${out}" ] || fail_selftest S9 "clean fixture produced findings"
  # S12: grep が 2 を返す（読み取り不能なファイル）場合は、未検出（1）に畳まず終了コード 2 で止まる
  rc=0
  (has_match -E 'x' "${t}/no-such-file" 2>/dev/null) || rc=$?
  [ "${rc}" -eq 2 ] || fail_selftest S12 "has_match did not stop with 2 (got ${rc})"
  rc=0
  (count_lines 'x' "${t}/no-such-file" 2>/dev/null) || rc=$?
  [ "${rc}" -eq 2 ] || fail_selftest S12 "count_lines did not stop with 2 (got ${rc})"
  ! non_empty 0 || fail_selftest S10 "empty target accepted"
  non_empty 1 || fail_selftest S10 "non-empty target rejected"
  rm -rf -- "${t}"
  if [ "${ST_FAIL}" -ne 0 ]; then
    exit 1
  fi
  log "selftest ok"
}

selftest

# 分類の照合: 表の重複・ルート Cargo.toml の members の分類漏れは検査不能（2）。
for p in "${PROD_DIRS[@]}"; do
  if in_list "${p}" "${TEST_ONLY_DIRS[@]}"; then
    log "error: crate classified as both production and test-only: ${p}; cannot inspect"
    exit 2
  fi
done
MEMBERS="$(grep -o '"crates/[^"/]*"' Cargo.toml | tr -d '"')" || {
  log "error: cannot read workspace members; cannot inspect"
  exit 2
}
for m in ${MEMBERS}; do
  name="${m#crates/}"
  if ! in_list "${name}" "${PROD_DIRS[@]}" && ! in_list "${name}" "${TEST_ONLY_DIRS[@]}"; then
    log "error: workspace member not classified (add to PROD_DIRS or TEST_ONLY_DIRS): ${name}; cannot inspect"
    exit 2
  fi
done

LIST="$(mktemp)" || {
  log "error: cannot create temp file; cannot inspect"
  exit 2
}
OUT="$(mktemp)" || {
  rm -f -- "${LIST}"
  log "error: cannot create temp file; cannot inspect"
  exit 2
}
TMP="$(mktemp)" || {
  rm -f -- "${LIST}" "${OUT}"
  log "error: cannot create temp file; cannot inspect"
  exit 2
}
trap 'rm -f -- "${LIST}" "${OUT}" "${TMP}"' EXIT
git ls-files -z --cached --others --exclude-standard -- crates/ >"${LIST}" || {
  log "error: git ls-files failed; cannot inspect"
  exit 2
}

prod_n=0
test_n=0
while IFS= read -r -d '' p; do
  case "${p}" in
    *[[:cntrl:]]*)
      log "error: path with control characters; cannot inspect"
      exit 2
      ;;
  esac
  top="${p#crates/}"
  top="${top%%/*}"
  if in_list "${top}" "${PROD_DIRS[@]}"; then
    scope='prod'
  elif in_list "${top}" "${TEST_ONLY_DIRS[@]}"; then
    scope='test'
  else
    log "error: file under unclassified crates/${top}; cannot inspect"
    exit 2
  fi
  [ -f "${p}" ] || continue
  if [ "${scope}" = prod ]; then
    prod_n=$((prod_n + 1))
    scan_native "${OUT}" "${p}"
    case "${p}" in
      *.rs)
        scan_rs "${OUT}" "${p}"
        scan_names "${OUT}" "${p}"
        ;;
    esac
    case "${p}" in
      */build.rs) printf 'N2\t%s:1\n' "${p}" >>"${OUT}" ;;
    esac
  else
    test_n=$((test_n + 1))
    # 試験専用の範囲は参考。規則の結果を T: 付きに付け替えて、合否の集計に入れない。
    : >"${TMP}"
    scan_native "${TMP}" "${p}"
    case "${p}" in
      *.rs)
        scan_rs "${TMP}" "${p}"
        scan_names "${TMP}" "${p}"
        ;;
    esac
    sed 's/^/T:/' "${TMP}" >>"${OUT}"
  fi
done <"${LIST}"

non_empty "${prod_n}" || {
  log "error: no files in the production scope; cannot inspect"
  exit 2
}

# 規則ごとの件数と検出の行を出す。
report() { # <接頭辞の正規表現> <規則名> <ラベル>
  local n
  count_lines "^$1	" "${OUT}"
  n="${COUNT}"
  log "$3: ${n}"
  show_lines "^$1	" "${OUT}" 's/^[^	]*	/  /'
  REPORTED="${n}"
}

fails=0
for r in C1 C2 C3 C4 C5; do
  report "${r}" "${r}" "${r}"
  fails=$((fails + REPORTED))
done
report N1 N1 "warning N1 (production, advisory)"
report N2 N2 "warning N2 (production, advisory)"
count_lines '^T:' "${OUT}"
tn="${COUNT}"
log "warning test-only crates (advisory, not counted): ${tn}"
show_lines '^T:' "${OUT}" 's/^T://;s/\t/ /g;s/^/  /'

log "scanned: production files ${prod_n}, test-only files ${test_n}; violations ${fails}"
if [ "${fails}" -gt 0 ]; then
  exit 1
fi
