---
paths:
  - "crates/db-*/**"
  - "**/migrations/**"
description: >
  認証 DB の層（fandhe-auth-db-contract / -db-tests / -db-pg / -db-migrate、移行の判定で足す -db-fandhe）と
  migration を書くときの規約。PostgreSQL を初期の DB とし fandhe-db へは ADB-1〜26 の実測まで移さないこと、
  領域ごとの契約 trait（AUDB-1〜3）、時刻の渡し方と列の型（AUDB-16・36）、再試行と成否不明（AUDB-5・28〜34）、
  SQL への値の埋め込み禁止（AUDB-43）、migration の主体と冪等性（AUDP-5・6・17・19〜21）を定める。
---

# 認証 DB の層の規約

正本は `docs/spec/04-behavior/db-contract.md`（AUDB）と `db-deployment.md`（AUDP）。契約テスト項目 `ADB-N` はビヘイビア ID ではない
（04-behavior/README.md の ID 規約 (e)）。crate の一覧は `.claude/rules/coding-rust.md`。

## DB の選択

- 初期は PostgreSQL（PoC は 17）。Docker は開発・検証用だけで、本番の手順・設定・ビルドに Docker を前提にしない（AUDP-1・AUDP-2）。
- `fandhe-db` への移行は ADB-1〜26 の全てを実測で満たすまで行わない（CG-7・AUDP-24）。`-db-fandhe` は移行の判定のときに足す。

## 契約 trait（`-db-contract`）

- 契約は領域ごとの意味のある操作（例: 認可コードを一度だけ消費する）にする。汎用の CRUD trait は 0 個（AUDB-1）。
- 各操作は原子的な 1 文の条件付き更新か、1 つのトランザクションで完結させる（AUDB-2）。
- 認証の経路では「読んでから書く」明示トランザクションを使わず、条件付き更新で先に消費する（AUDB-35）。
- SQL・ロック・SQLSTATE は DB ごとの実装（`-db-pg` 等）の中だけに置き、契約 trait と誤り型に出さない（AUDB-3・DPOL-37）。
- レート制限の計数は 1 文の UPSERT の加算式で行う。計数を読んでから書かない（AUDB-27）。
- 監査は業務トランザクションの `ROLLBACK` の後に書き、失敗の監査が巻き戻らないようにする（AUDB-26）。

## 契約テスト（`-db-tests`）

- 契約テストの本体は PostgreSQL の実装にも `sqlx` にも依存しない（DPOL-37）。
- 変異を入れた実装を全て検知できること（AUDB-4）。変異スイッチは既定で無効の feature か別 crate（DPOL-31。`.claude/rules/coding-rust.md`）。
- 契約テストを通すために安全条件や期待を緩めない（AUDB-6・AUDB-69）。通らないときは実装を直すか、利用者に伺う。

## 時刻と列の型

- 時刻は呼び出し側から引数で渡す。`now()`・`CURRENT_TIMESTAMP` など DB の現在時刻関数を使わない（AUDB-16・AUDB-36）。
- 時刻の列は UTC のミリ秒の整数。`timestamptz`・`TIMESTAMP` を使わない（AUDB-16・AUDB-36）。
- 期限は `now < expires_at` のときだけ有効。境界は「有効・期限切れ・期限切れ」（`exp − 1ms`・`exp`・`exp + 1ms`）で、結果は渡した時刻だけで決まる（AUDB-16）。

## 再試行と成否不明

- 誤りは「再試行で解ける失敗」「契約の結果としての拒否（消費済み・期限切れ等）」「成否不明」を区別する。分類は DB ごとの実装の中（AUDB-5）。
- 再試行は分類と上限に従う（AUDB-28〜34）。`40P01` は分類の外として失敗にする（AUDB-34）。
- COMMIT の応答が得られない等の成否不明は、成功ではなく「失敗（成否不明）」を返す。再試行の前に要求 ID で照会し、二重の発行を作らない（AUDB-28）。

## SQL

- 値を SQL の文字列に埋め込まない。全て `$n` のパラメータで渡す（AUDB-43）。
- `sqlx` で動的な SQL を `AssertSqlSafe` で包むのは、値を含まない構造（表名・列名の選択など）だけ。包む箇所ごとに理由を一覧に書く（AUDB-43）。
- `$n` が使えない位置でも文字列の埋め込みで回避しない（AUDB-43）。`NULL` のパラメータは AUDB-63。
- 秘密値の列の形（ハッシュ・HMAC・KEK）は `.claude/rules/secret-values.md`。

## migration（`-db-migrate`）

- migration は専用の bin（`fandhe-auth-db-migrate`）で、アプリと別の DB の主体で実行する（AUDP-5・AUDP-6）。
- 認証サーバーの設定・環境・プロセスに migration 用の資格情報を持たせない。認証サーバーの主体に DDL の権限を与えない（AUDP-5）。
- 1 文ずつ前へ進む冪等な DDL にする。どの DDL の直後で止まっても再実行で同じ状態になる（AUDP-19・AUDP-20）。
- 適用済みの版のチェックサムが手元と一致しなければ起動を拒否する（AUDP-17・AUDP-20）。適用済みの migration を書き換えない。
- 同時実行の排他に `pg_advisory_lock` を使わない（AUDP-21）。
- 接続の資格情報は環境から注入し、repo に入れない（TASK-5）。Docker の起動定義と migration の bin の骨組みは TASK-5、migration の中身は TASK-648〜TASK-652。

## 関連 rule

- crate 構成・誤り応答: `.claude/rules/coding-rust.md`
- `sqlx` 等の依存の版: `.claude/rules/dependency-policy.md`
