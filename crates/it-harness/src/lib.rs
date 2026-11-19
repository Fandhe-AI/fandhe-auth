//! 結合試験のハーネスの枠を置く crate（TASK-10）。試験専用で、本番のビルドには入らない。
//!
//! 役割: 最小の組込み先（TASK-716）・DB（dev/postgres。TASK-5）・Hub の stub（TASK-7）・console の stub
//! （TASK-8）をつなぐ本体の結合試験で、構成要素の起動と停止の入口を 1 か所にそろえる。呼び出し元は
//! 結合試験（試験ビルド）だけで、構成要素の実体は [`Component`] を実装して置く（DB は 2/6、Hub の stub は 3/6 で置いた）。
//!
//! 範囲外: 複数インスタンス・前段のリバースプロキシ・TLS の終端を含む統合の試験は Hub の `ARUN` の担当で、
//! ここには置かない。E2E（TASK-11）・計測と負荷（TASK-12）も別のハーネスで、区別のため `it-` を付けた。
//!
//! 依存の向き: どの crate もこの crate に依存しない（LIB-2・LIB-4）。段階 1 のリリースのメンバーではない
//! （LIB-14・LIB-15）。名前・置き場は仮称で、人が確定する。
//!
//! 安全: 検証を外す意味の分岐・feature は作らない（DPOL-30〜32）。誤りの値に秘密値・接続文字列を載せない
//! （SECR-11・SECR-23）。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
//!
//! 構成の下書きは `docs/harness/integration.md`。

#![forbid(unsafe_code)]

pub mod component;
pub mod database;
pub mod harness;
pub mod hub_stub;

pub use component::{Component, ComponentKind};
pub use database::{Database, DatabaseConfig, DatabaseEndpoint, EndpointHandle};
pub use harness::{
    CompositionEntry, CompositionRecord, Harness, HarnessConfig, HarnessError, HarnessErrorKind,
    RunningHarness,
};
pub use hub_stub::{HubStubComponent, HubStubHandle, HubStubSession};
