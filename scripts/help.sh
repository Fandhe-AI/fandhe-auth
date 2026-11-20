#!/usr/bin/env bash
# Make を介さない操作一覧。`make help` は Makefile の `## ` コメントから
# 機械的に生成されるのに対し、本ファイルは手動で保守する一覧であり、
# 両者を自動で同期する仕組みは無い。Makefile のターゲットを追加・改名・
# 削除したら同じ変更で本ファイルも更新すること。
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib.sh
source "${DIR}/lib.sh"

reject_args "$(basename "$0")" "$@"

cat <<'EOF'
このリポジトリの操作と、Make を使わずに直接実行する方法:

  help        scripts/help.sh                この一覧を表示する
  doctor      scripts/doctor.sh              開発環境を診断する（読み取りのみ。何も導入・修復しない）
  setup       scripts/setup.sh               Git hooks 有効化と .env 雛形の配置（再実行しても安全）
  check       scripts/check.sh               editorconfig-checker + shellcheck（ソースは変更しない）
  test-hooks  scripts/hooks/test-hooks.sh    フックスクリプト自体の回帰テスト（bash + git のみ）
  secret-scan scripts/hooks/secret-scan.sh --tracked   追跡ファイル全体の秘密情報の混入検査（検査のみ。値は出さず件数で判定）
  scan-canary scripts/secret-scan-canary.sh   secret-scan の陰性対照（ダミーで検出器が 1 を返し、集計行が期待と完全に一致し、ダミー値が出ないことを確かめる。何も変更しない）
  build       cargo build --workspace --locked   workspace 全体をビルドする（--locked。--all-features は使わない）
  test        cargo test --workspace --locked    workspace 全体の試験を流す
  fmt         cargo fmt --all                rustfmt で整形を適用する（ソースを書き換える）
  lint        scripts/lint.sh                rustfmt の差分検査 + clippy -D warnings + crate 間の依存の向き + vendored 検査
  crate-deps  scripts/check-crate-deps.sh    crate 間の依存の向きの検査（許可表との照合）
  no-vendored scripts/check-openssl-vendored.sh openssl-sys の vendored の検査（合否は本番メンバーの normal・build 依存で、既定 feature と PROD_FEATURES で明示した feature だけ。全 feature の走査は参考（警告）。検査のみ）
  pg-check    scripts/dev-pg-check.sh        開発・検証用 PostgreSQL の起動の確かめ（起動中が対象。起動・停止はしない。CI 外）
  it-db       scripts/it-db.sh               ハーネスの DB 結合試験（Docker 必須・#[ignore] の試験を流す。CI 外）
  it-db-ci    scripts/it-db-ci.sh            CI の DB ジョブの入口（Docker・Docker Compose・openssl 必須・枠の試験だけを流す。0 件は失敗）
  e2e-chrome  scripts/e2e-chrome.sh          ハーネスの Chrome の E2E（chromedriver 必須・#[ignore] の試験を流す。CI 外）
  e2e-firefox scripts/e2e-firefox.sh         ハーネスの Firefox の E2E（geckodriver・Firefox 必須（記録のある組み合わせは Firefox 156〜158 + geckodriver 0.37.1。詳細は scripts/e2e-firefox.sh）・#[ignore] の試験を流す。CI 外）

CI 向けに直接実行するコマンド:

  scripts/install-msrv.sh [COMPONENT...]  CI 向け。Cargo.toml の MSRV（scripts/msrv.sh が読む。終了コード 2 = 読めない）の toolchain を rustup で入れて既定にする（component は引数で渡す）
  scripts/secret-scan-ci.sh [PATH]      CI 向けの入口（PATH は走査対象のリポジトリのルート）。HEAD の tree から作った一時 index を --summary（件数のみ公開）で走査し、続けて陰性対照を実行。作業中の index は読まない。終了コード 0 = 検出なし / 1 = 検出、または陰性対照の失敗（検出器の異常）/ 2 = 検査不能

Git hooks（lefthook.yml から呼ばれる。直接実行も可）:

  scripts/hooks/secret-scan.sh          staged 差分の秘密情報検知（--tracked は追跡ファイル全体。フックからは呼ばない）
  scripts/hooks/commit-msg-check.sh F   コミットメッセージ F の Conventional Commits 検証

`make <target>` は上記と同じコマンドを呼ぶだけの薄い入口であり、
処理の定義は scripts/ 側にのみ存在する。
EOF
