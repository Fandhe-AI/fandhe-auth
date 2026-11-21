//! DB の外の保存先の遮断の注入（TASK-13 の 4/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: DB の外の保存先（世代・台帳。ORST-17・ORST-2）への経路の遮断を結合試験から注入する。
//! 保存先の前に置くループバックの TCP 転送器（[`super::db`] の部品）の遮断と解除だけを外に出し、
//! 注入と同時に 1/8 の [`FaultTimeline`] へ `external_store_block` の開始と解除を記録する。
//! 呼び出し元は結合試験（試験ビルド）だけ。呼び出し先は [`super::db`] の転送器と [`super::record`]。
//!
//! 保存先の実体は未決（ORST-2。Hub `ASTORE-38` の決定待ち）。ここでは仮に「ループバックの上流への経路の
//! 遮断」とし、ディレクトリの権限を外す形（PoC-10）は入れない。
//! TODO(TASK-716): Host の構築時に転送器の接続情報を渡す結線を置く。
//!
//! 設計上の約束（2/8 と同じ）:
//! - ループバックだけを相手にする。
//! - 記録が成功してから状態を変える。基準線では記録が拒否し、注入もしない（fail-closed）。
//! - ポート・アドレスを誤りと `Debug` に出さない（SECR-11・SECR-23・SECR-26）。
//! - 環境変数・設定・feature で切り替える経路を作らない（DPOL-30〜32）。
//! - 遅延は注入の一覧に無いので出さない。
//!
//! 名前・形は仮置きで、人が確認する。

use std::fmt;

use super::db::{DbFaultError, DbFaultProxy, UpstreamStall};
use super::{FaultKind, FaultRecordError, FaultTimeline};
use crate::database::{DatabaseEndpoint, EndpointHandle};

/// 誤りの種類。ポート・アドレス・`io::Error` の文言を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalStoreFaultError {
    /// 待ち受けの開始に失敗した。
    StartFailed,
    /// すでに遮断している。
    AlreadyActive,
    /// 遮断していない。
    NotActive,
    /// 記録が拒否した（基準線である等）。状態は変えていない。
    Record(FaultRecordError),
    /// 解除の排出に失敗し、`Released` を記録せず遮断のまま戻った（`DbFaultError::DrainFailed` と同じ。ORST-17）。
    DrainFailed,
    /// 遮断の経路では起きない想定の内部の誤り。
    Internal,
}

impl fmt::Display for ExternalStoreFaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartFailed => f.write_str("external store fault error: start failed"),
            Self::AlreadyActive => f.write_str("external store fault error: already active"),
            Self::NotActive => f.write_str("external store fault error: not active"),
            Self::Record(e) => write!(f, "external store fault error: {e}"),
            Self::DrainFailed => f.write_str("external store fault error: drain failed"),
            Self::Internal => f.write_str("external store fault error: internal"),
        }
    }
}

impl std::error::Error for ExternalStoreFaultError {}

fn convert(e: DbFaultError) -> ExternalStoreFaultError {
    match e {
        DbFaultError::StartFailed => ExternalStoreFaultError::StartFailed,
        DbFaultError::AlreadyActive => ExternalStoreFaultError::AlreadyActive,
        DbFaultError::NotActive => ExternalStoreFaultError::NotActive,
        DbFaultError::Record(r) => ExternalStoreFaultError::Record(r),
        DbFaultError::DrainFailed => ExternalStoreFaultError::DrainFailed,
        DbFaultError::InvalidParam => ExternalStoreFaultError::Internal,
    }
}

/// DB の外の保存先の前に置く遮断の転送器。`Drop` と [`Self::stop`] でスレッドを止めて待つ。
pub struct ExternalStoreFaultProxy {
    inner: DbFaultProxy,
}

impl fmt::Debug for ExternalStoreFaultProxy {
    /// ポートと接続の情報を出さない（SECR-11）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalStoreFaultProxy")
            .finish_non_exhaustive()
    }
}

impl ExternalStoreFaultProxy {
    /// 上流を `127.0.0.1:<port>` に固定して始める。
    pub fn start_to_port(port: u16) -> Result<Self, ExternalStoreFaultError> {
        DbFaultProxy::start_to_port_as(port, FaultKind::ExternalStoreBlock)
            .map(|inner| Self { inner })
            .map_err(convert)
    }

    /// 上流への接続の直前に停止点を置いて始める（試験用。接続中で止まる状態を同期で作る。ORST-17）。
    /// 宛先（`127.0.0.1:<port>`）の決定と接続は転送器が行い、停止点は接続先も結果も変えない。
    pub fn start_to_port_with_stall(
        port: u16,
        stall: UpstreamStall,
    ) -> Result<Self, ExternalStoreFaultError> {
        DbFaultProxy::start_to_port_with_stall_as(port, FaultKind::ExternalStoreBlock, stall)
            .map(|inner| Self { inner })
            .map_err(convert)
    }

    /// 接続情報のハンドルに向けて始める（別の `Database` を保存先に見立てる場合）。
    pub fn start_for(handle: EndpointHandle) -> Result<Self, ExternalStoreFaultError> {
        DbFaultProxy::start_for_as(handle, FaultKind::ExternalStoreBlock)
            .map(|inner| Self { inner })
            .map_err(convert)
    }

    /// 待ち受けのポート（`127.0.0.1`）。
    pub fn listen_port(&self) -> u16 {
        self.inner.listen_port()
    }

    /// 上流の接続情報のポートを転送器のものに差し替えた複製。
    /// TODO(TASK-716): Host の構築時にこの値を渡す結線を置く。
    pub fn proxied_endpoint(&self) -> Option<DatabaseEndpoint> {
        self.inner.proxied_endpoint()
    }

    /// 上流へつないで転送を始めた接続の数。
    pub fn accepted_count(&self) -> u64 {
        self.inner.accepted_count()
    }

    /// 上流への connect の試行回数（診断用。接続処理の直前に加算するので到達の確認には使えず、成否も表さない）。
    pub fn upstream_attempt_count(&self) -> u64 {
        self.inner.upstream_attempt_count()
    }

    /// 転送器が accept した後に直ちに閉じた接続の数（遮断中に accept したもの・上流のハンドルが空・
    /// 上流への接続の失敗・確立中の遮断・解除のときに待ち行列から排出して切ったもの。2/8 と同じ契約）。
    /// 遮断中で待ち行列にあってまだ accept していない接続は、解除の排出までは数えない。
    pub fn refused_count(&self) -> u64 {
        self.inner.refused_count()
    }

    /// 保存先への経路を遮断する。開いている接続を切り、以後に転送器が accept した接続は直ちに切る。
    pub fn block(&mut self, timeline: &mut FaultTimeline) -> Result<(), ExternalStoreFaultError> {
        self.inner.block(timeline).map_err(convert)
    }

    /// 遮断を解除する。切れた接続は戻らないので、client はつなぎ直す。
    pub fn release_block(
        &mut self,
        timeline: &mut FaultTimeline,
    ) -> Result<(), ExternalStoreFaultError> {
        self.inner.release_block(timeline).map_err(convert)
    }

    /// 停止する。解除していない注入は `FaultTimeline::finish` の `unreleased_count` に出る。
    pub fn stop(self) {
        self.inner.stop();
    }
}
