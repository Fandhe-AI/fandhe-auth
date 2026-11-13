---
paths:
  - "crates/**"
  - "**/*.rs"
description: >
  fandhe-auth の Rust コードを書くときの規約。workspace と crate 名の表（crates/<名前>）、crate 間の依存の向き
  （DPOL-1・3・7・39・40）、時刻・乱数・ID の注入、#![forbid(unsafe_code)] の既定と例外手順、MSRV・edition が
  未定であること、試験用の分岐を feature・環境変数で作らないこと（DPOL-27〜32）、誤り応答の形と列挙防止
  （THRT-25・27 ほか）、本番に持ち込まない PoC 由来物を定める。秘密値・DB・依存・Web のセキュリティ要求は別 rule を参照する。
---

# Rust コーディング規約（fandhe-auth）

根拠は `docs/spec/04-behavior/` の ID。引く ID の多くは「検討中」（本番の crate が無いため）だが、守る規則として扱う。
spec の読み方は `.claude/rules/spec-reference.md`。

## workspace と crate

全 crate を repo ルートの 1 つの workspace に置く（DPOL-36・DPOL-41）。crate 名は接頭辞 `fandhe-auth-` 付き、
ディレクトリは接頭辞を除いた `crates/<名前>`（`D-2026-10-27-8`・`D-2026-10-28-1`）。

| crate | ディレクトリ | 役割 |
| --- | --- | --- |
| `fandhe-auth-core` | `crates/core` | 認証コア（I/O なしのドメイン・状態遷移） |
| `fandhe-auth-webauthn` | `crates/webauthn` | WebAuthn アダプタ |
| `fandhe-auth-provider` | `crates/provider` | OIDC/OAuth Provider 層 |
| `fandhe-auth-keys` | `crates/keys` | 鍵管理 |
| `fandhe-auth-db-contract` / `-db-tests` / `-db-pg` / `-db-migrate`（bin） | `crates/db-*` | 認証 DB の層（`.claude/rules/db-layer.md`） |
| `fandhe-auth-db-fandhe` | `crates/db-fandhe` | fandhe-db 向け実装。移行の判定のときに足す（今は作らない） |
| `fandhe-auth-client` | `crates/client` | RP（ログインを使う側）向け |
| `fandhe-auth-secret` | `crates/secret` | 秘密値の型（`.claude/rules/secret-values.md`） |
| `fandhe-auth-types` | `crates/types` | サーバー・ブラウザ共有型 |
| `fandhe-auth-frontend` | `crates/frontend` | 画面（fandhe-frontend に依存） |
| `fandhe-auth-webauthn-wasm` | `crates/webauthn-wasm` | ブラウザ側 WebAuthn モジュール |
| `fandhe-auth-backend` | `crates/backend` | `fandhe-backend` に依る HTTP 層（lib。下記） |
| `fandhe-auth-server` | `crates/server` | 認証サーバーの bin（組み立てのみ） |

- `fandhe-auth-backend`（`crates/backend`）: `D-2026-10-29-1` (5) の案を採用（利用者の決定 2026-11-13。spec への記録〔DPOL-41 の表・`D-`〕は未反映で、spec repo への PR は利用者の指示を待つ）。
  `fandhe-backend` に依る部分（ハンドラ・ルーティング・Cookie/CSRF の配線・外向き HTTP・outbox）を持つ lib で、`-server` は組み立てだけにする。
  `backend` は設定・秘密値を引数で受け取り、環境変数・秘密値の読み込み・待ち受けは `-server` だけが行う（DPOL-31・DPOL-32）。
  ルーターは試験から組み立てられる形にし、全経路を回す試験（CSRF-7・CSRF-32・CSRF-51 など）に使う。
- 表に無い crate を足すときは、作る前に利用者に確かめる。

## 依存の向き

- `core` は非同期ランタイム・DB ドライバ・HTTP サーバー・`fandhe-backend`・`fandhe-frontend`・`web-sys` に依存しない（DPOL-1）。
- `core` は現在時刻・乱数・ファイル・ネットワークを直接読まない（DPOL-1）。時刻・乱数・ID は引数（または注入する trait）で受け、
  同じ入力で同じ出力にする。property test で確かめる（DPOL-3）。`SystemTime::now()`・`Uuid::new_v4()`・`rand::thread_rng()` を
  `core` に書かない。呼ぶのは組み立て側（server 等）。
- `client`・`types`・`frontend` は `fandhe-auth-db*` に依存しない（DPOL-7）。
- `types` は I/O なし・`wasm32` でビルドできる依存だけ（DPOL-39）。
- サーバーのビルドに `web-sys` を入れない。画面部品と WebAuthn の wasm は別 crate（DPOL-40）。
- 運用・検査用の `Ops` は `-db-migrate` と試験ビルドだけが依存する別 crate か feature に置き、認証サーバーの bin から到達させない（DPOL-38 の (イ)）。
- 判定・承認の経路に LLM・検索のクライアントを入れない（HAPR-1・SECR-28。詳細は `.claude/rules/security.md`）。
- 依存の向きの機械的な検査は TASK-3 で作る。依存を足す手順は `.claude/rules/dependency-policy.md`。

## unsafe・MSRV・edition・整形

- 各 crate のルート（`lib.rs`・`main.rs`）に `#![forbid(unsafe_code)]` を置く（利用者決定 2026-11-13）。
- 例外が要るときは、外す crate・理由・代替案を書いて利用者の承認を待つ。AI の判断で外さない・`deny` に緩めない。
- MSRV・edition・toolchain は **未定**（TASK-3・TASK-4 で決める）。決まるまで `rust-version`・`edition` の値を推測で書かない。
- rustfmt・clippy の設定とビルド手順は TASK-3 で決まる。決まるまで独自の lint 設定を足さない。
- Cargo のプロファイルは `.claude/rules/rust-build-profile.md`。

## 試験用の分岐を作らない

- 試験用のログイン・ログインを省く分岐・検証（CSRF・署名・PKCE・redirect URI・TLS）を外す分岐を、cargo feature・`cfg(feature …)`・
  環境変数・設定ファイル・引数・要求ヘッダのどれでも作らない（DPOL-28・DPOL-30・DPOL-32、HAPR-2、CSRF-7）。
  適合性テストは通常のログイン画面をブラウザ自動操作で通す（DPOL-27）。
- 検証を外す意味の設定・分岐を作らない。`disable`・`skip_`・`bypass`・`no_csrf`・`insecure`・`allow_all`・`without_gate`・
  `no_gate`・`unchecked` の語は CI の走査が数える（DPOL-30）。検証と無関係な用途で使うときも、走査に掛かる前提で名前を選ぶ。
- 例外として許すのは契約テストの変異スイッチだけ。既定で無効の feature か別 crate に閉じ、環境変数だけで有効になる経路は 0 件（DPOL-31）。
- 環境変数で読むのは接続先・待ち受け・ログの置き場などの設定値だけ。読む変数の一覧を残す（DPOL-32）。
- 相手側が未決の値は設定で受け、試験用の仮の値で実装する。本番の値を推測で置かない（`A-2026-10-28-1`〜`14`）。

## 誤り応答と列挙防止

- レート制限は検証の前に 429・本文 `{"error":"rate_limited"}`（THRT-1）。トークンエンドポイントの発動中は 429・`Retry-After`・
  `error` は `invalid_client`（THRT-21）。入口ごとの形は rate-limit.md の原文を確かめる。
- CSRF の拒否は 403・本文なし（CSRF-1）。本文の上限超過は 413（CSRF-37。デコードの誤りと分けるかは CSRF-39 で検討中）。
- fail-closed（DB 不達など）は 503・`Cache-Control: no-store`・本文空・`Retry-After`（AVLB-22。入口ごとの差は原文を確かめる）。
- 外に出す失敗理由は 1 種類に統一する（`login_failed`・`recovery_failed`・`invite_invalid`）。登録済み・未登録で本文と状態コードの差 0（THRT-25）。
- 存在しないハンドルにも同形の challenge を返す。偽の credential ID を作る鍵は永続化する（THRT-27）。
- 画面・応答にスタックトレースや DB の状態などの内部の事情を出さない（LGUI-14）。
- 誤り型の `Display`・`Debug` に秘密値や SQLSTATE を載せない（`.claude/rules/secret-values.md`・AUDB-5）。

## ログ

- 構造化ログ・計測の部品は **未選定**（TASK-4）。決まるまで部品を固定する依存を足さない。
- 監査は業務トランザクションの ROLLBACK の後に書く（AUDB-26）。ログに秘密値を出さない（SECR-23〜27）。

## 本番に持ち込まない PoC 由来物

- 試験用のログイン、試験用の scope→claim 表（DPOL-28・CLM-25。試験コードの中にだけ置く）。
- PoC の秘密値型 3 実装（`fandhe-auth-secret` の 1 つに統合。SECR-15・SECR-31）。
- PoC の比較用 `alt/`（DPOL-10）と PoC の固定値（60 秒・300 秒など。要件の値ではない。04-behavior/README.md の ID 規約 (a)）。
- PoC のコードを写すときは、値が要件か PoC のパラメータかを原文で確かめる。

## 関連 rule

- 秘密値: `.claude/rules/secret-values.md` / DB 層: `.claude/rules/db-layer.md`
- 依存: `.claude/rules/dependency-policy.md` / Web のセキュリティ要求: `.claude/rules/security.md`
