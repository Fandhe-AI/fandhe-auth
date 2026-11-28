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
  check       scripts/check.sh               editorconfig-checker + shellcheck + 暗号の自作の走査（crypto-scan。ソースは変更しない）
  test-hooks  scripts/hooks/test-hooks.sh    フックスクリプト自体の回帰テスト（bash + git が基本。part 12 の lib-deps だけ cargo が要り、無ければローカルは SKIP、CI では失敗）
  secret-scan scripts/hooks/secret-scan.sh --tracked   追跡ファイル全体の秘密情報の混入検査（検査のみ。値は出さず件数で判定）
  scan-canary scripts/secret-scan-canary.sh   secret-scan の陰性対照（ダミーで検出器が 1 を返し、集計行が期待と完全に一致し、ダミー値が出ないことを確かめる。何も変更しない）
  build       cargo build --workspace --locked   workspace 全体をビルドする（--locked。--all-features は使わない）
  test        cargo test --workspace --locked    workspace 全体の試験を流す
  fmt         cargo fmt --all                rustfmt で整形を適用する（ソースを書き換える）
  lint        scripts/lint.sh                rustfmt の差分検査 + clippy -D warnings + crate 間の依存の向き + vendored 検査
  crate-deps  scripts/check-crate-deps.sh    crate 間の依存の向きの検査（許可表との照合）
  lib-deps    scripts/check-lib-deps.sh      本体の crate の通常の依存の禁止 crate の検査（cargo tree。陰性対照つき。検査のみ）
  no-vendored scripts/check-openssl-vendored.sh openssl-sys の vendored の検査（合否は本番メンバーの normal・build 依存で、既定 feature と PROD_FEATURES で明示した feature だけ。全 feature の走査は参考（警告）。静的リンクを強いる設定の検出を含む。検査のみ）
  crypto-scan scripts/check-crypto-primitives.sh 自作の暗号処理の走査（合否は本番メンバーの C1〜C5。試験専用 crate と N1・N2 は参考の警告。発見的で人のレビューの代わりにならない。make check からも呼ばれる。検査のみ）
  audit-cfg   scripts/check-audit-config.sh fetch  cargo-audit が読む設定が無いこと（承認済みは無し）の検査。取得の前の段（CARGO_HOME は既定のみ。cargo-audit 0.22.2 の読み込み先。検査のみ）
  audit-cfg-nofetch scripts/check-audit-config.sh no-fetch  audit-cfg の取得なしの段（cargo-audit --no-fetch の前。advisory-db の状態の検査を加える。検査のみ）
  pg-check    scripts/dev-pg-check.sh        開発・検証用 PostgreSQL の起動の確かめ（起動中が対象。起動・停止はしない。CI 外）
  it-db       scripts/it-db.sh               ハーネスの DB 結合試験（Docker 必須・#[ignore] の試験を流す。CI 外）
  it-db-ci    scripts/it-db-ci.sh            CI の DB ジョブの入口（Docker・Docker Compose・openssl 必須・枠の試験だけを流す。0 件は失敗）
  e2e-chrome  scripts/e2e-chrome.sh          ハーネスの Chrome の E2E（chromedriver 必須・#[ignore] の試験を流す。CI 外）
  e2e-firefox scripts/e2e-firefox.sh         ハーネスの Firefox の E2E（geckodriver・Firefox 必須（記録のある組み合わせは Firefox 156〜158 + geckodriver 0.37.1。詳細は scripts/e2e-firefox.sh）・#[ignore] の試験を流す。CI 外）
  load-trial  scripts/load-trial.sh          計測・負荷のハーネスの小さな試し実行（Docker 不要・stub が相手・記録の本文を標準出力に出す・0 件は失敗。CI 外）

CI 向けに直接実行するコマンド:

  scripts/install-msrv.sh [COMPONENT...]  CI 向け。Cargo.toml の MSRV（scripts/msrv.sh が読む。終了コード 2 = 読めない）の toolchain を rustup で入れて既定にし、後続のステップへ RUSTUP_TOOLCHAIN で渡す（配布元の変数が設定済みなら 2。component は引数で渡す）
  scripts/secret-scan-ci.sh [PATH]      CI 向けの入口（PATH は走査対象のリポジトリのルート）。HEAD の tree から作った一時 index を --summary（件数のみ公開）で走査し、続けて陰性対照を実行。作業中の index は読まない。終了コード 0 = 検出なし / 1 = 検出、または陰性対照の失敗（検出器の異常）/ 2 = 検査不能

Git hooks（lefthook.yml から呼ばれる。直接実行も可）:

  scripts/hooks/secret-scan.sh          staged 差分の秘密情報検知（--tracked は追跡ファイル全体。フックからは呼ばない）
  scripts/hooks/commit-msg-check.sh F   コミットメッセージ F の Conventional Commits 検証
  scripts/hooks/shellcheck-staged.sh F...  staged の *.sh を最大 CPU 数のバッチに分けて並列で shellcheck する（引数 0 件は何もしない。0 = 成功、1 = 指摘だけ、2 = 検査不能）

`make <target>` は上記と同じコマンドを呼ぶだけの薄い入口であり、
処理の定義は scripts/ 側にのみ存在する。
EOF
