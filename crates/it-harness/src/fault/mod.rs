//! 障害注入の種類の一覧（TASK-13 の 1/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: DB の遮断と遅延・インスタンスの停止・DB の外の保存先の遮断・Hub の stub の遅れと拒否・時計のずれ
//! の種類を 1 か所で数え、種類ごとの対象とパラメータの形を決める。呼び出し元は結合試験（試験ビルド）と、
//! 後続の段（2/8〜7/8）の注入の実装。注入の仕組み自体はここに無い（各段が `fault/` にファイルを足す）。
//! 記録の形式は [`record`]。2/8 で DB の遮断と遅延の注入（[`db`]）を、3/8 でインスタンスの停止の注入（`instance`。Unix 限定）を、4/8 で DB の外の保存先の遮断の注入（[`external_store`]）を、5/8 で Hub の stub の遅れと拒否の注入（[`hub_stub`]）を置いた。
//!
//! 設計上の約束:
//! - 種類は網羅の `match` で名前と対象を引く。種類を足して書き忘れるとコンパイルできない。
//! - パラメータは作るときに検査する（fail-closed）。0 の遅れ・0 のずれは拒否する。注入したと記録されたのに
//!   実際は何も起きていない、という状態を作らないため（DPOL-30〜32 の向き）。
//! - 遅れ・ずれの値は試験用の仮の値で、要件の値ではない（THRT-29・A-2026-10-28-1）。上限は置かない。
//! - 実行時の文字列を受ける経路を作らない（SECR-11・SECR-23・SECR-26）。環境変数・設定ファイルは読まない（DPOL-32）。
//! - 種類の参考: AVLB-31（時計のずれ）・AVLB-33（`SIGTERM`・`SIGKILL`）・ORST-17（DB の外の保存先）。
//!
//! 名前・形は仮置きで、人が確認する。

use std::fmt;
use std::time::Duration;

pub mod db;
pub mod external_store;
pub mod hub_stub;
#[cfg(unix)]
pub mod instance;
pub mod record;

pub use db::{DbFaultError, DbFaultProxy};
pub use external_store::{ExternalStoreFaultError, ExternalStoreFaultProxy};
pub use hub_stub::{HubStubFaultError, HubStubFaultInjector};
#[cfg(unix)]
pub use instance::{InstanceExit, InstanceFaultError, InstanceFaultHandle};
pub use record::{
    FAULT_RECORD_FORMAT, FaultEvent, FaultRecord, FaultRecordError, FaultTimeline, RunMode,
};

/// 注入の対象。[`crate::ComponentKind`] とは別の型（DB の外の保存先は構成要素ではないため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultTarget {
    Database,
    Instance,
    ExternalStore,
    HubStub,
}

impl FaultTarget {
    /// 記録に出す名前（ASCII 固定）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Instance => "instance",
            Self::ExternalStore => "external_store",
            Self::HubStub => "hub_stub",
        }
    }
}

/// 事象の段階。遮断・遅延・ずれは `Applied` と `Released` の組、`SIGTERM`・`SIGKILL` は `Applied` だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPhase {
    /// 注入の開始。
    Applied,
    /// 解除。
    Released,
}

/// パラメータを持たない種類。網羅の一覧は [`FaultCategory::CATALOG`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultCategory {
    DbBlock,
    DbDelay,
    InstanceSigterm,
    InstanceSigkill,
    ExternalStoreBlock,
    HubStubDelay,
    HubStubRefusal,
    ClockSkew,
}

impl FaultCategory {
    /// 全種類。担当の段は `docs/harness/fault-injection.md` の表（DB 2/8・停止 3/8・保存先 4/8・Hub 5/8・時計 6/8）。
    pub const CATALOG: [FaultCategory; 8] = [
        Self::DbBlock,
        Self::DbDelay,
        Self::InstanceSigterm,
        Self::InstanceSigkill,
        Self::ExternalStoreBlock,
        Self::HubStubDelay,
        Self::HubStubRefusal,
        Self::ClockSkew,
    ];

    /// 記録に出す名前（snake_case の ASCII）。
    pub fn name(self) -> &'static str {
        match self {
            Self::DbBlock => "db_block",
            Self::DbDelay => "db_delay",
            Self::InstanceSigterm => "instance_sigterm",
            Self::InstanceSigkill => "instance_sigkill",
            Self::ExternalStoreBlock => "external_store_block",
            Self::HubStubDelay => "hub_stub_delay",
            Self::HubStubRefusal => "hub_stub_refusal",
            Self::ClockSkew => "clock_skew",
        }
    }

    /// 注入の対象。時計のずれの対象は仮に instance とし、どの時計をずらすかは 6/8 で決める（AVLB-31）。
    pub fn target(self) -> FaultTarget {
        match self {
            Self::DbBlock | Self::DbDelay => FaultTarget::Database,
            Self::InstanceSigterm | Self::InstanceSigkill | Self::ClockSkew => {
                FaultTarget::Instance
            }
            Self::ExternalStoreBlock => FaultTarget::ExternalStore,
            Self::HubStubDelay | Self::HubStubRefusal => FaultTarget::HubStub,
        }
    }
}

/// Hub の stub の注入点（`fandhe-auth-hub-stub` の 4 つの口に対応させる。結線は [`hub_stub`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStubPort {
    Presentation,
    Logout,
    Heartbeat,
    Jwks,
}

impl HubStubPort {
    /// 記録に出す名前（ASCII 固定）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Presentation => "presentation",
            Self::Logout => "logout",
            Self::Heartbeat => "heartbeat",
            Self::Jwks => "jwks",
        }
    }
}

/// Hub の stub の拒否の形（hub-stub の `Refusal`・`JwksRefusal` への変換は [`hub_stub`]。`MalformedBody` は JWKS の口だけ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStubRefusalKind {
    Unreachable,
    /// HTTP の状態コード。100〜599 だけを受ける。
    Status(u16),
    MalformedBody,
}

/// 注入の種類とパラメータ。公開フィールドを直接組んでも、記録の入口で [`FaultKind::validate`] が再検査する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    DbBlock,
    DbDelay {
        delay_ms: u64,
    },
    InstanceSigterm,
    InstanceSigkill,
    ExternalStoreBlock,
    HubStubDelay {
        port: HubStubPort,
        delay_ms: u64,
    },
    HubStubRefusal {
        port: HubStubPort,
        refusal: HubStubRefusalKind,
    },
    ClockSkew {
        skew_ms: i64,
    },
}

/// パラメータの検査に落ちたことを表す。値を載せない（SECR-11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFaultParam;

impl fmt::Display for InvalidFaultParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fault error: invalid parameter")
    }
}

impl std::error::Error for InvalidFaultParam {}

/// 0 でなく `u64` のミリ秒に収まる遅れだけを受ける。
fn delay_to_ms(d: Duration) -> Result<u64, InvalidFaultParam> {
    u64::try_from(d.as_millis())
        .ok()
        .filter(|ms| *ms > 0)
        .ok_or(InvalidFaultParam)
}

impl FaultKind {
    /// DB の応答の遅れ。
    pub fn db_delay(delay: Duration) -> Result<Self, InvalidFaultParam> {
        Ok(Self::DbDelay {
            delay_ms: delay_to_ms(delay)?,
        })
    }

    /// Hub の stub の応答の遅れ。
    pub fn hub_stub_delay(port: HubStubPort, delay: Duration) -> Result<Self, InvalidFaultParam> {
        Ok(Self::HubStubDelay {
            port,
            delay_ms: delay_to_ms(delay)?,
        })
    }

    /// Hub の stub の拒否。
    pub fn hub_stub_refusal(
        port: HubStubPort,
        refusal: HubStubRefusalKind,
    ) -> Result<Self, InvalidFaultParam> {
        let k = Self::HubStubRefusal { port, refusal };
        k.validate()?;
        Ok(k)
    }

    /// 時計のずれ（ミリ秒。負は遅らせる向き）。0 は拒否する。
    pub fn clock_skew(skew_ms: i64) -> Result<Self, InvalidFaultParam> {
        let k = Self::ClockSkew { skew_ms };
        k.validate()?;
        Ok(k)
    }

    /// パラメータの検査。0 の遅れ・0 のずれ・範囲外の状態コードを拒否する（fail-closed）。
    pub fn validate(&self) -> Result<(), InvalidFaultParam> {
        let ok = match *self {
            Self::DbDelay { delay_ms } | Self::HubStubDelay { delay_ms, .. } => delay_ms > 0,
            Self::ClockSkew { skew_ms } => skew_ms != 0,
            Self::HubStubRefusal {
                refusal: HubStubRefusalKind::Status(s),
                ..
            } => (100..=599).contains(&s),
            // hub-stub の `Refusal` で表せないので、JWKS 以外の口は受けない。
            Self::HubStubRefusal {
                port,
                refusal: HubStubRefusalKind::MalformedBody,
            } => port == HubStubPort::Jwks,
            _ => true,
        };
        if ok { Ok(()) } else { Err(InvalidFaultParam) }
    }

    /// パラメータを落とした種類。
    pub fn category(&self) -> FaultCategory {
        match self {
            Self::DbBlock => FaultCategory::DbBlock,
            Self::DbDelay { .. } => FaultCategory::DbDelay,
            Self::InstanceSigterm => FaultCategory::InstanceSigterm,
            Self::InstanceSigkill => FaultCategory::InstanceSigkill,
            Self::ExternalStoreBlock => FaultCategory::ExternalStoreBlock,
            Self::HubStubDelay { .. } => FaultCategory::HubStubDelay,
            Self::HubStubRefusal { .. } => FaultCategory::HubStubRefusal,
            Self::ClockSkew { .. } => FaultCategory::ClockSkew,
        }
    }

    /// 注入の対象。
    pub fn target(&self) -> FaultTarget {
        self.category().target()
    }

    /// 解除の無い一回きりの事象か（`SIGTERM`・`SIGKILL`。停止の後の再起動の扱いは 3/8 で決める）。
    pub fn is_instantaneous(&self) -> bool {
        matches!(self, Self::InstanceSigterm | Self::InstanceSigkill)
    }
}

impl fmt::Display for FaultKind {
    /// 記録の 1 行の末尾に出すパラメータ部分（先頭に空白を含む。無ければ空）。静的な名前と数値だけ。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::DbDelay { delay_ms } => write!(f, " delay_ms={delay_ms}"),
            Self::HubStubDelay { port, delay_ms } => {
                write!(f, " port={} delay_ms={delay_ms}", port.name())
            }
            Self::HubStubRefusal { port, refusal } => {
                write!(f, " port={} refusal=", port.name())?;
                match refusal {
                    HubStubRefusalKind::Unreachable => f.write_str("unreachable"),
                    HubStubRefusalKind::Status(s) => write!(f, "status_{s}"),
                    HubStubRefusalKind::MalformedBody => f.write_str("malformed_body"),
                }
            }
            Self::ClockSkew { skew_ms } => write!(f, " skew_ms={skew_ms:+}"),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_names_are_unique_snake_case() {
        let mut names: Vec<_> = FaultCategory::CATALOG.iter().map(|c| c.name()).collect();
        assert!(
            names
                .iter()
                .all(|n| n.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        );
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn zero_params_are_rejected() {
        assert!(FaultKind::db_delay(Duration::ZERO).is_err());
        assert!(FaultKind::db_delay(Duration::from_micros(10)).is_err());
        assert!(FaultKind::db_delay(Duration::MAX).is_err());
        assert!(FaultKind::clock_skew(0).is_err());
        assert!(
            FaultKind::hub_stub_refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(99)).is_err()
        );
        assert!(FaultKind::db_delay(Duration::from_millis(5)).is_ok());
        assert!(FaultKind::clock_skew(-1).is_ok());
    }

    #[test]
    fn malformed_body_only_on_jwks() {
        for p in [
            HubStubPort::Heartbeat,
            HubStubPort::Presentation,
            HubStubPort::Logout,
        ] {
            assert!(FaultKind::hub_stub_refusal(p, HubStubRefusalKind::MalformedBody).is_err());
        }
        assert!(
            FaultKind::hub_stub_refusal(HubStubPort::Jwks, HubStubRefusalKind::MalformedBody)
                .is_ok()
        );
    }
}
