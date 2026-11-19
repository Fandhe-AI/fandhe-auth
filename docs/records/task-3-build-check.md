# TASK-3 雛形の全体確認の記録

`TASK-3`（親 Issue #20）の 6/6 段目（Issue #26）。workspace の Cargo 設定・crate の雛形・依存の向きの検査・
ビルド手順の文書がそろったあとに、ビルドと検査を新しく流した結果を残す。

この文書は実行したコマンドと出力の事実だけを書く。判定・「確定」への変更・例外の承認は人が行う（`ACPT-7`）。
「試験が通る」とは書かない（試験は 0 件）。判定の欄は空けてある。

## 1. 目的と範囲

- 対象: `TASK-3` の成果物（workspace の Cargo 設定、crate の雛形、依存の向きの検査、ビルド手順の文書）の確認。
- 段階 2 の crate と Hub が担当する crate を含めていないこと（`LIB-14`・`LIB-15`）を数えて残す。
- 本文（spec）は転記しない。ID・件数・コマンドの出力だけを書く。
- 出力中の絶対パスは `<repo>` に置き換えた。

## 2. 基準

- 実行時の base: `origin/main` の `1bb17983df0e957b9599d0bdaffa63824de1cde2`。作業ブランチ `build/26-build-docs` は、
  この commit の上に本変更（未コミットの状態）を載せて流した。
- spec の pin: `1df12ad0c2e52fa3b4b00afdb9d0fb8c4b59a100`（`docs/spec`）。
- ツールの版:

| ツール | 版 |
|---|---|
| cargo | 1.99.1 (5f94df478 2026-08-27) |
| rustc | 1.99.1 (58cf8f9c3 2026-10-13) |
| rustfmt | 1.10.0-stable (58cf8f9c35 2026-10-13) |
| clippy | 0.1.99 (58cf8f9c35 2026-10-13) |
| GNU Make | 4.4.1 |
| editorconfig-checker | v3.7.0 |
| shellcheck | 0.11.0 |
| lefthook | 2.2.2 |

## 3. 構成の確認

- `cargo metadata --no-deps --format-version 1 --locked` の packages: 5 件。
  `fandhe-auth-core`・`fandhe-auth-secret`・`fandhe-auth-db-tests`・`fandhe-auth-client`・`fandhe-auth-plugin-oidc`。
- `ls crates` の直下: 5 件（`client`・`core`・`db-tests`・`plugin-oidc`・`secret`）。
- `Cargo.lock` の `name = ` の行: 5 件（外部の依存は 0）。
- 次の名前の、`Cargo.toml` の members 内の件数と `crates/` 直下のディレクトリの件数（数えたコマンド:
  `grep -c "\"crates/<名前>\"" Cargo.toml` と `ls crates | grep -cx "<名前>"`）。

| 区分 | 名前 | members | ディレクトリ |
|---|---|---|---|
| 段階 2 | `backend`・`frontend`・`webauthn-wasm` | 各 0 | 各 0 |
| Hub 担当 | `server`・`db-pg`・`db-fandhe`・`db-migrate` | 各 0 | 各 0 |
| `D-2026-10-27-8` の名前で段階 1 に入らないもの | `db-contract`・`types`・`webauthn`・`provider`・`keys` | 各 0 | 各 0 |

- `cargo tree --workspace --locked -e normal,build,dev --target all -i web-sys`:
  `error: package ID specification 'web-sys' did not match any packages`（依存グラフに `web-sys` が無い。`DPOL-40` の観点の事実）。

## 4. ビルドと試験

| コマンド | 終了コード | 出力の要点 |
|---|---|---|
| `make build` | 0 | `Finished dev profile [unoptimized + debuginfo]`。5 crate をコンパイル |
| `make test` | 0 | 10 件の `test result: ok. 0 passed; 0 failed; 0 ignored`（各 crate の lib と doc-test）。試験は 0 件 |

試験は 0 件のため、この結果は「ビルドが通り、試験の実行機構が動く」ことだけを示す。

## 5. 検査

| コマンド | 終了コード | 出力の要点 |
|---|---|---|
| `make lint` | 0 | `cargo fmt --all --check` と clippy `-D warnings` が通り、`crate dependency direction ok (5 members)` |
| `make crate-deps` | 0 | `crate dependency direction ok (5 members)` |
| `make check` | 0 | editorconfig-checker と shellcheck の指摘 0 件 |
| `make test-hooks` | 0 | `# passed: 69, failed: 0` |
| `make doctor` | 0 | 必須ツールすべて `ok`。`.env` は `not found`（`make setup` が雛形から作る旨の note） |

- `make help` と `scripts/help.sh` のターゲット名の集合: `help`・`doctor`・`setup`・`check`・`test-hooks`・`build`・`test`・`fmt`・
  `lint`・`crate-deps` の 10 件で一致（目で照合）。
- `Cargo.toml`・`Cargo.lock` の base からの差分: 無い（依存・メンバーは変えていない）。

## 6. fail-closed の確認

worktree の外の一時ディレクトリに `Cargo.toml`・`Cargo.lock`・`crates/`・`scripts/` をコピーし、仮の member
`crates/zz-probe`（`fandhe-auth-zz-probe`）を `Cargo.toml` と `Cargo.lock` に足して `scripts/check-crate-deps.sh` を流した。

- 終了コード 1。
  `error: unknown workspace member: fandhe-auth-zz-probe (add it to the allow table after confirming with the owner)`、
  続けて `crate dependency direction check failed (1 violation(s))`。
- 許可表に無い member は検査が失敗する（fail-closed）。一時ディレクトリは削除した。

## 7. 判定の欄

空欄。人が記録する（`ACPT-7`）。

## 8. 対象外と持ち越し

- `make build`・`make test`・`make lint`・`make crate-deps` の CI への搭載: `TASK-6`。
- MSRV・toolchain・edition の確定と `rust-toolchain.toml`: `TASK-4`（人が決める）。
- `cargo deny`・`cargo audit` の make ターゲット化: `.claude/rules/dependency-policy.md` の範囲。
- ターゲット名 `build`・`test` と許可表の値は暫定で、人が確定する。
- 親 Issue #20 の受け入れの判定: 人が行う。
