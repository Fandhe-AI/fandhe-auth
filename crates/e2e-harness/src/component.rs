//! ハーネスが束ねる構成要素の種類と、起動・停止の契約。
//!
//! 呼び出し元は [`crate::harness`] だけ。実体は後続の段が [`E2eComponent`] を実装して置く。1/6 では置かない。

use crate::harness::E2eError;

/// 構成要素の種類。2 種だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum E2eComponentKind {
    /// RP 側（options を出し応答を検証する側）。実体は `TASK-716` と PKEY の実装タスクの完成後に置く。
    RelyingParty,
    /// ブラウザのドライバ（chromedriver・geckodriver）。実体は置き済み（2/6・3/6）。
    BrowserDriver,
}

impl E2eComponentKind {
    /// 起動順（RelyingParty → BrowserDriver）。停止はこの逆。
    pub const START_ORDER: [E2eComponentKind; 2] = [
        E2eComponentKind::RelyingParty,
        E2eComponentKind::BrowserDriver,
    ];

    /// 起動順の位置。
    pub(crate) fn rank(self) -> usize {
        Self::START_ORDER
            .iter()
            .position(|k| *k == self)
            .unwrap_or(usize::MAX)
    }
}

/// 構成要素の起動・停止の契約。誤りに秘密値・URL・パスを載せない（SECR-11・SECR-23）。
///
/// BrowserDriver の実体は Chrome・Firefox とも置き済み（`chrome`・`firefox`（unix のみ））。
///
/// TODO(TASK-716): RelyingParty の実体は最小の組込み先の完成後に置く。
pub trait E2eComponent {
    /// 種類。
    fn kind(&self) -> E2eComponentKind;
    /// 起動する。
    fn start(&mut self) -> Result<(), E2eError>;
    /// 停止する。
    fn stop(&mut self) -> Result<(), E2eError>;
}
