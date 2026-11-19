---
paths:
  - "crates/db-*/**"
  - "**/migrations/**"
description: >
  認証 DB の契約と契約テスト（本体の fandhe-auth-db-contract〔位置は未定〕・-db-tests）を書くときの規約。領域ごとの契約 trait（AUDB-1〜3）、
  契約テストの独立性と変異（AUDB-4・AUDB-6）、時刻の渡し方と列の型（AUDB-16・36）、再試行と成否不明の分類（AUDB-5・28）を定める。
  PostgreSQL の実装・migration・SQL・再試行の実行は Hub の担当（-db-pg・-db-migrate は作らない。ASTORE）なので、移管された ID は付記で引く。
---

# 認証 DB の契約の規約

正本は `docs/spec/04-behavior/db-contract.md`（AUDB）と `db-deployment.md`（AUDP）。契約テスト項目 `ADB-N` はビヘイビア ID ではない
（04-behavior/README.md の ID 規約 (e)）。crate の区分は `./coding-rust.md`、境界は `./library-boundary.md`。

## 本体の範囲

本体が持つのは、領域ごとの契約 trait と契約テスト（`-db-tests`）まで（DPOL-38 の改訂・LIB-14）。次は Hub の担当で、この repo では作らない。
作る前に利用者に確かめる。

- PostgreSQL の実装（`-db-pg`）・`fandhe-db` 向けの実装（`-db-fandhe`）・migration の bin（`-db-migrate`）・`Ops`（→ Hub `ASTORE-5`）。
- SQL の実装・`$n` のパラメータ・`AssertSqlSafe`（AUDB-43・AUDB-63 → Hub `ASTORE-91`・`ASTORE-92`）。段階 1 の crate（契約・契約テスト）に SQL を書かない（AUDB-3）。
  TASK-5 の PostgreSQL の参照実装の置き場は spec に記述が無く、未定。そこに SQL を書くときは、`$n` のパラメータだけにする（要求値は緩めない。`./security.md`）。
- migration の主体・冪等性・チェックサム・排他（AUDP-5・6・17・19・20・21 → Hub `ASTORE-51`・`52`・`66`・`68`・`69`・`70`。`TASK-648`〜`TASK-652` も移管）。
  migration の規則はこの repo の規約に写さず、Hub の `ASTORE` を引く。
- migration はこの repo に置かない（Hub の担当。`AUDP` の移管先）。`**/migrations/**` に置かれていたら指摘する。

## DB の選択

- 契約テストの参照実装は PostgreSQL を使う記述が残る（TASK-5。置き場は spec に未記載で、未定）。Docker は開発・検証用だけで、
  TASK-5 は契約テスト用 PostgreSQL の Docker 構成までに絞られた。本番の手順・設定・ビルドに Docker を前提にしない（AUDP-1・AUDP-2 → Hub `ASTORE-46`・`47`）。
  接続の資格情報は環境から注入し、repo に入れない（TASK-5。接続設定の雛形に資格情報を含めない）。
- `fandhe-db` への移行の判定は ADB-1〜26 の全てを実測で満たすまで行わない（CG-7・AUDP-24 → Hub `ASTORE-73`）。移行の判定は Hub の担当で、`-db-fandhe` は作らない。
- 下限として残る本体の行は `AUDP-33`（`db-deployment.md`。検討中。下限は緩めない）。

## 契約 trait

- 契約は領域ごとの意味のある操作（例: 認可コードを一度だけ消費する）にする。汎用の CRUD trait は 0 個（AUDB-1）。
- 各操作は原子的な 1 文の条件付き更新か、1 つのトランザクションで完結させる（AUDB-2）。
- 認証の経路では「読んでから書く」明示トランザクションを使わず、条件付き更新で先に消費する（AUDB-35 → Hub `ASTORE-84`。契約の形は AUDB-2）。
- SQL・ロック・SQLSTATE を契約 trait と誤り型に出さない（AUDB-3・DPOL-37。SQLSTATE・ロックを写す DB ごとの実装は Hub の `ASTORE-32`）。
- レート制限の計数は 1 文の UPSERT の加算式で行う。計数を読んでから書かない（AUDB-27）。共有カウンターの保存の実装は Hub。
- 監査は業務トランザクションの `ROLLBACK` の後に書き、失敗の監査が巻き戻らないようにする（AUDB-26）。
- 貸出・冪等キー・取りこぼし 0 の契約（AUDB-22）は本体（行内分割）。配送の実行は Hub の AEVT、DB の貸出は Hub の `ASTORE-25`。

## 契約テスト（`-db-tests`）

- 契約テストの本体は PostgreSQL の実装にも `sqlx` にも依存しない（DPOL-37・AUDB-3）。組込み先が `dev-dependencies` で自分の実装に流せる公開 crate とする（LIB-11）。
- 変異を入れた実装を全て検知できること（AUDB-4）。変異スイッチは既定で無効の feature か別 crate（DPOL-31。`./coding-rust.md`）。
- 契約テストを通すために安全条件や期待を緩めない（AUDB-6。同趣旨の `AUDB-69` は → Hub `ASTORE-2`）。通らないときは実装を直すか、利用者に伺う。
- DB の型の往復試験は Hub（AUDB-36 の行内分割。往復の部分は Hub の `ASTORE-33`）。メモリの代替アダプターで契約テストを流す（LIB-6）。

## 時刻と列の型

- 時刻は呼び出し側から引数で渡す。DB の現在時刻関数（`now()`・`CURRENT_TIMESTAMP`）を使わない（AUDB-16・AUDB-36。受ける側は `Clock`〔仮称。HOST-1〕）。
- 時刻の列は UTC のミリ秒の整数。`timestamptz`・`TIMESTAMP` を使わない（AUDB-16・AUDB-36）。
- 期限は `now < expires_at` のときだけ有効。境界は「有効・期限切れ・期限切れ」（`exp − 1ms`・`exp`・`exp + 1ms`）で、結果は渡した時刻だけで決まる（AUDB-16）。

## 再試行と成否不明

- 誤りは「再試行で解ける失敗」「契約の結果としての拒否（消費済み・期限切れ等）」「成否不明」を区別する。分類は DB ごとの実装の中（AUDB-5）。
- COMMIT の応答が得られない等の成否不明は、成功ではなく「失敗（成否不明）」を返す。再試行の前に要求 ID で照会し、二重の発行を作らない（AUDB-28）。
- 直列化の失敗・デッドロックで再試行しても、原子性・一度だけを保つ（二重の消費 0 件。巻き戻った中で読んだ値を成功として返さない。AUDB-29。`40001`・`40P01` の対応は Hub の `ASTORE-32`）。
- 一意制約の違反など契約の結果としての拒否は、再試行せず契約の結果（登録済み・消費済み）として返す（AUDB-30）。系列の失効のデッドロック 0 件は AUDB-33（検討中）。
- 再試行の上限・`40P01` の扱い（AUDB-31・32・34 → Hub `ASTORE-34`・`35`・`83`）は DB の実装側の規則で、ここには写さない。

## 関連 rule

- crate の区分・誤り応答: `./coding-rust.md` / 境界・段階: `./library-boundary.md`
- 秘密値の列の形（ハッシュ・HMAC・KEK）: `./secret-values.md`
- `sqlx` 等の依存の版: `./dependency-policy.md`
