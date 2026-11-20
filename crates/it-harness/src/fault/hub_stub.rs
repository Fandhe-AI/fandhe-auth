//! Hub の stub の応答の遅れと拒否の注入（TASK-13 の 5/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: hub-stub の注入器（TASK-7 #66 の `HubFaultInjector`）を、1/8 の [`FaultTimeline`] と結ぶ。
//! 開始（`Applied`）と解除（`Released`）を記録してから注入の設定を差し替える。遅れと拒否の判定・内側を呼ばない
//! 約束（jti を既受信にしない・最終受信時刻を進めない・JWKS のキャッシュを変えない）は hub-stub 側の実装をそのまま使い、
//! ここでは作り直さない。呼び出し元は結合試験（試験ビルド）だけ。呼び出し先は hub-stub の注入器と [`super::record`]。
//!
//! 設計上の約束:
//! - 「TASK-7 の設定の仕組み」は hub-stub の `HubFaultConfig`・`FaultRule`・`CallWindow` と `set_faults` と読む。
//!   `HubStubConfig` には欄を足さない。
//! - 記録の期間の形（`Applied`〜`Released`）は、`CallWindow { first: 1, last: None }`（以降すべて）の規則で表す。
//!   解除はその規則を除いた設定の差し替え。回数の範囲を指定する注入は出さない。
//! - hub-stub は口ごとに最初に当たった 1 件の規則だけを使う。そのため口ごとに有効な遅れと拒否（各最大 1 件）を
//!   1 つの規則へ合成する。分けると片方が黙って落ち、記録では注入したのに効かない状態になる。
//! - 注入器は `new(wait, &timeline)` で 1 つの実行に結ぶ。別の実行の記録は `RunMismatch`、同じ実行への 2 つ目は `AlreadyBound`。
//!   未使用（`apply` が一度も通らない）の注入器を破棄すると結合を取り消し、結び直せる。使った注入器は印を残す。
//!   `finish` は注入器を消費し、有効な規則を次の実行（基準線を含む）へ持ち越さず、記録と呼び出しの記録を組で返す。
//! - 事象には呼び出しの記録の件数（`FaultEvent::log_pos`）を添え、期間と呼び出しを順序で照合できる。
//! - 記録が先、状態の変更が後。記録が拒否されたら状態を変えない。基準線では記録が拒否し、注入もしない（fail-closed）。
//! - `MalformedBody` は JWKS の口だけが受ける（hub-stub の `Refusal` で表せない）。他の口は `InvalidParam`。
//! - 遅れは `Wait` へ渡すだけ。眠る実装は出さない（セッションのロックを握ったまま待たないため。未決）。
//!   `HubStubHandle::with` のクロージャ内で受け口を呼ぶとセッションのロックを握ったまま `Wait` が走る。
//!   `Wait` は同じハンドルを参照しない（再入しない）ものに限る。
//! - 受け口は `timeline` を受け、注入器の実行と一致しなければ内側を呼ばず `RunMismatch`（基準線の呼び出しを注入器のログへ入れない）。
//! - hub-stub の呼び出しの記録（`InjectionLog.at` は試験の論理時刻）は [`FaultRecord`](super::FaultRecord) に入れず、
//!   [`HubStubFaultInjector::call_log`] で読み取り専用に出す。対応は順序だけで取る。
//! - 受け口は呼び出しごとに借りる。token は内側へ渡すだけで保持・記録しない（SECR-11・SECR-23〜27）。
//! - 環境変数・設定ファイル・feature で切り替える経路を作らない（DPOL-30〜32）。遅れの値は試験用の仮の値（THRT-29・A-2026-10-28-1）。
//! - 根拠: TASK-13・AVLB-18・AVLB-22・RVPG-28・OCL-36・Hub AUTHC-21。stub の合格を実物に数えない（A-2026-10-28-9・ACPT-7）。
//!
//! 対象外: Hub が呼ぶ側の `AuthPort`・`ChangeFeedPort`・`AuthJwksPort`（hub-stub の設計で対象外）。
//! 名前・形は仮置きで、人が確認する。

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fandhe_auth_hub_stub::fault::{
    CallWindow, FaultRule, HubFaultConfig, HubFaultInjector, InjectedRefusal, InjectionLog,
    JwksRefusal, Refusal, Wait,
};
use fandhe_auth_hub_stub::heartbeat::{Heartbeat, HeartbeatReceiver};
use fandhe_auth_hub_stub::jwks::{HubJwksEndpoint, JwksResponse};
use fandhe_auth_hub_stub::logout::{LogoutReceipt, LogoutRejection, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationRejection, ReceivedIdentity,
};

use crate::record::UtcTimestamp;

use super::{
    FaultKind, FaultPhase, FaultRecord, FaultRecordError, FaultTarget, FaultTimeline, HubStubPort,
    HubStubRefusalKind,
};

/// 誤りの種類。値・token を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStubFaultError {
    /// 種類が Hub の stub のものでない・パラメータの検査に落ちた。状態も記録も変えていない。
    InvalidParam,
    /// 同じ口で同じ系統（遅れ・拒否）の注入がすでに有効。
    AlreadyActive,
    /// 同じ種類・値の開始が無い。
    NotActive,
    /// 記録が拒否した（基準線である等）。状態は変えていない。
    Record(FaultRecordError),
    /// 注入器を作ったときの実行と別の実行の記録を渡された。状態も記録も変えていない。
    RunMismatch,
    /// この実行にはすでに注入器を結んである（1 つの実行に注入器は 1 つ）。
    AlreadyBound,
}

impl fmt::Display for HubStubFaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParam => f.write_str("hub stub fault error: invalid parameter"),
            Self::AlreadyActive => f.write_str("hub stub fault error: already active"),
            Self::NotActive => f.write_str("hub stub fault error: not active"),
            Self::Record(e) => write!(f, "hub stub fault error: {e}"),
            Self::RunMismatch => f.write_str("hub stub fault error: run mismatch"),
            Self::AlreadyBound => f.write_str("hub stub fault error: already bound"),
        }
    }
}

impl std::error::Error for HubStubFaultError {}

/// `finish` の失敗。注入器とタイムラインを返すので、正しい組で再試行できる（記録を失わない）。
pub struct HubStubFinishFailure<W: Wait> {
    /// 失敗の理由。
    pub error: HubStubFaultError,
    /// 渡された注入器。
    pub injector: HubStubFaultInjector<W>,
    /// 渡されたタイムライン。
    pub timeline: FaultTimeline,
}

impl<W: Wait> fmt::Debug for HubStubFinishFailure<W> {
    /// 理由だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubStubFinishFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// 受け口の呼び出しの誤り。内側へ委ねる前に実行の一致を確かめるので、別の実行の呼び出しは内側のログに入らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStubCallError {
    /// 注入器を作ったときの実行と別の実行のタイムラインを渡された。内側は呼んでいない。
    RunMismatch,
    /// 注入された拒否（hub-stub の内側が返した）。
    Injected(InjectedRefusal),
}

impl PartialEq<InjectedRefusal> for HubStubCallError {
    fn eq(&self, other: &InjectedRefusal) -> bool {
        matches!(self, Self::Injected(r) if r == other)
    }
}

/// 口ごとの有効な注入（遅れと拒否を各 1 件まで）。
#[derive(Debug, Clone, Copy, Default)]
struct PortFaults {
    delay: Option<FaultKind>,
    refusal: Option<FaultKind>,
}

fn slot(port: HubStubPort) -> usize {
    match port {
        HubStubPort::Presentation => 0,
        HubStubPort::Logout => 1,
        HubStubPort::Heartbeat => 2,
        HubStubPort::Jwks => 3,
    }
}

/// 有効な注入から 1 つの規則を作る。何も無い口は `None`。
fn rule<R>(pf: &PortFaults, conv: impl Fn(HubStubRefusalKind) -> R) -> Option<FaultRule<R>> {
    let delay = match pf.delay {
        Some(FaultKind::HubStubDelay { delay_ms, .. }) => Some(Duration::from_millis(delay_ms)),
        _ => None,
    };
    let refusal = match pf.refusal {
        Some(FaultKind::HubStubRefusal { refusal, .. }) => Some(conv(refusal)),
        _ => None,
    };
    if delay.is_none() && refusal.is_none() {
        return None;
    }
    Some(FaultRule {
        calls: CallWindow {
            first: 1,
            last: None,
        },
        delay,
        refusal,
    })
}

/// 提示・失効の通知・生存確認の口の変換。`MalformedBody` は入口で弾くので来ないが、来ても成功にせず接続の失敗へ倒す。
fn plain(r: HubStubRefusalKind) -> Refusal {
    match r {
        HubStubRefusalKind::Status(s) => Refusal::Status(s),
        HubStubRefusalKind::Unreachable | HubStubRefusalKind::MalformedBody => Refusal::Unreachable,
    }
}

fn jwks(r: HubStubRefusalKind) -> JwksRefusal {
    match r {
        HubStubRefusalKind::Unreachable => JwksRefusal::Unreachable,
        HubStubRefusalKind::Status(s) => JwksRefusal::Status(s),
        HubStubRefusalKind::MalformedBody => JwksRefusal::MalformedBody,
    }
}

fn build_config(active: &[PortFaults; 4]) -> HubFaultConfig {
    HubFaultConfig {
        presentation: rule(&active[0], plain).into_iter().collect(),
        logout: rule(&active[1], plain).into_iter().collect(),
        heartbeat: rule(&active[2], plain).into_iter().collect(),
        jwks: rule(&active[3], jwks).into_iter().collect(),
    }
}

/// 注入の入口。内側は hub-stub の注入器。
pub struct HubStubFaultInjector<W: Wait> {
    run_id: u64,
    inner: HubFaultInjector<W>,
    active: [PortFaults; 4],
    /// 実行側の結合の印。未使用（一度も `apply` が通っていない）まま破棄されたら下ろす。
    bound: Arc<AtomicBool>,
    /// `apply` が一度でも通ったか。
    used: bool,
}

impl<W: Wait> Drop for HubStubFaultInjector<W> {
    /// 未使用の注入器の破棄は結合を取り消す（結び直せる）。使った注入器は印を残す
    /// （事象が記録に積まれており、注入器を介さない確定や再結合で状態が食い違わないように）。
    fn drop(&mut self) {
        if !self.used {
            self.bound.store(false, Ordering::SeqCst);
        }
    }
}

impl<W: Wait> fmt::Debug for HubStubFaultInjector<W> {
    /// 件数だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubStubFaultInjector")
            .field("active", &self.active_count())
            .field("calls", &self.inner.log().records().len())
            .finish()
    }
}

impl<W: Wait> HubStubFaultInjector<W> {
    /// 注入なし（基準線の形）で始め、`timeline` の実行に結ぶ。`wait` は遅れの渡し先。
    /// 注入器は 1 つの実行だけに使う。別の実行（基準線を含む）には新しい注入器を作る。
    /// 有効な規則と呼び出しの記録を、別の実行へ持ち越せない構造にするため（fail-closed）。
    /// 1 つの実行に結べる注入器は 1 つだけ。事象の呼び出し位置（`log_pos`）は注入器ごとの呼び出しの記録の件数なので、
    /// 複数あると順序の照合が崩れる。2 つ目は `AlreadyBound`。
    /// 一度も `apply` が通らないまま注入器を破棄した場合は結合を取り消し、結び直すか直接の確定ができる。
    ///
    /// `wait` は遅れの渡し先で、`HubStubHandle::with` のクロージャ内で受け口を呼ぶとセッションのロックを握ったまま走る。
    /// 同じハンドルを参照する（再入する）ものを渡さない。
    pub fn new(wait: W, timeline: &FaultTimeline) -> Result<Self, HubStubFaultError> {
        let Some(bound) = timeline
            .bind_injector()
            .map_err(HubStubFaultError::Record)?
        else {
            return Err(HubStubFaultError::AlreadyBound);
        };
        Ok(Self {
            run_id: timeline.run_id(),
            inner: HubFaultInjector::new(HubFaultConfig::baseline(), wait),
            active: [PortFaults::default(); 4],
            bound,
            used: false,
        })
    }

    /// 検査して口の番号と系統（遅れなら `true`）を返す。
    fn classify(kind: FaultKind) -> Result<(usize, bool), HubStubFaultError> {
        if kind.target() != FaultTarget::HubStub || kind.validate().is_err() {
            return Err(HubStubFaultError::InvalidParam);
        }
        match kind {
            FaultKind::HubStubDelay { port, .. } => Ok((slot(port), true)),
            FaultKind::HubStubRefusal { port, .. } => Ok((slot(port), false)),
            _ => Err(HubStubFaultError::InvalidParam),
        }
    }

    fn cell(&mut self, i: usize, is_delay: bool) -> &mut Option<FaultKind> {
        if is_delay {
            &mut self.active[i].delay
        } else {
            &mut self.active[i].refusal
        }
    }

    /// 注入を始める。記録（`Applied`）が通ってから設定を差し替える。
    pub fn apply(
        &mut self,
        timeline: &mut FaultTimeline,
        kind: FaultKind,
    ) -> Result<(), HubStubFaultError> {
        self.check_run(timeline)?;
        let (i, is_delay) = Self::classify(kind)?;
        if self.cell(i, is_delay).is_some() {
            return Err(HubStubFaultError::AlreadyActive);
        }
        timeline
            .record_now_with_log_pos(
                kind,
                FaultPhase::Applied,
                Some(self.inner.log().records().len()),
            )
            .map_err(HubStubFaultError::Record)?;
        self.used = true;
        *self.cell(i, is_delay) = Some(kind);
        self.inner.set_faults(build_config(&self.active));
        Ok(())
    }

    /// 開始と同じ種類・値の注入を解除する。記録（`Released`）が通ってから設定を差し替える。
    pub fn release(
        &mut self,
        timeline: &mut FaultTimeline,
        kind: FaultKind,
    ) -> Result<(), HubStubFaultError> {
        self.check_run(timeline)?;
        let (i, is_delay) = Self::classify(kind)?;
        if *self.cell(i, is_delay) != Some(kind) {
            return Err(HubStubFaultError::NotActive);
        }
        timeline
            .record_now_with_log_pos(
                kind,
                FaultPhase::Released,
                Some(self.inner.log().records().len()),
            )
            .map_err(HubStubFaultError::Record)?;
        *self.cell(i, is_delay) = None;
        self.inner.set_faults(build_config(&self.active));
        Ok(())
    }

    fn guard(&self, timeline: &FaultTimeline) -> Result<(), HubStubCallError> {
        self.check_run(timeline)
            .map_err(|_| HubStubCallError::RunMismatch)
    }

    fn check_run(&self, timeline: &FaultTimeline) -> Result<(), HubStubFaultError> {
        if timeline.run_id() == self.run_id {
            Ok(())
        } else {
            Err(HubStubFaultError::RunMismatch)
        }
    }

    /// 実行を終える。注入器を消費するので、解除しなかった規則は次の実行へ残らない
    /// （解除しなかった分は記録の `unreleased_count` に出る）。別の実行の記録は `RunMismatch`。
    /// 記録と呼び出しの記録を組で返す。事象の `log_pos` と呼び出しの順序で、終了後にも照合できる。
    ///
    /// # Errors
    /// 失敗のときは注入器とタイムラインを [`HubStubFinishFailure`] に入れて返す（所有権を失わず、正しい組で再試行できる）。
    pub fn finish(
        self,
        timeline: FaultTimeline,
        recorded_at: UtcTimestamp,
    ) -> Result<(FaultRecord, InjectionLog), Box<HubStubFinishFailure<W>>> {
        let checked = self.check_run(&timeline).and_then(|()| {
            timeline
                .check_finishable()
                .map_err(HubStubFaultError::Record)
        });
        if let Err(error) = checked {
            return Err(Box::new(HubStubFinishFailure {
                error,
                injector: self,
                timeline,
            }));
        }
        let log = self.inner.log().clone();
        Ok((timeline.into_record(recorded_at), log))
    }

    /// 有効な注入の数。
    pub fn active_count(&self) -> usize {
        self.active
            .iter()
            .map(|p| usize::from(p.delay.is_some()) + usize::from(p.refusal.is_some()))
            .sum()
    }

    /// hub-stub の呼び出しの記録（読むだけ。`at` は呼び出し元が渡した試験の論理時刻）。
    pub fn call_log(&self) -> &InjectionLog {
        self.inner.log()
    }

    /// 本人トークンの提示（hub-stub の同名へ委ねる）。
    pub fn present(
        &mut self,
        timeline: &FaultTimeline,
        inner: &mut IdentityTokenReceiver,
        token: &str,
        now: u64,
    ) -> Result<Result<ReceivedIdentity, PresentationRejection>, HubStubCallError> {
        self.guard(timeline)?;
        self.inner
            .present(inner, token, now)
            .map_err(HubStubCallError::Injected)
    }

    /// 失効の通知（同上）。
    pub fn receive_logout(
        &mut self,
        timeline: &FaultTimeline,
        inner: &mut LogoutTokenReceiver,
        token: &str,
        now: u64,
    ) -> Result<Result<LogoutReceipt, LogoutRejection>, HubStubCallError> {
        self.guard(timeline)?;
        self.inner
            .receive_logout(inner, token, now)
            .map_err(HubStubCallError::Injected)
    }

    /// 生存確認（同上）。
    pub fn receive_heartbeat(
        &mut self,
        timeline: &FaultTimeline,
        inner: &mut HeartbeatReceiver,
        heartbeat: &Heartbeat,
        now: u64,
    ) -> Result<(), HubStubCallError> {
        self.guard(timeline)?;
        self.inner
            .receive_heartbeat(inner, heartbeat, now)
            .map_err(HubStubCallError::Injected)
    }

    /// Hub の JWKS の配信（同上）。
    pub fn serve_jwks(
        &mut self,
        timeline: &FaultTimeline,
        inner: &mut HubJwksEndpoint,
        now: u64,
    ) -> Result<JwksResponse, HubStubCallError> {
        self.guard(timeline)?;
        self.inner
            .serve_jwks(inner, now)
            .map_err(HubStubCallError::Injected)
    }
}
