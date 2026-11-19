---
paths:
  - "crates/**"
  - "**/*.rs"
description: >
  fandhe-auth（Hub が組み込むライブラリの本体）の Rust コードを書くときの規約。workspace と crate の表（段階 1・段階 2・Hub の担当・位置が未定。
  crates/<名前>）、crate 間の依存の向き（DPOL-1・3・7・39・40）、時刻・乱数・ID の注入、#![forbid(unsafe_code)] の既定と例外手順、
  MSRV 1.100・edition 2024（D-2026-11-20-12）、ログ・誤り応答の文言は英語（D-2026-11-20-13）、試験用の分岐を feature・環境変数で作らないこと（DPOL-27〜32）、本体の判定が返す誤りの種類と列挙防止
  （THRT-25・27 ほか。応答の出力は Hub）、本番に持ち込まない PoC 由来物を定める。境界は library-boundary.md、
  秘密値・DB・依存・Web のセキュリティ要求は別 rule を参照する。
---

# Rust コーディング規約（fandhe-auth）

根拠は `docs/spec/04-behavior/` の ID。引く ID の多くは「検討中」（本番の crate が無いため）だが、守る規則として扱う。
spec の読み方は `./spec-reference.md`。この repo は Hub が組み込むライブラリの本体で、HTTP の受付・DB の実装・配備は Hub が持つ（`./library-boundary.md`）。

## workspace と crate

本体の crate は repo ルートの 1 つの workspace に置く（DPOL-37・DPOL-41・LIB-14）。crate 名は接頭辞 `fandhe-auth-` 付き、
ディレクトリは接頭辞を除いた `crates/<名前>`（`D-2026-10-27-8`・`D-2026-10-28-1`・`D-2026-10-29-2`）。

名前は利用者の決定で、段階（下の区分）は spec 上〔提案〕である（`LIB-14`・`A-2026-10-30-9`。`LIB-14` は検討中）。
TASK-3・TASK-5 の追随は「推奨で決めた置き方」と書かれている（`05-tasks.md`・`D-2026-11-04-6`）。段階を確定した構成として書かない。

| 区分 | crate（ディレクトリ） | 役割 | 扱い |
| --- | --- | --- | --- |
| 段階 1 | `fandhe-auth-core`（`crates/core`） | 型・判定・プロトコル処理・ストレージ契約の trait（I/O なし） | 作る前提 |
| 段階 1 | `fandhe-auth-secret`（`crates/secret`） | 秘密値の型（`./secret-values.md`） | 作る前提 |
| 段階 1 | `fandhe-auth-db-tests`（`crates/db-tests`） | 契約テスト（`./db-layer.md`） | 作る前提 |
| 段階 1 | `fandhe-auth-client`（`crates/client`） | RP（ログインを使う側）向け。同じ repo の別 crate とする案（`Q-2026-10-30-7` の推奨） | 作る前提 |
| 段階 1 | `fandhe-auth-plugin-oidc`（`crates/plugin-oidc`） | 外部ログイン Plugin の OIDC 共通部品。同梱の Plugin（本体に含めない。PLG-26） | 作る前提。名前は決定済み（`D-2026-11-20-2`。crates.io の空きは未照会。公開するかは別に決める） |
| 段階 2 | `-backend`（`crates/backend`） | `fandhe-backend` の HTTP の型との接続の部品（任意。`TASK-9` の暫定の置き場。下記） | 暫定で workspace に残す（`D-2026-11-20-1`）。`interim` に中身を足す前、または `fandhe-backend` への依存を足す前の早い方で `LIB-15` の判定を記録する（`D-2026-11-20-3`）。最初のリリースには含めない扱い |
| 段階 2 | `-webauthn-wasm`・`-frontend` | ブラウザ側 WebAuthn モジュール・画面部品（任意） | `LIB-15` の基準を満たしたとき。作る前に利用者に確かめる |
| 段階 2 | SAML・ID トークンを使わない SNS の OAuth の Plugin | 外部ログイン Plugin | `LIB-15` の基準を満たしたとき。作る前に利用者に確かめる。名前は未決 |
| Hub の担当 | `-db-pg`・`-db-fandhe`・`-db-migrate`・`-server` | DB の実装・migration・認証サーバーの bin | 本体の workspace に含めない。作らない |
| 試験専用 | `fandhe-auth-hub-stub`（`crates/hub-stub`） | Hub の検証用 stub（仮称・TASK-7。本体・Plugin からは依存しない。試験専用の it-harness だけが依存する。TASK-10 (3/6)。許可表の行は人の確認待ち） | 段階 1 のリリースに含めない |
| 試験専用 | `fandhe-auth-console-stub`（`crates/console-stub`） | console の検証用 stub（仮称・TASK-8。本体・Plugin からは依存しない。試験専用の it-harness だけが依存する。TASK-10 (4/6)。許可表の行は人の確認待ち） | 同上 |
| 試験専用 | `fandhe-auth-it-harness`（`crates/it-harness`） | 結合試験のハーネスの枠（仮称・TASK-10。どの crate からも依存しない） | 同上 |
| 試験専用 | `fandhe-auth-e2e-harness`（`crates/e2e-harness`） | passkey の E2E のハーネスの枠（仮称・TASK-11。どの crate からも依存しない。許可表の行は人の確認待ち） | 同上 |
| 位置が未定 | `-db-contract`・`-types`・`-webauthn`・`-provider`・`-keys` | 契約 trait・共有型・WebAuthn アダプタ・Provider 層・鍵管理 | 独立 crate か `-core` の中かは未定（`PoC-15` の後に `LIB-15` の基準で決める。判定は人。`DPOL-38`・`DPOL-41`。TASK-4 の扱い〔2026-11-20 の追随〕の (4) が挙げる残りの対象〔構文木の走査・property test と fuzz・構造化ログと計測の口〕に無い） |

- 「Hub の担当」の crate の名前は、将来の切り出し候補として spec に残るだけで、本体の最初のリリースに含まれない（LIB-14）。
- `fandhe-auth-backend`（`crates/backend`）: 名前の採用は `D-2026-10-29-2` で spec に記録済み（`D-2026-11-06-6`）。役割は spec に合わせ、
  段階 2 の任意の接続部品とする（利用者の決定 2026-11-19。この役割の決定は spec に記録が無く、役割は `LIB-14` に合わせる。
  暫定の置き場として残すことの記録は `D-2026-11-20-1`）。`fandhe-auth-server`・outbox の配送・CSRF Gate の登録は Hub で、この repo では作らない
  （LIB-14・AUDB-22 の行内分割〔配送は Hub の AEVT〕・`csrf-ssr.md` の適用範囲）。当初の役割（ハンドラ・outbox を持つ lib）は 2026-10-29 時点の記録で、書き換えない。
- `crates/backend` の扱い（`D-2026-11-20-1`・`D-2026-11-20-3`。判定は人が行う）:
  - `TASK-9` の暫定の置き場（`interim` module）として workspace に残す。`LIB-14` の最初のリリースには含めない扱い。
    ただし今の `LIB-14` の操作では段階 2 のメンバーとして見つかる。メンバーから除くか・どの仕組みで除くかは未決（`Q-2026-11-20-1`）。
  - `interim` に最初の中身を足す前、または `fandhe-backend` への依存を足す前の早い方で、`LIB-15` の 4 基準の判定を記録する。
    **判定を記録するまで、`crates/backend` のどこにも（`interim` の外の新しい module・ファイルも含めて）中身を足さない（自動の実装を含む）**。記録は人が行う。進めてよいのは、記録欄の (1)〜(4) すべてに「満たす」という人の記入があるときだけで、「未記入」・「満たさない」の欄があれば止めて利用者に報告する。
    今の `crates/backend`（空の枠）は `LIB-15` の期待（記録のない切り出しが 0 件）を満たしておらず、利用者がそれを判定の時点までの外れとして受け入れた（`D-2026-11-20-3` の (ア)）。
  - 作らない枠: `form_limit`（`TASK-534` は移管。→ Hub `ARUN-46`・`ARUN-47`）、`trusted_proxy` の移管分
    （`CSRF-40`〜`42` → Hub `ARUN-43`、`CSRF-44` → Hub `ARUN-44`。`TASK-536` の accept ループと `CSRF-47` → Hub `ARUN-43`）。
    Gate の登録（Hub `ARUN-39`）・Cookie の発行（Hub `ARUN-36`・`ARUN-38`）も作らない。
  - 本体に残る部分（`LIB-1` で行ごとに判定）:
    - `CSRF-43`・`CSRF-52`（`TASK-535`）と `CSRF-45`・`CSRF-46`（`TASK-536`）の本体の部分は、`trusted_proxy` の枠の外で作る。
      枠を外す PR では、枠の中の `TODO(TASK-535)`・`TODO(TASK-536)` を消さずに移す。
    - 残り 4 枠（`csrf_gate`・`set_cookie`・`local_redirect`・`no_store`）の要求値の部分。
    - 置き場は未決で、`LIB-1` の判定のときに利用者が決める。置き場が `interim` の中なら、上のとおり先に `LIB-15` の判定が要る。
    - `CSRF-33`〜`35` など本体の契約を最初のリリースから外す決定ではない。
- 利用者の決定 2026-11-13（spec に記録なし）: `#![forbid(unsafe_code)]` を各 crate に置く（下記）。
- 表に無い crate を足すとき、段階 2・Hub の crate を作るときは、作る前に利用者に確かめる。

## 依存の向き

- `core` は非同期ランタイム・DB ドライバ・HTTP サーバー・`fandhe-backend`・`fandhe-frontend`・`web-sys` に依存しない（DPOL-1）。
- `core` は現在時刻・乱数・ファイル・ネットワークを直接読まない（DPOL-1）。時刻・乱数・ID は引数（または注入する trait）で受け、
  同じ入力で同じ出力にする。property test で確かめる（DPOL-3）。`SystemTime::now()`・`Uuid::new_v4()`・`rand::thread_rng()` を
  `core` に書かない。値を渡すのは組込み先（Hub。暗号論的な乱数の供給元の義務は Hub の `ARUN-11`）で、受ける側が本体（DPOL-3・HOST-1・HOST-2）。
- `client`・`types`・`frontend` は `fandhe-auth-db*` に依存しない（DPOL-7）。
- `types` は I/O なし・`wasm32` でビルドできる依存だけ（DPOL-39）。
- サーバー側のビルドに `web-sys` を入れない。画面部品と WebAuthn の wasm は別 crate（DPOL-40。段階 2 のブラウザ側モジュールに絞られた。サーバーのビルドの確認は Hub のビルドにも課される〔Hub の `ARUN-16`〕）。
- 本体の範囲は契約 trait・契約テストまで（DPOL-38 の改訂）。運用・検査用の `Ops`・`-db-migrate` は Hub の担当で、HTTP の経路から `Ops` へ到達できないことの確認も Hub（→ Hub `ASTORE-5`）。
- 本体から Plugin・組込み先への依存を作らない（LIB-2・LIB-4。`./library-boundary.md`）。
- `iss` は設定の型で受け、固定値を置かない（DPOL-48）。
- 判定・承認の経路に LLM・検索のクライアントを入れない（HAPR-1・SECR-28。詳細は `./security.md`）。
- 依存の向きの機械的な検査は TASK-3 で作る（TASK-3 は段階 1 に絞られた）。依存を足す手順は `./dependency-policy.md`。

## unsafe・MSRV・edition・整形

- 各 crate のルート（`lib.rs`・`main.rs`）に `#![forbid(unsafe_code)]` を置く（利用者の決定 2026-11-13。spec に記録なし）。
- 例外が要るときは、外す crate・理由・代替案を書いて利用者の承認を待つ。AI の判断で外さない・`deny` に緩めない。
- MSRV は 1.100、edition は 2024（利用者の決定。`D-2026-11-20-12`）。`rust-version`・`edition` にはこの値を書く。
  組込み先は stable の 1.100 以上が要る（spec の「main の読み」で、規約としては断定しない）。
  `rust-toolchain.toml` を置くか・toolchain の固定の仕方は **未定**（決まるまで推測で足さない）。
- rustfmt・clippy の設定とビルド手順は TASK-3 で決まる。決まるまで独自の lint 設定を足さない。
- Cargo のプロファイルは `./rust-build-profile.md`。

## 試験用の分岐を作らない

- 試験用のログイン・ログインを省く分岐・検証（CSRF・署名・PKCE・redirect URI・TLS）を外す分岐を、cargo feature・`cfg(feature …)`・
  環境変数・設定ファイル・引数・要求ヘッダのどれでも作らない（DPOL-28・DPOL-30・DPOL-32、HAPR-2、CSRF-7）。
  適合性テストは Provider を変更せず、本体に試験用のコードを入れない（DPOL-27。ブラウザの自動操作を伴う実行環境は → Hub `ARUN-20`）。
- 検証を外す意味の設定・分岐を作らない。`disable`・`skip_`・`bypass`・`no_csrf`・`insecure`・`allow_all`・`without_gate`・
  `no_gate`・`unchecked` の語は CI の走査が数える（DPOL-30）。検証と無関係な用途で使うときも、走査に掛かる前提で名前を選ぶ。
- 本番に現れない例外は、契約テストの変異スイッチ（DPOL-31）とメモリの代替アダプター（LIB-6）の 2 つだけ。条件は `./dependency-policy.md` の feature の節に従う。
- 本体の設定は引数（設定の型）で受け、検査関数で検証する（LIB-1）。`fandhe-auth-core` は環境変数・ファイルを直接読まない（HOST-22）。
  他の crate が環境変数を読むなら、接続先・待ち受け・ログの置き場などの設定値だけで、読む変数の一覧を残す（DPOL-32）。
  環境変数・秘密値の読み込みと待ち受けは組込み先の担当。
- 相手側が未決の値は設定で受け、試験用の仮の値で実装する。本番の値を推測で置かない（`A-2026-10-28-1`〜`14`）。

## 誤り応答と列挙防止

判定の意味・種類は本体、応答（状態コード・本文・ヘッダ）の出力は組込み先（Hub）が行う（`rate-limit.md`・`csrf-ssr.md` の適用範囲）。
要求値は緩めない（`D-2026-10-30-1`）。本体の契約が要求する応答の形は次のとおり（出力の実行は Hub）。

| 本体の判定が返す種類 | 組込み先が出す形（本体の要求。実行は Hub） | 根拠 |
| --- | --- | --- |
| レート制限の発動 | 検証の前に 429・本文 `{"error":"rate_limited"}`。トークンエンドポイントは 429・`Retry-After`・`error` は `invalid_client`。入口ごとの形は rate-limit.md の原文を確かめる | THRT-1・THRT-21 |
| CSRF の拒否 | 403・本文なし（Gate の登録と実行は Hub の `ARUN-39`） | CSRF-1 |
| 本文の上限超過 | 413（→ Hub `ARUN-46`・`ARUN-47`。旧 CSRF-37・CSRF-39） | 移管済み |
| fail-closed（DB 不達など） | 503・`Cache-Control: no-store`・本文空・`Retry-After`（入口ごとの差は原文を確かめる） | AVLB-22 |

- ログと誤り応答の文言は英語で書く（`D-2026-11-20-13`）。外に返す誤り応答の形は spec（THRT-1 等）に従い、ここで文言を決めない。
- 外に出す失敗理由は 1 種類に統一する（`login_failed`・`recovery_failed`・`invite_invalid`）。登録済み・未登録で本文と状態コードの差 0（THRT-25）。
- 存在しないハンドルにも同形の challenge を返す。偽の credential ID を作る鍵は永続化する（THRT-27）。
- 画面・応答にスタックトレースや DB の状態などの内部の事情を出さない（LGUI-14。画面の安全条件は `ui-safety.md`）。
- 誤り型の `Display`・`Debug` に秘密値や SQLSTATE を載せない（`./secret-values.md`・AUDB-5）。

## ログ

- 構造化ログ・計測は、本体の依存に置くのは出す口（facade）だけで、受ける側（subscriber・exporter・保存先）は組込み先が選ぶ（`D-2026-11-20-5`・`LIB-1`）。
  ログの出す口は `tracing`、計測の出す口は `metrics`（利用者の決定。`D-2026-11-20-8`）。
  試験でログを捕まえる受ける側の `tracing-subscriber` は、`SECR-23` の試験のためだけに dev-dependencies へ置いてよい。本番の依存の木には入れない（`D-2026-11-20-7` の K7）。
  口の feature の選びは、最小の試し実装で決める（**未定**。TASK-4 の成果物。#35）。依存の版の固定の手順は **未定**。
  秘密値を出さない要求は変えない（SECR-23・SECR-24・SECR-26・SECR-27）。
- 監査は業務トランザクションの ROLLBACK の後に書く（AUDB-26）。
  本体が他へ渡すデータに秘密値を出さない（SECR-23・SECR-24・SECR-26・SECR-27。前段のログの `SECR-25` は → Hub `ARUN-78`）。

## 本番に持ち込まない PoC 由来物

- 試験用のログイン、試験用の scope→claim 表（DPOL-28・CLM-25。試験コードの中にだけ置く）。
- PoC の秘密値型 3 実装（`fandhe-auth-secret` の 1 つに統合。SECR-15・SECR-31）。
- PoC の比較用 `alt/`（DPOL-10）と PoC の固定値（60 秒・300 秒など。要件の値ではない。04-behavior/README.md の ID 規約 (a)）。
- PoC のコードを写すときは、値が要件か PoC のパラメータかを原文で確かめる。

## 関連 rule

- 境界・段階: `./library-boundary.md` / spec の読み方: `./spec-reference.md`
- 秘密値: `./secret-values.md` / DB 層: `./db-layer.md`
- 依存: `./dependency-policy.md` / Web のセキュリティ要求: `./security.md`
