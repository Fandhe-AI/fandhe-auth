# 開発用 Makefile。
#
# Make は scripts/*.sh を呼ぶだけの薄い入口であり、処理の定義は scripts/ 側にのみ置く
# （各レシピは 1 行の呼び出しに限定し、ここでチェック内容を二重定義しない）。
# Make を使わない場合の同等コマンドは scripts/help.sh を参照。
#
# GNU Make 3.81（macOS 標準）で動作確認済み。3.82+ 専用機能（.ONESHELL・.RECIPEPREFIX 等）は使わない。
#
# 使い方: `make help`（既定ターゲット）でターゲット一覧を表示する。
# 一覧は下記の `## ` コメントから生成されるため、`## ` の無いターゲットは表示されない。
# ターゲットを追加・改名・削除したら scripts/help.sh も同じ変更で更新すること。
#
# 言語固有の build / test / fmt / lint / crate-deps は本リポジトリで定義済み。他の言語固有ターゲットは派生リポジトリで追加する。
# 中身の無いターゲット（`@true` 等）は呼び出し元が終了コードを信用してしまうため置かない。
# ターゲット・scripts/ の追加・変更時は .claude/rules/makefile.md に従う。

.DEFAULT_GOAL := help

.PHONY: help doctor setup check test-hooks secret-scan scan-canary build test fmt lint crate-deps no-vendored crypto-scan audit-cfg pg-check it-db it-db-ci e2e-chrome e2e-firefox load-trial

help: ## このヘルプを表示する（Make 非依存の一覧: scripts/help.sh）
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## /{printf "  \033[36mmake %-11s\033[0m %s\n", $$1, $$2}' $(firstword $(MAKEFILE_LIST))

doctor: ## 開発環境を診断する（読み取りのみ・何も導入しない）。直接: scripts/doctor.sh
	@scripts/doctor.sh

setup: ## Git hooks を有効化し .env を雛形から作成する（再実行可）。直接: scripts/setup.sh
	@scripts/setup.sh

check: ## editorconfig-checker + shellcheck（ソースは変更しない）。直接: scripts/check.sh
	@scripts/check.sh

test-hooks: ## scripts/hooks/ のフックスクリプト自体の回帰テスト（bash + git のみ）。直接: scripts/hooks/test-hooks.sh
	@scripts/hooks/test-hooks.sh

# secret-scan は暫定の名前（人が確定する）。scripts/secret-scan-ci.sh（--tracked --summary と scan-canary 相当）は CI の secret scan ジョブが呼ぶ（#51。TASK-6）。
secret-scan: ## 追跡ファイル全体の秘密情報の混入検査（検査のみ・値は出さず件数で判定）。直接: scripts/hooks/secret-scan.sh --tracked
	@scripts/hooks/secret-scan.sh --tracked

# scan-canary は暫定の名前（人が確定する）。検出器が常に合格する状態を CI で見逃さないための陰性対照。
scan-canary: ## secret-scan の陰性対照（ダミーで検出器が 1 を返し、集計行が期待と完全に一致し、ダミー値が出ないことを確かめる。何も変更しない）。直接: scripts/secret-scan-canary.sh
	@scripts/secret-scan-canary.sh

# build・test は暫定の名前（人が確定する）。CI の build-test ジョブがこの 2 つを呼ぶ（TASK-6）。--all-features は付けない（DPOL-31）。
build: ## workspace 全体をビルドする（Cargo.lock の外を解決しない）。直接: cargo build --workspace --locked
	@cargo build --workspace --locked

test: ## workspace 全体の試験を流す（CI の build-test ジョブと同じ契約）。直接: cargo test --workspace --locked
	@cargo test --workspace --locked

fmt: ## rustfmt で整形を適用する（ソースを書き換える）。直接: cargo fmt --all
	@cargo fmt --all

# CI の lint ジョブがこれを呼ぶ（TASK-6）。clippy の警告・整形の差分は非 0 で失敗する。
lint: ## rustfmt の差分検査 + clippy -D warnings + crate 間の依存の向き + vendored 検査（ソースは変更しない）。直接: scripts/lint.sh
	@scripts/lint.sh

crate-deps: ## crate 間の依存の向きの検査（許可表との照合）。直接: scripts/check-crate-deps.sh
	@scripts/check-crate-deps.sh

# no-vendored は暫定の名前（人が確定する）。DPOL-13・TASK-667。lint からも呼ばれる。
no-vendored: ## openssl-sys の vendored の検査（合否は本番メンバーの normal・build 依存で、既定 feature と PROD_FEATURES で明示した feature だけ。全 feature の走査は参考（警告）。静的リンクを強いる設定の検出を含む。検査のみ）。直接: scripts/check-openssl-vendored.sh
	@scripts/check-openssl-vendored.sh

# audit-cfg は暫定の名前（人が確定する）。DPOL-34・TASK-679。cargo-audit を起動する前に必ず呼ぶ検査で、cargo-audit 自体は流さない。
audit-cfg: ## cargo-audit が読む設定が無いこと（承認済みは無し）の検査（cargo-audit 0.22.2 の読み込み先。検査のみ）。直接: scripts/check-audit-config.sh
	@scripts/check-audit-config.sh

# crypto-scan は暫定の名前（人が確定する）。LIB-21・DPOL-4・DPOL-47・TASK-715 (3/4)。
crypto-scan: ## 自作の暗号処理の走査（合否は本番メンバーの C1〜C5。試験専用 crate と N1・N2 は参考の警告。発見的で人のレビューの代わりにならない。検査のみ）。直接: scripts/check-crypto-primitives.sh
	@scripts/check-crypto-primitives.sh

# pg-check は暫定の名前（人が確定する）。起動・停止の make ターゲットは置かない
# （make の子プロセスは生成したパスワードを呼び出し元のシェルへ export できず、保存へ流れるため。SECR-6）。
pg-check: ## 開発・検証用 PostgreSQL の起動の確かめ（起動中が対象。起動・停止はしない。CI 外）。直接: scripts/dev-pg-check.sh
	@scripts/dev-pg-check.sh

# it-db は暫定の名前（人が確定する）。Docker を使う結合試験（#[ignore]）の明示の入口。CI には載せない（TASK-6）。
it-db: ## ハーネスの DB 結合試験（Docker 必須・#[ignore] の試験を流す。CI 外）。直接: scripts/it-db.sh
	@scripts/it-db.sh

# it-db-ci は暫定の名前（人が確定する）。CI の DB ジョブ（it-db-ci）が呼ぶ入口で、枠の試験（db_job_frame）だけを流す（TASK-6。#52）。
# パスワードは試験プロセスの中で生成する（SECR-6）。ここでも CI でも起動・停止の手順は持たない。
it-db-ci: ## CI の DB ジョブの入口（Docker・Docker Compose・openssl 必須・枠の試験だけを流す。0 件は失敗）。直接: scripts/it-db-ci.sh
	@scripts/it-db-ci.sh

# e2e-chrome は暫定の名前（人が確定する）。chromedriver を使う E2E（#[ignore]）の明示の入口。CI には載せない（TASK-6）。
e2e-chrome: ## ハーネスの Chrome の E2E（chromedriver 必須・#[ignore] の試験を流す。CI 外）。直接: scripts/e2e-chrome.sh
	@scripts/e2e-chrome.sh

# e2e-firefox は暫定の名前（人が確定する）。geckodriver を使う E2E（#[ignore]）の明示の入口。CI には載せない（TASK-6）。
e2e-firefox: ## ハーネスの Firefox の E2E（geckodriver・Firefox 必須（記録のある組み合わせは Firefox 156〜158 + geckodriver 0.37.1。詳細は scripts/e2e-firefox.sh）・#[ignore] の試験を流す。CI 外）。直接: scripts/e2e-firefox.sh
	@scripts/e2e-firefox.sh

# load-trial は暫定の名前（人が確定する）。計測・負荷のハーネスの小さな試し実行の明示の入口。CI には載せない（同じ試験は make test に入っている。TASK-12 10/10）。
load-trial: ## 計測・負荷のハーネスの小さな試し実行（Docker 不要・stub が相手・記録の本文を標準出力に出す・0 件は失敗。CI 外）。直接: scripts/load-trial.sh
	@scripts/load-trial.sh
