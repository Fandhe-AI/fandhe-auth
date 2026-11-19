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
  scan-canary scripts/secret-scan-canary.sh   secret-scan の陰性対照（実行時に組み立てたダミーで検出器が 1 を返すことを確かめる。何も変更しない）
  build       cargo build --workspace --locked   workspace 全体をビルドする（--locked。--all-features は使わない）
  test        cargo test --workspace --locked    workspace 全体の試験を流す
  fmt         cargo fmt --all                rustfmt で整形を適用する（ソースを書き換える）
  lint        scripts/lint.sh                rustfmt の差分検査 + clippy -D warnings + crate 間の依存の向き
  crate-deps  scripts/check-crate-deps.sh    crate 間の依存の向きの検査（許可表との照合）
  pg-check    scripts/dev-pg-check.sh        開発・検証用 PostgreSQL の起動の確かめ（起動中が対象。起動・停止はしない。CI 外）

Git hooks（lefthook.yml から呼ばれる。直接実行も可）:

  scripts/hooks/secret-scan.sh          staged 差分の秘密情報検知（--tracked は追跡ファイル全体。フックからは呼ばない）
  scripts/hooks/commit-msg-check.sh F   コミットメッセージ F の Conventional Commits 検証

`make <target>` は上記と同じコマンドを呼ぶだけの薄い入口であり、
処理の定義は scripts/ 側にのみ存在する。
EOF
