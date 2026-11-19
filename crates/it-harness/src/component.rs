//! ハーネスが束ねる構成要素の種類と、起動・停止の契約。
//!
//! 呼び出し元は [`crate::harness`] だけ。実体（DB・Host・stub）は後続の段が [`Component`] を実装して置く。

use crate::harness::HarnessError;

/// 構成要素の種類。`TASK-10` の範囲の 4 種だけ。
///
/// 複数インスタンス・前段のプロキシ・TLS の終端の種類は作らない（Hub の `ARUN` の担当）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    /// 最小の組込み先（TASK-716。実体は TASK-716 の完成後に置く）。
    Host,
    /// DB（`dev/postgres`。TASK-5。実体は 2/6）。
    Database,
    /// Hub の stub（TASK-7。実体は 3/6）。
    HubStub,
    /// console の stub（TASK-8。実体は 4/6）。
    ConsoleStub,
}

impl ComponentKind {
    /// 起動順（Database → Host → HubStub → ConsoleStub）。停止はこの逆。
    pub const START_ORDER: [ComponentKind; 4] = [
        ComponentKind::Database,
        ComponentKind::Host,
        ComponentKind::HubStub,
        ComponentKind::ConsoleStub,
    ];

    /// stub（実物ではない構成要素）か。`HubStub`・`ConsoleStub` が `true`（A-2026-10-28-9・ACPT-7）。
    /// 網羅の `match` で書くので、種類を足して判定を書き忘れるとコンパイルできない。
    pub fn is_stub(self) -> bool {
        match self {
            ComponentKind::HubStub | ComponentKind::ConsoleStub => true,
            ComponentKind::Database | ComponentKind::Host => false,
        }
    }

    /// 起動順の位置。
    pub(crate) fn rank(self) -> usize {
        Self::START_ORDER
            .iter()
            .position(|k| *k == self)
            .unwrap_or(usize::MAX)
    }
}

/// 構成要素の起動・停止の契約。誤りに秘密値・接続文字列を載せない（SECR-11・SECR-23）。
///
/// Database の実体は 2/6（#91）で [`crate::database::Database`] に置いた。
/// Hub stub の実体は 3/6（#92）で [`crate::hub_stub::HubStubComponent`] に置いた。
/// TODO(TASK-10): console stub は 4/6（#93）が置く。
/// TODO(TASK-716): Host の実体は最小の組込み先の完成後に置く（2/6 では置かない）。
pub trait Component {
    /// 種類。
    fn kind(&self) -> ComponentKind;
    /// 起動する。
    fn start(&mut self) -> Result<(), HarnessError>;
    /// 停止する。
    fn stop(&mut self) -> Result<(), HarnessError>;
}
