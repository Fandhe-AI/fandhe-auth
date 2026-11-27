---
paths:
  - "Makefile"
  - "scripts/**"
description: >
  Makefile・scripts/ を編集する際の方針。Makefile は scripts/*.sh を 1 行で呼ぶだけの薄い入口とし、
  処理の定義は scripts/ にのみ置く。ターゲット追加・改名・削除の手順、各ターゲットの契約、
  新規スクリプトの規約を規定する。scripts/hooks/** の書き方は git-hooks.md の担当。
---

# Makefile・scripts 編集ルール

## 目的

ローカル（`make <target>`）・Git hooks（`lefthook.yml`）・CI（`.github/workflows/ci.yml`）は同じコマンド契約を
共有する。入口は複数でも処理の定義は `scripts/` の 1 か所に置き、Makefile・CI・hooks で二重定義しない。
一般的な設計指針は make スキル（`.claude/skills/make/SKILL.md`、特に `references/architecture/command-contracts.md`）を参照する。

`scripts/hooks/**` の書き方（fail-closed・ロケール・`git diff` の扱い等）は `.claude/rules/git-hooks.md` が担当する。
本ルールはそれ以外の `scripts/*.sh` と `Makefile` を対象とする。

## Makefile の書き方

- レシピは `@scripts/<name>.sh [args]` の 1 行呼び出しに限定する
  - 派生リポジトリで言語ネイティブのコマンドが 1 行で済むもの（`cargo build` 等）は直書きしてよい
  - 複数行・`for` / `if` などの条件分岐・環境依存の処理（`curl`・`docker` 等）は `scripts/` へ移す
  - 例外は `help` のみ（`## ` コメントから一覧を生成する 1 行の `awk`）
- 全ターゲットを `.PHONY` に列挙する。`.DEFAULT_GOAL := help` を維持する
- 各ターゲットに `## <説明>。直接: scripts/<name>.sh` 形式のコメントを付ける
  - `help` は正規表現 `^[a-zA-Z0-9_-]+:.*## ` に一致する行だけを表示する。`## ` が無いターゲットは一覧に出ない
  - ターゲット名の表示幅は `%-11s`（`secret-scan` でちょうど 11 文字）。12 文字以上の名前を足す場合は幅も広げる
- 中身の無いターゲット（`@true`・`echo` だけ等）は置かない。呼び出し元が終了コードを信用して素通りするため
- GNU Make 3.81（macOS 標準）互換を保つ。`.ONESHELL`・`.RECIPEPREFIX`・`define` + `$(eval)` の多用など 3.82+ 前提の機能は使わない
- Makefile 冒頭のコメント（役割・3.81 互換・help の仕組み）は削らない

## ターゲットを追加・改名・削除するときの手順

1. 処理の実体を `scripts/<name>.sh` に書く（下記「スクリプトの規約」）
2. `Makefile` に 1 行レシピ・`.PHONY`・`## ` コメントを追加する
3. `scripts/help.sh` の一覧を同じ変更で更新する（手動で保守する一覧であり、自動同期の仕組みは無い）
4. 呼び出し元を洗い出して更新する

    ```bash
    grep -rn "make \|scripts/" .github lefthook.yml README.md scripts .claude/rules
    ```

    - `ci.yml` の `make check` / `make test-hooks`、`lefthook.yml`、README の「コマンド」表・「派生リポジトリでの拡張」が主な呼び出し元
5. `make help` と `scripts/help.sh` の表示が一致すること、`make check` が通ることを確認する
6. 改名・削除は終了コードや引数の互換を壊す変更として PR 本文に明記する

## ターゲットの契約（本リポジトリに実在するもの）

| ターゲット | 実体 | 契約 |
| --- | --- | --- |
| `help` | Makefile の `awk`（Make 非依存の一覧は `scripts/help.sh`） | 一覧の表示のみ |
| `doctor` | `scripts/doctor.sh` | 読み取りのみ。何も導入・更新・修復しない。必須ツール欠落時は非 0 |
| `setup` | `scripts/setup.sh` | 冪等（再実行しても安全）。`lefthook install` と `.env` 雛形の配置のみで、既存の `.env` は上書きしない。ツール本体は導入しない |
| `check` | `scripts/check.sh` | ソースを変更しない（自動修正しない）。editorconfig-checker + shellcheck。CI の `check` ジョブと同一 |
| `test-hooks` | `scripts/hooks/test-hooks.sh`（入口。本体は `test-hooks-N.sh`、共有部は `test-hooks-lib.sh`） | 一時 git リポジトリで検証し作業ツリーを変更しない。依存は bash + git のみ。CI の `test-hooks` ジョブと同一 |
| `secret-scan` | `scripts/hooks/secret-scan.sh --tracked` | 検査のみ。追跡ファイル全体の秘密情報の混入を規則表で検査し、規則 ID ごとの件数・合計・ファイル名だけを出す（値は出さない）。0 件で 0・検出で 1・検査不能で 2。CI の `secret scan (tracked files)` ジョブは `scripts/secret-scan-ci.sh` 経由で本検出器を呼ぶ（#51。TASK-6）。名前は暫定 |
| `scan-canary` | `scripts/secret-scan-canary.sh` | 検査のみ。元の index・作業ツリー・object store を変えない。実行時に組み立てたダミーを足した一時 index で、検出器が 1 を返し、`--summary` の出力が許可した集計行の集合と完全に一致し（余計な行があれば失敗）、ダミーの値が出力に無ければ 0。検出能力の異常は 1、検出器が検査不能（2 等）・準備や読み取りの失敗は 2。元のリポジトリの hooks は起動しない。CI の `secret scan (tracked files)` ジョブは `scripts/secret-scan-ci.sh` 経由で本検出器を呼ぶ（#51。TASK-6）。名前は暫定 |
| `build` | `cargo build --workspace --locked`（直書き） | ソースを変更しない。`--locked` で `Cargo.lock` の外を解決しない。`--all-features` は付けない（DPOL-31）。CI の `build-test` ジョブと同一（TASK-6）。名前は暫定 |
| `test` | `cargo test --workspace --locked`（直書き） | ソースを変更しない。`--locked`・`--all-features` なし。CI の `build-test` ジョブと同一（TASK-6）。名前は暫定 |
| `fmt` | `cargo fmt --all`（直書き） | rustfmt を適用する（ソースを書き換える唯一の make ターゲット） |
| `lint` | `scripts/lint.sh` | 検査のみ。`cargo fmt --check`・clippy `-D warnings`・`crate-deps`・`no-vendored`。CI の `lint` ジョブと同一（TASK-6） |
| `crate-deps` | `scripts/check-crate-deps.sh` | 検査のみ。crate 間の依存の向きを許可表と照合する。表に無い member・辺は失敗（fail-closed） |
| `lib-deps` | `scripts/check-lib-deps.sh` | 検査のみ。本体の crate の通常の依存に禁止の crate が 0 件かを `cargo tree` と陰性対照で確かめる（LIB-2・TASK-714）。終了コードは 0 合格・1 違反・2 検査不能（直接実行のとき）。名前は暫定 |
| `no-vendored` | `scripts/check-openssl-vendored.sh` | 検査のみ。`openssl-sys` の `vendored` が依存解決に 0 件かを陰性対照つきで確かめる（DPOL-13・TASK-667）。終了コードの 1（違反）・2（検査不能）の区別はスクリプトの直接実行に適用される（make 経由ではレシピが 1 で終了しても make 自体は 2 を返す）。名前は暫定 |
| `pg-check` | `scripts/dev-pg-check.sh` | 起動中の開発・検証用 PostgreSQL の起動の確かめ。起動・停止はしない。CI へは載せない（TASK-6）。名前は暫定。手順の正は `dev/postgres/README.md` |
| `it-db` | `scripts/it-db.sh` | ハーネス（`crates/it-harness`）の Docker を使う結合試験（`#[ignore]`）の明示の入口。Docker 必須・CI 外（TASK-6）。パスワードは試験プロセスの中で生成する。名前は暫定 |
| `it-db-ci` | `scripts/it-db-ci.sh` | CI の DB ジョブの入口。Docker・openssl 必須。枠の試験（`db_job_frame`）だけを `--ignored` で流し、一致 0 件は失敗（AUDB-6 の向き）。CI の `it-db-ci` ジョブと同一（TASK-6 5/10）。パスワードは試験プロセスの中で生成する。名前は暫定 |
| `e2e-chrome` | `scripts/e2e-chrome.sh` | ハーネス（`crates/e2e-harness`）の Chrome の仮想 authenticator の E2E（`#[ignore]`）の明示の入口。chromedriver 必須・CI 外（TASK-6）。結果は〔仮想〕で実機・Safari に数えない。名前は暫定 |
| `e2e-firefox` | `scripts/e2e-firefox.sh` | ハーネス（`crates/e2e-harness`）の Firefox の仮想 authenticator の E2E（`#[ignore]`）の明示の入口。geckodriver・Firefox 必須・CI 外（TASK-6）。結果は〔仮想〕で実機・Safari に数えない。名前は暫定 |
| `load-trial` | `scripts/load-trial.sh` | ハーネス（`crates/load-harness`）の小さな試し実行（`trial_run`）の明示の入口。Docker 不要・CI 外（TASK-12 10/10）。一致 0 件は失敗（AUDB-6 の向き）。記録は試験の一時ディレクトリに書かれ成功時に消えるため、`--show-output` で本文を標準出力に出す。本文は試験が秘密値の照合を通した後に固定の開始行・終了行で挟んで出し、入口は集計行が合格 1 件（無視 0 件）かつ区画がちょうど 1 件のときだけ区画の中身を出して 0 を返す（集計行の不一致のときは捕捉した出力を再掲せず固定の文言だけを出して 1。試験の捕捉を外す `RUST_TEST_NOCAPTURE` は子プロセスへ渡さない。区画が 1 件でない・開始行が終了行より後・区画が空または記録の形式の行が無いときは固定の文言（件数を含む）を出して 1。失敗時は捕捉した出力を再掲しない）。stub の結果を実物に数えない（`A-2026-10-28-9`）。名前は暫定 |

派生リポジトリでターゲットを足す場合の契約:

- `check` を拡張する場合も検査専用を保つ。自動修正は別ターゲット（例: `fix`）に分ける
- 生成物の削除（例: `clean`）はそのリポジトリが生成したディレクトリだけを対象にし、共有キャッシュ（`~/.cargo`・pnpm store 等）や追跡ファイルを消さない
- CI で呼ぶターゲットは、CI 側で独自のロジックを足さずに済む形にする（`.claude/rules/github-actions.md`）

## スクリプトの規約

- 先頭は `#!/usr/bin/env bash` と役割を説明する日本語コメント、続けて `set -euo pipefail`
- `scripts/lib.sh` を source し、その関数を使う

    ```bash
    DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    # shellcheck source=./lib.sh
    source "${DIR}/lib.sh"
    ```

    - 引数を取らないスクリプトは `reject_args "$(basename "$0")" "$@"`（誤用は終了コード 2）
    - 外部コマンドは `require_cmd <name>` で確認する（導入は試みない。案内は `doctor.sh` / README の役割）
    - 進捗は `log`（stderr）。作業ディレクトリは `cd "${ROOT_DIR}"` で固定し、cwd に依存しない
- 実行時の出力（`log`・エラーメッセージ・usage）は英語、コード内コメントは日本語（既存スクリプトに合わせる）
- 実行権限を付ける（`chmod +x`。git 上で `100755` になっていること）
- shellcheck を通す（`.shellcheckrc` により source 先の `lib.sh` も追って検査される）。`make check` で確認する
- macOS 標準の bash 3.2 でも動く書き方にする（`mapfile`・連想配列・`${var,,}` は使わない）
