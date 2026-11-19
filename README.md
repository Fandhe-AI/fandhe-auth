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
| `rustfmt.toml` | rustfmt の設定（edition のみ。暫定） |
| `scripts/` | 処理の実体（`help` / `doctor` / `setup` / `check` / `lint` / `check-crate-deps` / `dev-pg-check`）。Make なしでも直接実行できる |
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
| Rust toolchain（cargo・rustfmt・clippy） | `make fmt`・`make lint`（版は未定。TASK-4 で決める） | ✓ |
| [direnv](https://direnv.net/) | `.env` の自動読み込み | 任意 |
| Docker・Docker Compose v2 以降・openssl | 開発・検証用 PostgreSQL と `make pg-check`（[dev/postgres/README.md](./dev/postgres/README.md)） | 任意 |

macOS（Homebrew）の例:

```bash
brew install lefthook editorconfig-checker shellcheck direnv
```

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

前提は Rust toolchain（cargo・rustfmt・clippy）です。版・MSRV・edition は暫定で、TASK-4 で人が決めます。

workspace の members は 8 crate で、ルートの `Cargo.toml` に明示して並べています（glob にしません）。
段階 1 の 5 crate（`core`・`secret`・`db-tests`・`client`・`plugin-oidc`）に、次の 3 つを足しています。
段階 2 の `frontend`・`webauthn-wasm` と、Hub が担当する crate（`server`・`db-pg`・`db-fandhe`・`db-migrate`）は
置きません（`LIB-14`・`LIB-15`）。

- `backend`（`fandhe-auth-backend`）: `TASK-9` の暫定の置き場として残しています（`D-2026-11-20-1`）。spec 上は段階 2 で、
  `LIB-15` の判定の記録までは中身を足しません（`D-2026-11-20-3`）。最初のリリースから除く仕組みは未決です（`Q-2026-11-20-1`）。
- `hub-stub`（Hub の検証用 stub。`TASK-7`）: 試験専用で、段階 1 のリリースのメンバーではなく、どの crate からも依存しません。
  名前と置き場は仮称・暫定で、人が確定します。
- `console-stub`（console の検証用 stub。`TASK-8`）: 試験専用で、扱いは `hub-stub` と同じです。

手順は次の順です。直接実行する場合のコマンドも併記します。

| 順 | `make` | 直接実行 | 内容 |
|---|---|---|---|
| 1 | `make build` | `cargo build --workspace --locked` | workspace 全体をビルドする |
| 2 | `make test` | `cargo test --workspace --locked` | 試験を流す（現時点で試験は 0 件） |
| 3 | `make fmt` | `cargo fmt --all` | 整形を適用する（ソースを書き換える） |
| 4 | `make lint` | `scripts/lint.sh` | rustfmt の差分検査・clippy `-D warnings`・crate 間の依存の向き |
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
| `make lint` | `scripts/lint.sh` | rustfmt の差分検査 + clippy `-D warnings` + crate 間の依存の向きの検査（CI の `lint` ジョブと同じ契約） |
| `make crate-deps` | `scripts/check-crate-deps.sh` | crate 間の依存の向きを許可表と照合する（TASK-714・TASK-663 で拡張予定） |
| `make pg-check` | `scripts/dev-pg-check.sh` | 開発・検証用 PostgreSQL の起動の確かめ（起動中のインスタンスが対象。起動・停止はしない。CI では流さない。名前は暫定） |
| `make test-hooks` | `scripts/hooks/test-hooks.sh` | フックスクリプト自体の回帰テスト（一時 git リポジトリで検出・許可ケースを検証。bash + git のみ） |

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

## CI

| ワークフロー | 内容 |
|---|---|
| `ci.yml` | `check`: ローカル・hooks と同じ `make check` を実行する（editorconfig-checker / shellcheck はバージョン固定 + SHA256 検証で導入）。`test-hooks`: フックスクリプト自体の回帰テスト（`make test-hooks`。ubuntu-latest・macos-latest）。`pr-title`: PR タイトルを commit-msg フックと同じスクリプトで検証する（squash merge でコミット件名になるため）。`build-test`: `make build` と `make test`（Rust の workspace のビルドと試験。ubuntu-latest）。`lint`: `make lint`（rustfmt の差分検査 + clippy `-D warnings` + crate 間の依存の向きの検査。ubuntu-latest）。`ci-complete`: 全ジョブ結果の集約 |
| `ai-review.yml` | Fandhe-AI/actions の ai-review（codex）による PR 自動レビュー。Actions variable `CODEX_HOME_DIR` が未設定の間は skip される |
| `update-external.yml` | エージェントスキル（`skills-lock.json`）と submodule（`.gitmodules`）の日次自動追従 PR。secrets は org の `SUBMODULE_PAT` を使う（`SKILLS_PAT` 未登録時は共通側が `SUBMODULE_PAT` へフォールバック）。作成する PR には `dependencies` / `automated` ラベルが付く |

- ruleset の required status checks には `ci-complete`（と ai-review の `codex / *`）を登録する。
  ruleset・マージ設定の導入は `setup-repo-guards` スキルの手順に従う
- CI にジョブを追加したら `ci-complete` の `needs` にも必ず追加する
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
