#!/usr/bin/env bash
# cargo-audit が読む設定が「承認済み（現時点は無し）」の状態であることを、cargo-audit の起動の前に確かめる（TASK-679・DPOL-34）。
# 理由: deps-audit の契約（.claude/rules/github-actions.md「依存の検査の拡張点」）が固定するのは cargo-deny の設定（deny.toml）だけで、
# cargo-audit は別の場所の audit.toml を黙って読み、[advisories] ignore 等で検出を抑えたまま成功し得る。その成功が DPOL-34 の合格に
# 入らないよう、承認していない設定の存在を失敗にする。呼び出し元: TASK-679 の監査の入口（cargo-audit を起動する直前に同じ環境・同じ cwd で呼ぶ）・
# TASK-680 の結線・`make audit-cfg`。検査を外す引数・環境変数は作らない（DPOL-30〜32）。
#
# 承認する cargo-audit の設定は「無し」（ファイルが 1 つも無いこと）。例外を足すかは人が決める（DPOL-20・DPOL-35）ため、この既定は暫定。
# 中身は解析しない: ignore 以外のキー（DB の path・url・stale、target の絞り込み、informational_warnings・severity_threshold、yanked 等）でも
# 検出を変えられるため、置き場に何かが存在すれば違反にする（壊れたシンボリックリンク・ディレクトリも含める）。
#
# 読み込み先の実測（cargo-audit 0.22.2。版を上げたら測り直す。VERIFIED_CARGO_AUDIT_VERSION と ci.yml の CARGO_AUDIT_VERSION の一致は試験で確かめる）:
#   - <cwd>/.cargo/audit.toml は読まれる。無いときに限り $CARGO_HOME/audit.toml が読まれる（両方あれば cwd 側だけ。ただし cwd 側が消えると
#     残った方が効くため、優先順位によらず両方を見る）。
#   - CARGO_HOME が未設定・空なら $HOME/.cargo/audit.toml。相対パスの CARGO_HOME は cwd 基準で解くため、解決先が cwd で変わる。検査不能（2）で止める。
#   - 読まれない: <cwd>/audit.toml・親ディレクトリの .cargo/audit.toml・`-f` の lockfile のディレクトリの .cargo/audit.toml。
#     よって cargo-audit を起動する cwd を固定する（AUDIT_RUN_DIRS）。
#   - 設定ファイルを指定する・読まない指定の CLI オプションは無い。
#   - .cargo/audit.toml が壊れたシンボリックリンクなら「無い」扱いで CARGO_HOME 側へ落ちる。ディレクトリなら誤りになる。
#   - .cargo/config.toml の [alias] は外部サブコマンド `cargo audit` を覆い隠せる。入口は `cargo-audit audit` を直接起動する。
# 限界: ガードの後に置かれた設定は防げない（TOCTOU。入口は直前に呼ぶ）。CLI の --ignore 等はここでは見えない。入口が固定の argv で塞ぐ
#   （github-actions.md の契約）。
#
# 2 段（DPOL-33・DPOL-34）: 引数は段の名前を 1 つだけ取る（検査を外す引数ではなく、どちらの段も設定の検査を行う）。
#   どちらの段も: CARGO_HOME が未設定か空（既定の置き場）であることを確かめる。設定されていれば 2（DB の置き場を別の場所へ向けさせない）。
#   fetch    取得（DB の取得を兼ねる cargo-audit の先行実行）の前。上の検査と設定ファイルの検査だけ（DB はまだ無くてよい）。
#   no-fetch 取得なし（--no-fetch）の実行の前。上に加えて、advisory-db（$HOME/.cargo/advisory-db）が git の作業ツリーの最上位で、
#            crates/ に advisory が 1 件以上あり（空でないことの確認）、変更（未追跡・無視されたファイルを含む git status の差分）が無いことを確かめる。
#            どれかが満たされなければ 2。実測: cargo-audit 0.22.2 の advisory DB の置き場は $CARGO_HOME/advisory-db で、空の DB を置いた CARGO_HOME では
#            `--no-fetch` が 0 件の advisory を読み込んで走査を進める。
#   限界（TASK-679 で追う。DPOL-33・DPOL-34）: HOME の差し替えと偽の git リポジトリ（作業ツリー・advisory 1 件以上・変更なしを満たす偽の DB）は防げない。
#            DB の git の設定・index の細工（skip-worktree・assume-unchanged・sparse-checkout 等）も偽の git リポジトリと同じ種類で防げない
#            （DB 側の `filter.*` と `.gitattributes` の組で git status の中から外部コマンドが走り得ることも同じ種類。#6170）
#            （git status は core.fsmonitor を空にし、未追跡・無視されたファイルを全て出し、--no-optional-locks で index を書かない指定で流す）。
#            取得と取得なしの実行が同じ DB を使うことの保証、DB の commit の記録の様式は TASK-679 の入口で決める。
#
# 出力は固定の書式の記録行（監査の記録に含める）。ファイルの中身・絶対パス・ホームのパスは出さない（SECR-23）。
# 終了コード: 0 合格 / 1 違反 / 2 検査不能（fail-closed）。`make` 経由では GNU Make が 1 も 2 に変える。
set -euo pipefail
unset CDPATH
# lib.sh の require_cmd は 1 つの名前しか見ず、欠けると 1（違反）で終わるため使わない。1 つずつ確かめ、欠ければ検査不能の 2 で止める。
# 後の DIR の解決が dirname を使うため、確認は DIR の解決と lib.sh の読み込みより前に置く（欠けても 2 になる。#6170）。
for required_cmd in git grep tr find dirname; do
  if ! command -v "${required_cmd}" > /dev/null 2>&1; then
    echo "error: required command not found on PATH: ${required_cmd}" >&2
    exit 2
  fi
done
# 継いだ git の向き先は、repo の一覧（手順 4）と advisory-db の status の両方を別の repo へ向けさせ得るため消す（#6170）。
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

# 段の名前は必須。無い・未知なら誤用として 2 で止める。
if [ "$#" -ne 1 ]; then
  echo "usage: $(basename "$0") fetch|no-fetch" >&2
  exit 2
fi
stage="$1"
case "${stage}" in
  fetch | no-fetch) ;;
  *)
    echo "usage: $(basename "$0") fetch|no-fetch" >&2
    exit 2
    ;;
esac

# 読み込み先を実測した版。ci.yml の CARGO_AUDIT_VERSION と一致させる（試験で照合する）。
VERIFIED_CARGO_AUDIT_VERSION="0.22.2"
# cargo-audit を起動する cwd（repo ルートからの相対）。契約の 4 の 2 つの lockfile に対応する。
AUDIT_RUN_DIRS=(. crates/trial-deps/fuzz)

violations=0

# 1: 道具の版。読み込み先は版で変わり得るため、実測した版と完全に一致しなければ検査不能。
if ! command -v cargo-audit > /dev/null 2>&1; then
  echo "error: cargo-audit not found on PATH" >&2
  exit 2
fi
actual_version=""
if ! actual_version="$(cargo-audit audit --version 2> /dev/null)"; then
  echo "error: cannot read the cargo-audit version" >&2
  exit 2
fi
if [ "${actual_version}" != "cargo-audit-audit ${VERIFIED_CARGO_AUDIT_VERSION}" ]; then
  echo "error: cargo-audit version differs from the measured one (${VERIFIED_CARGO_AUDIT_VERSION}); measure the read locations again" >&2
  exit 2
fi
echo "audit-config: cargo-audit ${VERIFIED_CARGO_AUDIT_VERSION} (read locations measured on ${VERIFIED_CARGO_AUDIT_VERSION})"
echo "audit-config: approved config: none"
echo "audit-config: stage: ${stage}"

# report_path <記録の表示名> <パス>: 存在（壊れたリンク・ディレクトリを含む）すれば違反として数える。
report_path() {
  if [ -e "$2" ] || [ -L "$2" ]; then
    echo "audit-config: $1: present (not approved)"
    violations=$((violations + 1))
  else
    echo "audit-config: $1: absent"
  fi
}

# 2: cargo-audit を起動する各 cwd の .cargo/audit.toml
for run_dir in "${AUDIT_RUN_DIRS[@]}"; do
  if [ "${run_dir}" = "." ]; then
    report_path "<root>/.cargo/audit.toml" "${ROOT_DIR}/.cargo/audit.toml"
  else
    report_path "<root>/${run_dir}/.cargo/audit.toml" "${ROOT_DIR}/${run_dir}/.cargo/audit.toml"
  fi
done

# 3: CARGO_HOME の解決（home crate と同じ順。空・未設定は $HOME/.cargo）
cargo_home_src="env"
cargo_home="${CARGO_HOME:-}"
if [ -z "${cargo_home}" ]; then
  cargo_home_src="HOME"
  if [ -z "${HOME:-}" ]; then
    echo "error: neither CARGO_HOME nor HOME is set" >&2
    exit 2
  fi
  cargo_home="${HOME}/.cargo"
fi
case "${cargo_home}" in
  /*) ;;
  *)
    echo "error: CARGO_HOME is a relative path; its resolution depends on the cwd" >&2
    exit 2
    ;;
esac
# 両方の段: CARGO_HOME は既定の置き場（未設定か空）だけを認める。
if [ "${cargo_home_src}" = "env" ]; then
  echo "error: CARGO_HOME is set; the ${stage} stage requires the default location (unset or empty)" >&2
  exit 2
fi
echo "audit-config: CARGO_HOME for advisory-db: default"
if [ "${stage}" = "no-fetch" ]; then
  db="${cargo_home}/advisory-db"
  if [ ! -d "${db}" ]; then
    echo "error: advisory-db is missing (CARGO_HOME from ${cargo_home_src})" >&2
    exit 2
  fi
  db_phys="$(cd "${db}" 2> /dev/null && pwd -P)" || {
    echo "error: cannot enter advisory-db" >&2
    exit 2
  }
  db_top=""
  if ! db_top="$(git -C "${db}" rev-parse --show-toplevel 2> /dev/null)" || [ "${db_top}" != "${db_phys}" ]; then
    echo "error: advisory-db is not the top of a git work tree" >&2
    exit 2
  fi
  # find の失敗（権限なし等）を件数 0 や一部の件数に読まないよう、一覧を先に取る。grep は 0 件で 1、実行の失敗で 2 を返す。
  advisory_list=""
  if ! advisory_list="$(find "${db}/crates" -type f -name 'RUSTSEC-*.md' 2> /dev/null)"; then
    echo "error: cannot count advisories in advisory-db" >&2
    exit 2
  fi
  grep_rc=0
  advisory_count="$(printf '%s\n' "${advisory_list}" | grep -c 'RUSTSEC-')" || grep_rc=$?
  if [ "${grep_rc}" -gt 1 ]; then
    echo "error: cannot count advisories in advisory-db" >&2
    exit 2
  fi
  case "${advisory_count}" in
    '' | *[!0-9]*)
      echo "error: cannot read the advisory count" >&2
      exit 2
      ;;
    *) ;;
  esac
  echo "audit-config: advisory-db (CARGO_HOME from ${cargo_home_src}): git work tree, advisories: ${advisory_count}"
  if [ "${advisory_count}" -lt 1 ]; then
    echo "error: advisory-db has no advisory (empty)" >&2
    exit 2
  fi
  db_status=""
  if ! db_status="$(git -c core.fsmonitor= -c status.showUntrackedFiles=all --no-optional-locks -C "${db}" status --porcelain --ignored 2> /dev/null)"; then
    echo "error: cannot read the advisory-db status" >&2
    exit 2
  fi
  if [ -n "${db_status}" ]; then
    echo "error: advisory-db has changes (modified or untracked entries: $(printf '%s\n' "${db_status}" | grep -c .))" >&2
    exit 2
  fi
  echo "audit-config: advisory-db changes: 0"
fi
report_path "CARGO_HOME/audit.toml (CARGO_HOME from ${cargo_home_src})" "${cargo_home}/audit.toml"

# 4: repo 全体の多重の防御（ほかの cwd から起動される道を塞ぐ。追跡外も数える）
# パイプの全段の終了（git ls-files・tr）を確かめる。tr の失敗で一覧が空・一部になり件数 0 で合格しないようにする。
listing_raw=""
listing_raw="$(
  git -C "${ROOT_DIR}" ls-files -z -co --exclude-standard 2> /dev/null | tr '\0' '\n'
  printf '\n%s' "${PIPESTATUS[*]}"
)" || true
listing_status="${listing_raw##*$'\n'}"
listing="${listing_raw%$'\n'*}"
if [ "${listing_status}" != "0 0" ]; then
  echo "error: cannot list repository files" >&2
  exit 2
fi
# grep は 0 件で 1、実行の失敗で 2 を返す。1 は件数 0 として読み、それ以外の非 0・数字でない出力は検査不能（fail-closed）にする。
grep_rc=0
nested="$(printf '%s\n' "${listing}" | grep -c -E '(^|/)\.cargo/audit\.toml$')" || grep_rc=$?
if [ "${grep_rc}" -gt 1 ]; then
  echo "error: cannot count nested .cargo/audit.toml files" >&2
  exit 2
fi
case "${nested}" in
  '' | *[!0-9]*)
    echo "error: cannot read the nested .cargo/audit.toml count" >&2
    exit 2
    ;;
  *) ;;
esac
echo "audit-config: .cargo/audit.toml in repo (tracked or untracked): ${nested}"
if [ "${nested}" -gt 0 ]; then
  violations=$((violations + 1))
fi

if [ "${violations}" -gt 0 ]; then
  echo "audit-config: result: violation"
  exit 1
fi
echo "audit-config: result: ok"
