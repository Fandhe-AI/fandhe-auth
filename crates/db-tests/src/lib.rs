//! DB 契約テストの crate。ストレージ契約（AUDB）の契約テストの本体を置く。
//!
//! 組込み先が自分のストレージ実装に対して `dev-dependencies` で流せる形で提供する
//! （LIB-11・LIB-12、DPOL-37）。
//!
//! 依存しないもの: PostgreSQL の実装・`sqlx`・DB ドライバ。SQL・ロック・SQLSTATE を持たず、
//! 契約の操作を呼んで結果を確かめるだけにする（DPOL-37・AUDB-3）。非同期ランタイム・
//! HTTP サーバー側の枠組みも持たない（LIB-2）。
//!
//! 変異の試験は、既定で無効の feature か別 crate に閉じ、環境変数だけで有効になる経路を
//! 作らない（DPOL-31・AUDB-4）。メモリ実装は試験用で、本番のビルドの依存に現れない（LIB-6）。
//! どちらも置き場と名前は未定。契約 trait の置き場も未定（TASK-584・TASK-684）。
//!
//! 期待を緩めて試験を通さない（AUDB-6・AUDB-69）。
//!
//! `runner`: 契約テストを複数の DB 実装で流す枠（TASK-9。ケースは空で、名前は仮称）。

#![forbid(unsafe_code)]

pub mod runner;

// TODO(TASK-584): 契約 trait の骨組みと契約テストの本体の分離・走査を置く。
// TODO(TASK-716): メモリの代替アダプターの置き場（試験用 feature または別 crate）を決める。
// TODO(TASK-718): dev-dependencies で流せる提供形と変異を置く。
