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
# 出力は固定の書式の記録行（監査の記録に含める）。ファイルの中身・絶対パス・ホームのパスは出さない（SECR-23）。
# 終了コード: 0 合格 / 1 違反 / 2 検査不能（fail-closed）。`make` 経由では GNU Make が 1 も 2 に変える。
set -euo pipefail
unset CDPATH
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"

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
report_path "CARGO_HOME/audit.toml (CARGO_HOME from ${cargo_home_src})" "${cargo_home}/audit.toml"

# 4: repo 全体の多重の防御（ほかの cwd から起動される道を塞ぐ。追跡外も数える）
listing=""
if ! listing="$(git -C "${ROOT_DIR}" ls-files -z -co --exclude-standard | tr '\0' '\n'; exit "${PIPESTATUS[0]}")"; then
  echo "error: cannot list repository files" >&2
  exit 2
fi
nested="$(printf '%s\n' "${listing}" | grep -c -E '(^|/)\.cargo/audit\.toml$' || true)"
echo "audit-config: .cargo/audit.toml in repo (tracked or untracked): ${nested}"
if [ "${nested}" -gt 0 ]; then
  violations=$((violations + 1))
fi

if [ "${violations}" -gt 0 ]; then
  echo "audit-config: result: violation"
  exit 1
fi
echo "audit-config: result: ok"
