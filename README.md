# fandhe-auth

Keycloak を使わない Rust 製の共通認証基盤の実装リポジトリです。
Hub が組み込むライブラリ（本体）として、型・判定・拡張契約・ストレージ契約・安全条件を提供します（`D-2026-10-30-1`）。
passkey・認証セッション・失効・復旧・登録済み自社クライアント向けの OIDC・ローカルエージェント認可を担い、
`fandhe-service-hub` と今後の自社サービスで共通して使うことを目指します。
HTTP の受付・Cookie の発行・DB の実装と migration・配送などの実行と運用は Hub の担当です。

開発基盤は [Fandhe-AI/template-dev](https://github.com/Fandhe-AI/template-dev) から作成しています。
言語固有の設定・実装は、実装着手の指示後にタスクに沿って追加します。

## 仕様（`docs/spec`）

仕様・フェーズ文書は private リポジトリ `Fandhe-AI/fandhe-auth-spec` を `docs/spec` に
submodule として置いています（閲覧には権限が必要です）。

認証の無い環境での clone が失敗しないよう `.gitmodules` で `update = none` を指定しているため、
`git clone --recurse-submodules` や `git submodule update --init` では取得されません。
spec リポジトリへの読み取り権限がある環境で、次のように明示的に取得します。

```bash
git -c submodule.docs/spec.update=checkout submodule update --init docs/spec
```

参照の更新は `update-external.yml` が日次で PR を作成します（後述の「CI」）。

## ライセンス

MIT OR Apache-2.0 のデュアルライセンスです（[LICENSE-MIT](./LICENSE-MIT) / [LICENSE-APACHE](./LICENSE-APACHE)）。

## 構成

| パス | 役割 |
|---|---|
| `.agents/skills/`・`.claude/skills/` | エージェント用スキル（`skills-lock.json` で管理する外部取得物。`.claude/skills/` は `.agents/skills/` への symlink） |
| `CLAUDE.md` | Claude Code 向けの案内（Overview・委譲方針・Conventions を持つ）。サブエージェント・ルール・スキルの一覧は持たず、実体（`.claude/agents/`・`.claude/rules/`・`skills-lock.json`）を参照する |
| `.claude/agents/`・`.claude/rules/` | サブエージェント定義（research / implement / testing / quality / author）とルール（委譲・セキュリティ・spec の参照・Rust の規約・秘密値・DB 層・依存方針・既存の hooks / Makefile / Actions / Rust ビルド設定など）。一覧は各ディレクトリの実体を正とし、スキルは `skills-lock.json`（実体は `.agents/skills/`、`.claude/skills/` は symlink）。`.claude/` 配下は `_/dotclaude/` で作成してから `mv` で配置する（`dotclaude-via-temp.md`） |
| `.claude/settings.json`・`.claude/hooks/` | Claude Code の hooks（SessionStart のリマインダー、編集した `.rs` への `rustfmt`） |
| `.claude/workflows/` | `implement-issue-tree` の named workflow（スキル内スクリプトへの相対 symlink） |
| `Cargo.toml`・`Cargo.lock`・`crates/` | Cargo の workspace（virtual workspace）と段階 1 の crate 雛形（`crates/<名前>`、crate 名は `fandhe-auth-<名前>`）。members は明示列挙する（後述の「ビルド」） |
| `Makefile` | `scripts/` を呼ぶだけの薄い入口（`make help` で一覧） |
| `rustfmt.toml` | rustfmt の設定（edition のみ。値は 2024。`D-2026-11-20-12`） |
| `scripts/` | 処理の実体（`help` / `doctor` / `setup` / `check` / `lint` / `check-crate-deps` / `check-lib-deps` / `check-openssl-vendored` / `check-crypto-primitives` / `dev-pg-check` / `it-db` / `it-db-ci` / `e2e-chrome`）。Make なしでも直接実行できる |
| `scripts/hooks/` | lefthook から呼ばれる Git hooks の実体 |
| `dev/postgres/` | 開発・検証用 PostgreSQL の Docker 構成（`TASK-5`）。契約テストを流す参照実装向けで、本番では Docker を必須にしない。手順は [dev/postgres/README.md](./dev/postgres/README.md) |
| `lefthook.yml` | Git hooks 定義（pre-commit / commit-msg） |
| `.editorconfig`・`.editorconfig-checker.json` | 文字コード・改行・インデントの宣言と、その検査の除外設定（ライセンス本文は除外） |
| `.shellcheckrc` | shellcheck が `source` 先（`scripts/lib.sh`）を追って検査するための設定 |
| `.envrc`・`.env.example` | direnv による `.env` の読み込みと、その雛形 |
| `.mcp.json` | Claude Code のプロジェクト共有 MCP サーバー定義（ベースは空） |
| `docs/spec/` | 仕様・フェーズ文書（private の submodule。前述の「仕様」） |
| `docs/security-review/` | 人のセキュリティレビュー（`TASK-83`）の資料・記録票（`TASK-82`）。範囲の各行と対応ビヘイビア ID の表など |
| `docs/records/` | TASK ごとの確認・走査の記録（判定は人が記録する） |
| `docs/interim/` | 暫定の実装の置き場と差し替え対象の台帳（`TASK-9`） |
| `docs/stubs/` | 検証用 stub（`TASK-7`・`TASK-8`）の振る舞いと実物との差の文書 |
| `.gitmodules` | submodule 定義（`docs/spec` を `update = none` で登録） |
| `LICENSE-MIT`・`LICENSE-APACHE` | ライセンス（前述の「ライセンス」） |
| `.github/workflows/` | CI・AI PR レビュー・外部ソース自動追従（後述の「CI」） |

## 必要なツール

| ツール | 用途 | 必須 |
|---|---|---|
| [GNU Make](https://www.gnu.org/software/make/) 3.81+ | `make` 入口 | ✓ |
| [lefthook](https://lefthook.dev/) | Git hooks | ✓ |
| [editorconfig-checker](https://github.com/editorconfig-checker/editorconfig-checker) | `.editorconfig` 準拠チェック | ✓ |
| [ShellCheck](https://www.shellcheck.net/) | シェルスクリプトの lint | ✓ |
| Rust toolchain（cargo・rustfmt・clippy） | `make fmt`・`make lint`（stable の 1.100 以上が要る。`D-2026-11-20-12`） | ✓ |
| [Python](https://www.python.org/) 3.11 以上（`python3`・標準の `tomllib`） | `make no-vendored`・`make lint`（`scripts/check-openssl-vendored.sh` が `.cargo/config*` の TOML の解析に使う。無い・3.11 未満だと検査不能で失敗する） | ✓ |
| [direnv](https://direnv.net/) | `.env` の自動読み込み | 任意 |
| Docker・Docker Compose v2 以降・openssl | 開発・検証用 PostgreSQL 、`make pg-check`・`make it-db`（[dev/postgres/README.md](./dev/postgres/README.md)） | 任意 |

macOS（Homebrew）の例:

```bash
brew install lefthook editorconfig-checker shellcheck direnv python@3.12
# python@3.12 は `python3` を PATH に入れないため、libexec/bin を PATH の先頭に足す（シェルの設定ファイルへ）
export PATH="$(brew --prefix python@3.12)/libexec/bin:${PATH}"
```

`libexec/bin` には versionless の `python3` が入っており、先頭に置くことで 3.11 以上の `python3` が選ばれます。

`make doctor` で導入状況を確認できます（読み取りのみで、何も導入・変更しません）。

## セットアップ

```bash
make doctor   # 必要なツールが揃っているか確認する
make setup    # Git hooks を有効化し、.env が無ければ .env.example から作成する
direnv allow  # direnv を使う場合のみ。.envrc の読み込みを許可する
```

`make setup` は何度実行しても安全です（既存の `.env` は上書きしません）。

`target/` は放置すると数十 GB になります。14 日以上使っていないビルド成果物は
`cargo sweep --time 14` で削除してください（導入: `cargo install cargo-sweep`）。
worktree はそれぞれ別の `target/` を持つため、不要になった worktree は `git worktree remove` で
`target/` ごと消し、残している worktree は `cargo sweep -r --time 14 .claude/worktrees` でまとめて掃除します。

## ビルド

前提は Rust toolchain（cargo・rustfmt・clippy）です。MSRV は 1.100、edition は 2024 と決まっています（`D-2026-11-20-12`）。そのため stable の 1.100 以上が要ります。ルートの `Cargo.toml` の `rust-version` で 1.100 を指定し、CI も 1.100 で検査します。

workspace の members は 11 crate で、ルートの `Cargo.toml` に明示して並べています（glob にしません）。
段階 1 の 5 crate（`core`・`secret`・`db-tests`・`client`・`plugin-oidc`）に、次の 6 つを足しています。
段階 2 の `frontend`・`webauthn-wasm` と、Hub が担当する crate（`server`・`db-pg`・`db-fandhe`・`db-migrate`）は
置きません（`LIB-14`・`LIB-15`）。

- `backend`（`fandhe-auth-backend`）: `TASK-9` の暫定の置き場として残しています（`D-2026-11-20-1`）。spec 上は段階 2 で、
  `LIB-15` の判定の記録までは中身を足しません（`D-2026-11-20-3`）。最初のリリースから除く仕組みは未決です（`Q-2026-11-20-1`）。
- `hub-stub`（Hub の検証用 stub。`TASK-7`）: 試験専用で、段階 1 のリリースのメンバーではなく、どの crate からも依存しません。
  名前と置き場は仮称・暫定で、人が確定します。
- `console-stub`（console の検証用 stub。`TASK-8`）: 試験専用で、扱いは `hub-stub` と同じです。
- `it-harness`（結合試験のハーネスの枠。`TASK-10`）: 試験専用で、扱いは `hub-stub` と同じです。構成は `docs/harness/integration.md` です。
- `e2e-harness`（passkey の E2E のハーネスの枠。`TASK-11`）: 試験専用で、扱いは `hub-stub` と同じです。構成は `docs/harness/e2e.md` です。
- `load-harness`（計測・負荷のハーネスの計測・集計・負荷の生成・対象の呼び出し・段階を変える実行・記録の出力の部品。`TASK-12`）: 試験専用で、どの crate からも依存されません。試験（dev）だけ `it-harness`・`hub-stub`・`secret`・`ring` に依存します。構成は `docs/harness/load.md` です。

手順は次の順です。直接実行する場合のコマンドも併記します。

| 順 | `make` | 直接実行 | 内容 |
|---|---|---|---|
| 1 | `make build` | `cargo build --workspace --locked` | workspace 全体をビルドする |
| 2 | `make test` | `cargo test --workspace --locked` | 試験を流す（現時点で試験は 0 件） |
| 3 | `make fmt` | `cargo fmt --all` | 整形を適用する（ソースを書き換える） |
| 4 | `make lint` | `scripts/lint.sh` | rustfmt の差分検査・clippy `-D warnings`・crate 間の依存の向き・vendored の検査 |
| 5 | `make crate-deps` | `scripts/check-crate-deps.sh` | crate 間の依存の向きだけを許可表と照合する |

約束:

- `--locked` を付け、`Cargo.lock` の外を解決しません
- `--all-features` でビルド・試験をしません（`DPOL-31`）
- crate を足すときは次の 3 つを同じ変更で行い、いずれも利用者の確認を経ます。`Cargo.toml` の members、
  `scripts/check-crate-deps.sh` の許可表、`LIB-15` の基準の記録。許可表に無い member があると検査は失敗します（fail-closed）
- `make build`・`make test` は CI の `build-test` ジョブで流します。`make lint` は CI の `lint` ジョブで流します（TASK-6）
- ターゲット名 `build`・`test` は暫定で、人が確定します

確認した結果は `docs/records/task-3-build-check.md` に残しています（判定は人が記録します）。

## コマンド

| コマンド | 直接実行 | 内容 |
|---|---|---|
| `make help` | `scripts/help.sh` | 操作の一覧を表示する |
| `make doctor` | `scripts/doctor.sh` | 開発環境を診断する |
| `make setup` | `scripts/setup.sh` | Git hooks 有効化と `.env` 雛形の配置 |
| `make check` | `scripts/check.sh` | editorconfig-checker + shellcheck（ソースは変更しない） |
| `make fmt` | `cargo fmt --all` | rustfmt で整形を適用する（ソースを書き換える） |
| `make lint` | `scripts/lint.sh` | rustfmt の差分検査 + clippy `-D warnings` + crate 間の依存の向きの検査 + vendored の検査（CI の `lint` ジョブと同じ契約） |
| `make crate-deps` | `scripts/check-crate-deps.sh` | crate 間の依存の向きを許可表と照合する（TASK-663 で拡張予定。外部の禁止 crate は `make lib-deps`） |
| `make lib-deps` | `scripts/check-lib-deps.sh` | 本体の crate（core・secret・db-tests・client）の通常の依存（normal・build。既定 feature + `PROD_FEATURES`）に、禁止の crate（Plugin・Hub・SDK・HTTP サーバー・DB ドライバー・非同期ランタイム）が 0 件かを `cargo tree` で検査する（LIB-2・TASK-714）。禁止の表は〔提案〕・暫定。全 feature の走査は参考の警告。表が空・対象が無い・`cargo tree` の失敗は検査不能（2）。陰性対照つき。`make lint` への結線は #5164。名前は暫定 |
| `make no-vendored` | `scripts/check-openssl-vendored.sh` | `openssl-sys` の `vendored` feature が 0 件かを `cargo tree -e features` で検査する。合否は試験専用メンバーを除いた本番対象メンバーの normal・build 依存に限る。feature は既定 + 環境変数 `PROD_FEATURES`（`<crate>/<feature>` のカンマ区切り。無ければ空）で、全 feature と試験専用メンバーを含む走査は参考の警告、dev 依存は対象外。メンバーが本番・試験専用のどちらの表にも無ければ検査不能（2）。静的リンクを強いる設定（環境変数と repo の `.cargo/config*` の `[env]` の `OPENSSL_STATIC`・`<TARGET>_OPENSSL_STATIC`・`PKG_CONFIG_ALL_STATIC`・`<TARGET>_PKG_CONFIG_ALL_STATIC`。上流が実際に読むのは、openssl-sys 0.9.117 が `OPENSSL_STATIC` と大文字・`_` 化した `<TARGET>_OPENSSL_STATIC`、pkg-config 0.3.34 が接頭辞なしの `OPENSSL_STATIC` と `PKG_CONFIG_ALL_STATIC`。`<TARGET>_PKG_CONFIG_ALL_STATIC`・ハイフン入りの名前・接尾辞の `OPENSSL_STATIC_<target>`・`PKG_CONFIG_ALL_STATIC_<target>` は上流が読まないが、安全側に多めに数える。上流の版を上げたら確かめ直す）が 1 件でもあれば違反（R4）で、本番の木の `openssl-sys` の件数と版の記録の宛先（Hub `AOPS-2`）を参考として出す。違反（1）と検査不能（2）を終了コードで分けられるのはスクリプトを直接実行したときだけで、`make` 経由では GNU Make が中のコマンドの 1 も 2 に変える（陰性対照つき。DPOL-13・TASK-667。名前は暫定） |
| `make it-db` | `scripts/it-db.sh` | ハーネスの DB 結合試験（Docker 必須・`#[ignore]` の試験を流す。CI では流さない。名前は暫定） |
| `make it-db-ci` | `scripts/it-db-ci.sh` | CI の DB ジョブの入口（Docker・Docker Compose・openssl 必須・枠の試験 `db_job_frame` だけを流す。0 件は失敗。名前は暫定） |
| `make e2e-chrome` | `scripts/e2e-chrome.sh` | ハーネスの Chrome の E2E（chromedriver 必須・`#[ignore]` の試験を流す。結果は〔仮想〕。CI では流さない。名前は暫定） |
| `make e2e-firefox` | `scripts/e2e-firefox.sh` | ハーネスの Firefox の E2E（geckodriver と Firefox 本体が必須。仮想 authenticator の WebDriver エンドポイントが使える組み合わせであること〔記録のある組み合わせは Firefox 156〜158 + geckodriver 0.37.1。2 つの pref は `moz:firefoxOptions.prefs` で渡す。この repo では未実行〕・`#[ignore]` の試験を流す。結果は〔仮想〕。CI では流さない。名前は暫定） |
| `make load-trial` | `scripts/load-trial.sh` | 計測・負荷のハーネスの小さな試し実行 `trial_run`（Docker 不要・stub が相手・負荷の記録の本文を標準出力に出す・0 件は失敗。結果は〔仮想〕。CI では流さない〔同じ試験は `make test` に入っている〕。名前は暫定。手順は `docs/harness/load.md`） |
| `make pg-check` | `scripts/dev-pg-check.sh` | 開発・検証用 PostgreSQL の起動の確かめ（起動中のインスタンスが対象。起動・停止はしない。CI では流さない。名前は暫定） |
| `make secret-scan` | `scripts/hooks/secret-scan.sh --tracked` | 追跡ファイル全体の秘密情報の混入検査（検査のみ。値は出さず件数で判定。`.env` 系の名前の検査は全パスに掛かり、本文の走査の除外は、検出器の repo の `skills-lock.json` に登録された `.agents/skills/<名前>/` だけで、本文の走査にだけ効く〔`*.lock`・`.claude/skills/` は除外しない。#6000。登録名のディレクトリの下は既存ファイルへの追記・新規ファイル・リネームによる移動も走査されない残る限界で、TASK-6・#6000 で追跡し人のレビューで確かめる〕。ローカルの走査（検出器と走査対象が同じ checkout）では、HEAD にコミット済みの一覧・作業ツリーの一覧・index の一覧のすべてに登録された名前（共通部分）だけを除外し、一部にしか無い名前は走査する。CI では保護された main の検出器の一覧の名前だけを使い、PR の head の一覧から名前を足さない（head の index の一覧は検証し、不正なら 2。除外は検出器の一覧と head の index の一覧の共通部分だけで、PR が登録を削除した名前は走査する）。index は走査の最初に固定し、走査中に変われば 2。`secret scan (tracked files)` ジョブが `scripts/secret-scan-ci.sh` 経由で呼ぶ〔#51〕。名前は暫定） |
| `make scan-canary` | `scripts/secret-scan-canary.sh` | `make secret-scan` の陰性対照（ダミーで検出器が 1 を返し、`--summary` の集計行が期待と完全に一致し、ダミー値が出力に無いことを確かめる。何も変更しない。名前は暫定） |
| `make test-hooks` | `scripts/hooks/test-hooks.sh`（入口。本体は `test-hooks-N.sh`、共有部は `test-hooks-core.sh`・`test-hooks-lib.sh`・`test-hooks-sc-lib.sh`） | フックスクリプト自体の回帰テスト（一時 git リポジトリで検出・許可ケースを検証。bash + git が基本。part 12〔lib-deps〕だけ cargo が要り、無ければローカルは TAP の `# SKIP`、CI では失敗。作業場所は repo の `target/` の下） |
| `make crypto-scan` | `scripts/check-crypto-primitives.sh` | 暗号の基本処理を自前で実装した箇所を発見的に走査する（`LIB-21`・`DPOL-4`・`DPOL-47`・`TASK-715` 3/4）。対象は `crates/` の追跡ファイルと未追跡の無視されていないファイル。合否は本番メンバー（core・secret・db-tests・client・plugin-oidc・backend）の C1〜C5（暗号定数・インラインアセンブリ・ネイティブのソース・暗号 trait の自前実装・rotate と wrapping の併存）で、試験専用メンバーと N1・N2 は参考の警告。分類が表に無いメンバーは検査不能（2）。限界は記録 `docs/records/task-715-crypto-deps.md` を参照。名前は暫定 |

## 開発・検証用 DB

契約テストを流す参照実装向けの PostgreSQL を Docker で起動する構成です（`TASK-5`）。本番の DB の所有・接続・migration は
Hub `ASTORE-5` 以降の担当で、ここでは扱いません。起動・初期化・停止・起動の確かめの手順は
[dev/postgres/README.md](./dev/postgres/README.md) にまとめています。接続設定の雛形は `.env.example` です（パスワードは置かず、実行時に生成します）。

## Git hooks

`make setup` 後、以下が自動実行されます。

- **pre-commit**
  - 簡易シークレット検知（`.env` 系ファイルの追加、トークン形式・秘密鍵・認証情報入り URL・ハードコード値）。
    保守的なヒューリスティックであり、網羅的なスキャナの代替ではありません
  - staged ファイルの editorconfig-checker
  - staged の `*.sh` に対する shellcheck
- **commit-msg**: [Conventional Commits](https://www.conventionalcommits.org/ja/) 形式の検証
  （`<type>[(<scope>)][!]: <要約>`、type は `feat` `fix` `docs` `style` `refactor` `perf` `test` `build` `ci` `chore` `revert`）

hooks に引っかかった場合は原因を修正してから再コミットしてください。`--no-verify` によるバイパスは行いません。

開発速度を落とさないため、hooks には staged ファイル限定で一瞬で終わる検査と、コミット単位でしか意味がない検査
（シークレット検知・コミットメッセージ）だけを置きます。テスト・ビルド・全体 lint など重い処理や CI と重複する検査は
CI（`.github/workflows/ci.yml`）で行います。時間予算（pre-commit 合計 1 秒未満）と判断手順は `.claude/rules/git-hooks.md` に定義しています。

フックスクリプト（`scripts/hooks/*.sh`）自体のテストもフックには置かず、CI の `test-hooks` ジョブ（`make test-hooks`）で
Linux・macOS（標準の bash 3.2 を含む）について実行します。`scripts/hooks/` を変更したらローカルでも `make test-hooks` を実行してください。
入口は `test-hooks.sh` で、試験の本体は `test-hooks-N.sh`（part）、共有部は `test-hooks-core.sh`・`test-hooks-lib.sh`・`test-hooks-sc-lib.sh` に置きます。足す試験は該当する part に書きます。

## CI

| ワークフロー | 内容 |
|---|---|
| `ci.yml` | `check`: ローカル・hooks と同じ `make check` を実行する（editorconfig-checker / shellcheck はバージョン固定 + SHA256 検証で導入）。`test-hooks`: フックスクリプト自体の回帰テスト（`make test-hooks`。ubuntu-latest・macos-latest）。`pr-title`: PR タイトルを commit-msg フックと同じスクリプトで検証する（squash merge でコミット件名になるため）。`build-test`: `make build` と `make test`（Rust の workspace のビルドと試験。ubuntu-latest）。`lint`: `make lint`（rustfmt の差分検査 + clippy `-D warnings` + crate 間の依存の向き + vendored の検査。ubuntu-latest）。`it-db-ci`: `make it-db-ci`（DB を使う結合試験の枠。Docker はハーネスが起動する。ubuntu-latest）。`deps-audit`: cargo-deny・cargo-audit の版を固定し、SHA256 を検証して導入するだけの枠（検査は未結線で TASK-680）。`ci-complete`: 全ジョブ結果の集約 |
| `ai-review.yml` | Fandhe-AI/actions の ai-review（codex）による PR 自動レビュー。Actions variable `CODEX_HOME_DIR` が未設定の間は skip される |
| `scheduled.yml` | 定期の実行の系統の枠（現状は手動起動 `workflow_dispatch` のみ。`pull_request`・`push` は持たない）。schedule の起動は、例外の見直しの後に TASK-681 で追加する予定。頻度の上限は規約の「日次以下」のままで、候補は日次、時刻とあわせて暫定。決めるのは TASK-681。検査は未結線で、緑でも DPOL-35 を満たしたことにはならない（TASK-681） |
| `update-external.yml` | エージェントスキル（`skills-lock.json`）と submodule（`.gitmodules`）の日次自動追従 PR。secrets は org の `SUBMODULE_PAT` を使う（`SKILLS_PAT` 未登録時は共通側が `SUBMODULE_PAT` へフォールバック）。作成する PR には `dependencies` / `automated` ラベルが付く |

- ruleset の required status checks には `ci-complete`（と ai-review の `codex / *`）を登録する。
  ruleset・マージ設定の導入は `setup-repo-guards` スキルの手順に従う
- `ci.yml` にジョブを追加したら `ci-complete` の `needs` にも必ず追加する（`scheduled.yml` の定期ジョブの扱いは `.claude/rules/github-actions.md` の「PR ごとの実行と定期の実行の振り分け」の節を参照）
- runner 方針（public は GitHub ホステッド、ai-review の review ジョブのみ self-hosted の例外）・発火条件・
  ジョブ追加時のチェックリスト・キャッシュ方針は `.claude/rules/github-actions.md` に定義している

## MCP サーバー

`.mcp.json` はベースとして空（`"mcpServers": {}`）にしてある。派生リポジトリで必要なサーバーを追加する。
API キー等は値を直接書かず `"${EXAMPLE_API_KEY:-}"` のように環境変数参照とし、実値は `.env`（direnv 経由）に置く。

## 環境変数

- 雛形は `.env.example`（キー名と空値・ダミー値のみ）、実値は `.env`（git 管理外）に置きます
- direnv を導入している場合、`.envrc` が `.env` をシェルへ読み込みます
- 個人用の追加設定は `.envrc.local`（git 管理外）に置けます

## 派生リポジトリでの拡張

1. **言語固有のコマンド**: `scripts/` に処理（例: `scripts/verify.sh`）を追加し、`Makefile` に 1 行で呼ぶターゲットと `## ` コメントを足し、`scripts/help.sh` の一覧も更新する（中身の無いターゲット `@true` 等は置かない。手順と規約は `.claude/rules/makefile.md`）
2. **`make check` の拡張**: `scripts/check.sh` に fmt チェック・lint 等を追記する（自動修正は別ターゲットに分ける）
3. **hooks の追加**: `.claude/rules/git-hooks.md` の時間予算と判断手順に従う。staged 限定で 1 秒未満の検査のみ `lefthook.yml` に job を追加し（実体が長くなる場合は `scripts/hooks/` へ切り出す）、テスト・ビルド等は `ci.yml` にジョブを追加して `ci-complete` の `needs` にも加える
4. **EditorConfig**: 言語ごとのセクション（例: `[*.rs] indent_size = 4`）を `.editorconfig` に追加する
5. **`.gitignore`**: ビルド成果物（`/target`・`/dist`・`node_modules/` 等）を追加する
6. **Rust の導入**: `.claude/rules/rust-build-profile.md` に従い、ルートの `Cargo.toml` に依存クレートの debuginfo を行番号のみにする dev プロファイルを入れ、README のセットアップ節に `cargo sweep` による `target/` の掃除手順を書く。ビルド設定（`[profile]`・`RUSTFLAGS`・`CARGO_PROFILE_*` 等）を変えるときは CI キャッシュのキーへの影響を確認する（`.claude/rules/github-actions.md` の「キャッシュ方針」）
