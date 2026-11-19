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

.PHONY: help doctor setup check test-hooks build test fmt lint crate-deps pg-check

help: ## このヘルプを表示する（Make 非依存の一覧: scripts/help.sh）
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## /{printf "  \033[36mmake %-10s\033[0m %s\n", $$1, $$2}' $(firstword $(MAKEFILE_LIST))

doctor: ## 開発環境を診断する（読み取りのみ・何も導入しない）。直接: scripts/doctor.sh
	@scripts/doctor.sh

setup: ## Git hooks を有効化し .env を雛形から作成する（再実行可）。直接: scripts/setup.sh
	@scripts/setup.sh

check: ## editorconfig-checker + shellcheck（ソースは変更しない）。直接: scripts/check.sh
	@scripts/check.sh

test-hooks: ## scripts/hooks/ のフックスクリプト自体の回帰テスト（bash + git のみ）。直接: scripts/hooks/test-hooks.sh
	@scripts/hooks/test-hooks.sh

# build・test は暫定の名前（人が確定する）。CI の build-test ジョブがこの 2 つを呼ぶ（TASK-6）。--all-features は付けない（DPOL-31）。
build: ## workspace 全体をビルドする（Cargo.lock の外を解決しない）。直接: cargo build --workspace --locked
	@cargo build --workspace --locked

test: ## workspace 全体の試験を流す（CI の build-test ジョブと同じ契約）。直接: cargo test --workspace --locked
	@cargo test --workspace --locked

fmt: ## rustfmt で整形を適用する（ソースを書き換える）。直接: cargo fmt --all
	@cargo fmt --all

# CI の lint ジョブがこれを呼ぶ（TASK-6）。clippy の警告・整形の差分は非 0 で失敗する。
lint: ## rustfmt の差分検査 + clippy -D warnings + crate 間の依存の向き（ソースは変更しない）。直接: scripts/lint.sh
	@scripts/lint.sh

crate-deps: ## crate 間の依存の向きの検査（許可表との照合）。直接: scripts/check-crate-deps.sh
	@scripts/check-crate-deps.sh

# pg-check は暫定の名前（人が確定する）。起動・停止の make ターゲットは置かない
# （make の子プロセスは生成したパスワードを呼び出し元のシェルへ export できず、保存へ流れるため。SECR-6）。
pg-check: ## 開発・検証用 PostgreSQL の起動の確かめ（起動中が対象。起動・停止はしない。CI 外）。直接: scripts/dev-pg-check.sh
	@scripts/dev-pg-check.sh
