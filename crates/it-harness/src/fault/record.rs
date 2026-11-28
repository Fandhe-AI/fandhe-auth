//! 障害注入の記録の形式（TASK-13 の 1/8）。注入した実行と基準線（故障なし）の実行で同じキーを出す。
//!
//! 役割: 「いつ・どの種類を・どのパラメータで注入したか」を読み手が確かめられるように残す。呼び出し元は
//! 結合試験（試験ビルド）だけ。記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。
//!
//! 設計上の約束:
//! - 時刻はハーネスのプロセスの基準の時計で押す。試験対象に注入した、ずれた時計や stub の引数の `now` は使わない
//!   （時計のずれを注入しても記録の時刻はずれない。AVLB-31）。壁時計 `at`（秒）と、実行開始からの単調時計の
//!   経過 `offset_ms` を持つ。hub-stub の `InjectionLog.at`（試験の論理時刻）は
//!   ここへ入れず、`HubStubFaultInjector::call_log` で別に読む（対応は順序だけ。事象行の `log_pos` は形式の版 `/2` で足した）。
//! - 基準線でも同じキーを出す。節を省くと「記録しなかった」と見分けられないため（TASK-10 の `not_started` と同じ）。
//! - 基準線に事象を積むのも、Fault の実行で 0 件のまま記録を作るのも誤り（fail-closed）。
//! - 記録は enum・数値・静的な名前だけで組む。ホスト名・ポート・パス・接続情報・実行時の文字列を持たない
//!   （SECR-11・SECR-23・SECR-26）。環境変数・設定ファイルは読まない（DPOL-32）。
//! - 形式の識別子・キー名・文言は仮置きで、人が確認する。`RunRecord` とは `run_started_at` で結ぶ（結ぶ形も仮置き）。

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

use super::{FaultKind, FaultPhase, FaultTarget};
use crate::record::UtcTimestamp;

/// 記録の形式の識別子（仮置き）。形式を変えたら末尾の版を上げる。/2 で事象行に任意の `log_pos=<n>` を足した（TASK-13 の 5/8）。
pub const FAULT_RECORD_FORMAT: &str = "fandhe-auth-it-fault-record/2";

/// 実行の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// 故障なしの基準線（F0）。
    Baseline,
    /// 注入ありの実行。
    Fault,
}

/// 誤りの種類。パス・`io::Error` の文言・パラメータの値を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultRecordError {
    /// 時刻が epoch より前。
    ClockBeforeEpoch,
    /// パラメータの検査に落ちた（0 の遅れ・0 のずれ等）。
    InvalidParam,
    /// 基準線の実行に事象を積もうとした。
    EventsInBaseline,
    /// Fault の実行で事象が 0 件のまま記録を作ろうとした。
    NoInjectionInFaultRun,
    /// 対応する `Applied` が無い `Released`。
    ReleaseWithoutApply,
    /// 解除の無い一回きりの事象（`SIGTERM`・`SIGKILL`）に `Released` を積もうとした。
    ReleaseOfInstantaneous,
    /// `offset_ms` が前の件より小さい。
    OffsetWentBackwards,
    /// 呼び出しの位置（`log_pos`）を Hub の stub 以外の事象に付けようとした。
    LogPosNotAllowed,
    /// 呼び出しの位置（`log_pos`）が前の件より小さい。
    LogPosWentBackwards,
    /// 公開の直接記録（`record_now`・`record_at`）で Hub の stub の事象を積もうとした（常に拒否。注入器の入口だけが積む）。
    HubStubEventOutsideInjector,
    /// Hub の stub の事象に呼び出しの位置（`log_pos`）が無い。
    LogPosRequired,
    /// 注入器を結んだ実行の記録を、注入器を介さず確定しようとした。
    InjectorBound,
    /// 時計のずれの注入器を結んだ実行の記録を、時計の注入器を介さず確定しようとした（共有のずれが残るため）。
    ClockInjectorBound,
    /// 注入器を結んだ実行と別の実行の記録を渡された（記録は消費しない）。
    RunMismatch,
    /// 時計のずれの注入器を結んだ実行へ公開の直接記録で時計の事象を積もうとした、または直接記録した時計の事象がある実行へ
    /// 注入器を結ぼうとした（時計の事象は注入器の入口だけが積む）。
    ClockSkewEventOutsideInjector,
    /// ファイルの書き込みに失敗した。
    WriteFailed,
}

impl fmt::Display for FaultRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::ClockBeforeEpoch => "clock is before the epoch",
            Self::InvalidParam => "invalid parameter",
            Self::EventsInBaseline => "events in baseline run",
            Self::NoInjectionInFaultRun => "no injection in fault run",
            Self::ReleaseWithoutApply => "release without apply",
            Self::ReleaseOfInstantaneous => "release of instantaneous fault",
            Self::OffsetWentBackwards => "offset went backwards",
            Self::LogPosNotAllowed => "log position not allowed for this target",
            Self::LogPosWentBackwards => "log position went backwards",
            Self::HubStubEventOutsideInjector => "hub stub event must go through the injector",
            Self::LogPosRequired => "hub stub event requires a log position",
            Self::InjectorBound => "timeline is bound to an injector; finish through it",
            Self::ClockInjectorBound => "timeline is bound to a clock injector; finish through it",
            Self::RunMismatch => "run mismatch",
            Self::ClockSkewEventOutsideInjector => "clock skew event must go through the injector",
            Self::WriteFailed => "write failed",
        };
        write!(f, "fault record error: {s}")
    }
}

impl std::error::Error for FaultRecordError {}

/// 記録の 1 件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultEvent {
    pub kind: FaultKind,
    pub phase: FaultPhase,
    /// ハーネスの壁時計（秒）。
    pub at: UtcTimestamp,
    /// 実行の開始からの単調時計の経過（ミリ秒）。
    pub offset_ms: u64,
    /// 事象を積んだ時点の、呼び出しの記録（hub-stub の `InjectionLog`）の件数。`None` は呼び出しの記録を持たない種類。
    /// 呼び出し `i`（0 始まり）は、`log_pos <= i` の事象より後・次の事象より前に起きた。
    /// 同じ口へ同じ障害を二度適用・解除しても、どの期間の呼び出しかを照合できる（TASK-13 の 5/8）。
    pub log_pos: Option<usize>,
}

/// 注入の段 1 つ（種類と相）。基準線と注入ありで同じ並びを持つかの照合に使う（TASK-13 の 7/8）。
/// 時刻・位置を持たない点なので、基準線でも積める（事象ではない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultStep {
    pub kind: FaultKind,
    pub phase: FaultPhase,
}

/// [`FaultTimeline::inject`] の誤り。値・パスを載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectError<E> {
    /// 注入の操作自体の誤り（記録が済んでいれば記録は残る）。
    Op(E),
    /// 段の宣言の検査（パラメータ・解除の対応）に落ちた。何も積んでいない。
    Record(FaultRecordError),
    /// 操作が積んだ事象が宣言した段と一致しない（fail-closed。積まれた事象は記録に残る）。
    Mismatch,
}

/// 事象を積む入口。`Instant` の原点と開始時刻を持つ。
///
/// `Clone` は実装しない。複製すると事象列と開いている注入の記録が分かれ、複製側の解除が元に反映されないため
/// （1 つの実行の記録は 1 つに集約する）。
#[derive(Debug)]
pub struct FaultTimeline {
    run_id: u64,
    injector_bound: Arc<AtomicBool>,
    /// 時計のずれの注入器を結んだか（1 つの実行に時計の注入器は 1 つ。取り消さない）。
    clock_bound: bool,
    mode: RunMode,
    started_at: UtcTimestamp,
    origin: Instant,
    events: Vec<FaultEvent>,
    open: Vec<FaultKind>,
    /// 注入の段の並び。注入ありは操作が積んだ事象と照合した段、基準線は宣言だけの段（事象は積まない）。
    points: Vec<FaultStep>,
    /// 基準線で宣言した段の、解除の対応を確かめるための開いている一覧（`open` とは別。`unreleased` に数えない）。
    shadow_open: Vec<FaultKind>,
}

/// Hub の stub の事象は公開の直接記録で積ませない（注入器を通さない確定を作らない。TASK-13 の 5/8）。
fn reject_hub_stub(kind: FaultKind) -> Result<(), FaultRecordError> {
    if kind.target() == FaultTarget::HubStub {
        return Err(FaultRecordError::HubStubEventOutsideInjector);
    }
    Ok(())
}

impl FaultTimeline {
    /// 注入ありの実行を始める。`origin` は単調時計の原点、`started` は壁時計の開始時刻。
    pub fn start(origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        Self::new(RunMode::Fault, origin, started)
    }

    /// 故障なしの基準線の実行を始める。事象は積めない。
    pub fn baseline(origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        Self::new(RunMode::Baseline, origin, started)
    }

    fn new(mode: RunMode, origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        let started_at = UtcTimestamp::from_system_time(started)
            .map_err(|_| FaultRecordError::ClockBeforeEpoch)?;
        static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
        Ok(Self {
            run_id: RUN_SEQ.fetch_add(1, Ordering::Relaxed),
            injector_bound: Arc::new(AtomicBool::new(false)),
            clock_bound: false,
            mode,
            started_at,
            origin,
            events: Vec::new(),
            open: Vec::new(),
            points: Vec::new(),
            shadow_open: Vec::new(),
        })
    }

    /// 注入の段を、実行の種別で分岐せずに通す（TASK-13 の 7/8）。同じ本体を基準線と注入ありで流せるようにする。
    ///
    /// - 注入あり: `op` を呼び、積まれた事象が `steps` と同じ数・同じ順・同じ (種類, 相) かを確かめて、段を残す。
    ///   `op` が記録の後で失敗しても（記録は事実なので）段に数え、`Op` で返す。
    /// - 基準線: `op` を呼ばない。`steps` へ注入ありの記録と同じ検査（パラメータ・一回きりの事象の解除・対応する
    ///   `Applied` の無い解除）を掛け、通れば段だけを残して `Ok(None)` を返す。事象は積まない（`EventsInBaseline` は変えない）。
    ///
    /// # Errors
    /// 空の `steps` と検査落ちは `Record`（何も積まない）、宣言と積まれた事象の食い違いは `Mismatch`、`op` の失敗は `Op`。
    pub fn inject<T, E>(
        &mut self,
        steps: &[FaultStep],
        op: impl FnOnce(&mut FaultTimeline) -> Result<T, E>,
    ) -> Result<Option<T>, InjectError<E>> {
        if steps.is_empty() {
            return Err(InjectError::Record(FaultRecordError::InvalidParam));
        }
        if self.mode == RunMode::Baseline {
            let mut shadow = self.shadow_open.clone();
            for st in steps {
                st.kind
                    .validate()
                    .map_err(|_| InjectError::Record(FaultRecordError::InvalidParam))?;
                match st.phase {
                    FaultPhase::Applied => {
                        if !st.kind.is_instantaneous() {
                            shadow.push(st.kind);
                        }
                    }
                    FaultPhase::Released => {
                        if st.kind.is_instantaneous() {
                            return Err(InjectError::Record(
                                FaultRecordError::ReleaseOfInstantaneous,
                            ));
                        }
                        let i = shadow
                            .iter()
                            .position(|k| *k == st.kind)
                            .ok_or(InjectError::Record(FaultRecordError::ReleaseWithoutApply))?;
                        shadow.remove(i);
                    }
                }
            }
            self.shadow_open = shadow;
            self.points.extend_from_slice(steps);
            return Ok(None);
        }
        let before = self.events.len();
        let result = op(self);
        let matched = self.events.len() - before == steps.len()
            && self.events[before..]
                .iter()
                .zip(steps)
                .all(|(e, st)| e.kind == st.kind && e.phase == st.phase);
        if !matched {
            // 1 件も積まずに失敗した操作は、宣言と食い違うのではなく操作の失敗として返す。
            if self.events.len() == before
                && let Err(e) = result
            {
                return Err(InjectError::Op(e));
            }
            return Err(InjectError::Mismatch);
        }
        self.points.extend_from_slice(steps);
        match result {
            Ok(t) => Ok(Some(t)),
            Err(e) => Err(InjectError::Op(e)),
        }
    }

    /// この実行の識別（プロセス内で一意）。注入器を単一の実行へ結ぶために使う。
    pub(crate) fn run_id(&self) -> u64 {
        self.run_id
    }

    /// 注入器をこの実行へ結ぶ。すでに結んでいれば `None`（1 つの実行に注入器は 1 つ。呼び出しの位置の照合のため）。
    /// 結べたら結合の印を返す。注入器が未使用のまま破棄されたとき、その印を下ろして結び直せるようにする。
    /// Hub の stub の事象は公開の入口では積めない（`push` が `log_pos` を必須にする）ので、結ぶ前の事象に Hub の stub のものは無い。
    pub(crate) fn bind_injector(&self) -> Result<Option<Arc<AtomicBool>>, FaultRecordError> {
        if self.injector_bound.swap(true, Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(Some(Arc::clone(&self.injector_bound)))
    }

    /// 時計のずれの注入器をこの実行へ結べるかを確かめる（状態は変えない）。結べるなら `Ok(true)`、すでに別の注入器が
    /// 結んでいれば `Ok(false)`。公開の直接記録で時計の事象を積んだ実行へは結べない（`ClockSkewEventOutsideInjector`。
    /// 解除の対応が食い違うため）。
    pub(crate) fn check_clock_bindable(&self) -> Result<bool, FaultRecordError> {
        if self.clock_bound {
            return Ok(false);
        }
        if self
            .events
            .iter()
            .any(|e| matches!(e.kind, FaultKind::ClockSkew { .. }))
        {
            return Err(FaultRecordError::ClockSkewEventOutsideInjector);
        }
        Ok(true)
    }

    /// 時計のずれの注入器の結合を確定する。初回の `Applied` の記録が成功した後にだけ呼ぶ（記録の拒否で結合が残らないように）。
    /// 結合は取り消さない（注入器を破棄しても、実行の時計の事象は残るため）。
    pub(crate) fn commit_clock_bound(&mut self) {
        self.clock_bound = true;
    }

    /// 時計の注入器を結んでいるか。
    pub(crate) fn is_clock_bound(&self) -> bool {
        self.clock_bound
    }

    /// Hub の stub の注入器を結んでいるか。
    pub(crate) fn is_injector_bound(&self) -> bool {
        self.injector_bound.load(Ordering::SeqCst)
    }

    /// ハーネスの時計で現在の時刻を押して積む。
    ///
    /// # Errors
    /// 時計の注入器を結んだ実行の時計の事象は `ClockSkewEventOutsideInjector`。
    /// Hub の stub の事象は常に `HubStubEventOutsideInjector`（注入器の内部の入口だけが `log_pos` 付きで積む）。
    pub fn record_now(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
    ) -> Result<(), FaultRecordError> {
        reject_hub_stub(kind)?;
        self.reject_clock_skew_if_bound(kind)?;
        self.record_now_with_log_pos(kind, phase, None)
    }

    /// `record_now` に、呼び出しの記録の件数（`FaultEvent::log_pos`）を添える。注入器の内部だけの入口。
    /// Hub の stub の事象だけが受け、位置は前の位置以上でなければならない。
    pub(crate) fn record_now_with_log_pos(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        log_pos: Option<usize>,
    ) -> Result<(), FaultRecordError> {
        let at = UtcTimestamp::from_system_time(SystemTime::now())
            .map_err(|_| FaultRecordError::ClockBeforeEpoch)?;
        let ms = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.push(kind, phase, at, ms, log_pos)
    }

    /// 時刻を引数で渡して積む（試験用の決定的な入口）。
    ///
    /// # Errors
    /// Hub の stub の事象（常に `HubStubEventOutsideInjector`）・基準線への積み込み・パラメータの検査落ち・`offset_ms` の逆行・対応する `Applied` の無い `Released`・
    /// 一回きりの事象への `Released`。誤りのときは何も積まない。
    pub fn record_at(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        at: UtcTimestamp,
        offset_ms: u64,
    ) -> Result<(), FaultRecordError> {
        reject_hub_stub(kind)?;
        self.reject_clock_skew_if_bound(kind)?;
        self.push(kind, phase, at, offset_ms, None)
    }

    /// 時計の注入器を結んだ実行では、公開の直接記録で時計の事象を積ませない（注入器の状態と記録を食い違わせない）。
    fn reject_clock_skew_if_bound(&self, kind: FaultKind) -> Result<(), FaultRecordError> {
        if self.clock_bound && matches!(kind, FaultKind::ClockSkew { .. }) {
            return Err(FaultRecordError::ClockSkewEventOutsideInjector);
        }
        Ok(())
    }

    fn push(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        at: UtcTimestamp,
        offset_ms: u64,
        log_pos: Option<usize>,
    ) -> Result<(), FaultRecordError> {
        if self.mode == RunMode::Baseline {
            return Err(FaultRecordError::EventsInBaseline);
        }
        kind.validate()
            .map_err(|_| FaultRecordError::InvalidParam)?;
        if self.events.last().is_some_and(|e| offset_ms < e.offset_ms) {
            return Err(FaultRecordError::OffsetWentBackwards);
        }
        if kind.target() == FaultTarget::HubStub && log_pos.is_none() {
            return Err(FaultRecordError::LogPosRequired);
        }
        if let Some(pos) = log_pos {
            if kind.target() != FaultTarget::HubStub {
                return Err(FaultRecordError::LogPosNotAllowed);
            }
            if self
                .events
                .iter()
                .rev()
                .find_map(|e| e.log_pos)
                .is_some_and(|prev| pos < prev)
            {
                return Err(FaultRecordError::LogPosWentBackwards);
            }
        }
        match phase {
            FaultPhase::Applied => {
                if !kind.is_instantaneous() {
                    self.open.push(kind);
                }
            }
            FaultPhase::Released => {
                if kind.is_instantaneous() {
                    return Err(FaultRecordError::ReleaseOfInstantaneous);
                }
                let i = self
                    .open
                    .iter()
                    .position(|k| *k == kind)
                    .ok_or(FaultRecordError::ReleaseWithoutApply)?;
                self.open.remove(i);
            }
        }
        self.events.push(FaultEvent {
            kind,
            phase,
            at,
            offset_ms,
            log_pos,
        });
        Ok(())
    }

    /// 記録を作る。Fault の実行で 0 件なら誤り（何も注入していない故障の試験を作らない）。
    /// 解除しなかった `Applied` は `unreleased_count` に数える（黙って省かない）。
    ///
    /// 注入器を結んだ実行では、注入器を `HubStubFaultInjector::finish` まで保持する。受け口を一度でも呼んだ（`apply` を通さなくても）
    /// 注入器を先に破棄すると結合の印が残り、この `finish` も新しい注入器の結合も拒否されて、基準線や DB の障害だけの記録も
    /// 確定できなくなる（呼び出しの記録と `log_pos` の照合を崩さないため）。未使用のまま破棄したときだけ結合が取り消される。
    ///
    /// # Errors
    /// 注入器を結んだ実行は `InjectorBound`（注入器が有効なまま残らないよう、`HubStubFaultInjector::finish` が確定する）。
    /// 時計の注入器を結んだ実行は `ClockInjectorBound`（`ClockSkewInjector::finish`、両方なら `HubStubFaultInjector::finish_with_clock`）。
    /// 失敗のときは `FaultFinishFailure` にタイムラインを入れて返す（蓄積した事象を失わず、正しい経路で再試行できる）。
    pub fn finish(self, recorded_at: UtcTimestamp) -> Result<FaultRecord, Box<FaultFinishFailure>> {
        let checked = if self.injector_bound.load(Ordering::SeqCst) {
            Err(FaultRecordError::InjectorBound)
        } else if self.clock_bound {
            // 時計の注入器を結んだ実行は、共有のずれを戻す `ClockSkewInjector::finish` から確定する（AVLB-31）。
            Err(FaultRecordError::ClockInjectorBound)
        } else {
            self.check_finishable()
        };
        match checked {
            Ok(()) => Ok(self.into_record(recorded_at)),
            Err(error) => Err(Box::new(FaultFinishFailure {
                error,
                timeline: self,
            })),
        }
    }

    /// 確定できるか。消費する前に確かめ、失敗しても記録を失わないようにする（注入器の内部だけ）。
    pub(crate) fn check_finishable(&self) -> Result<(), FaultRecordError> {
        if self.mode == RunMode::Fault && self.events.is_empty() {
            return Err(FaultRecordError::NoInjectionInFaultRun);
        }
        Ok(())
    }

    /// 検査（`check_finishable`）を通した後の確定。
    pub(crate) fn into_record(self, recorded_at: UtcTimestamp) -> FaultRecord {
        FaultRecord {
            run_id: self.run_id,
            points: self.points,
            mode: self.mode,
            started_at: self.started_at,
            recorded_at,
            unreleased: self.open.len(),
            events: self.events,
        }
    }
}

/// `FaultTimeline::finish` の失敗。タイムラインを返すので、記録を失わずに再試行できる。
pub struct FaultFinishFailure {
    /// 失敗の理由。
    pub error: FaultRecordError,
    /// 渡されたタイムライン。
    pub timeline: FaultTimeline,
}

impl fmt::Debug for FaultFinishFailure {
    /// 理由だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultFinishFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for FaultFinishFailure {
    /// 理由の文言だけを出す。タイムラインの中身は出さない（SECR-23〜27）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl std::error::Error for FaultFinishFailure {
    /// `Box<dyn Error>` へ載せて `?` で返せるようにする（TASK-13 の 8/8）。理由は `source` で辿れる。
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// 障害注入の記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultRecord {
    run_id: u64,
    points: Vec<FaultStep>,
    mode: RunMode,
    started_at: UtcTimestamp,
    recorded_at: UtcTimestamp,
    unreleased: usize,
    events: Vec<FaultEvent>,
}

impl FaultRecord {
    /// 実行の種別。
    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// 積んだ事象（積んだ順）。
    pub fn events(&self) -> &[FaultEvent] {
        &self.events
    }

    /// 注入の段の並び（[`FaultTimeline::inject`] で通した分）。基準線と注入ありで同じ並びかを照合する。
    /// 記録の形式（`Display`）には出さない。
    pub fn injection_points(&self) -> &[FaultStep] {
        &self.points
    }

    /// 元の実行の識別（プロセス内で一意）。組の照合に使う。
    pub(crate) fn run_id(&self) -> u64 {
        self.run_id
    }

    /// 解除しなかった注入の件数。
    pub fn unreleased_count(&self) -> usize {
        self.unreleased
    }

    /// 新規のファイルへ書き出す。`RunRecord::write_new` と同じ約束（無ければ `dir` を作る・上書きしない・
    /// 乱数を使わないファイル名）。名前に実行の種別（`baseline`・`fault`）を入れ、基準線と注入ありの記録を分ける（7/8）。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, FaultRecordError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir).map_err(|_| FaultRecordError::WriteFailed)?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let mode = match self.mode {
            RunMode::Baseline => "baseline",
            RunMode::Fault => "fault",
        };
        let name = format!(
            "it-fault-{mode}-{}-{}-{n}.txt",
            self.started_at.unix_seconds(),
            std::process::id()
        );
        let path = dir.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| FaultRecordError::WriteFailed)?;
        file.write_all(self.to_string().as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| FaultRecordError::WriteFailed)?;
        Ok(path)
    }
}

impl fmt::Display for FaultRecord {
    /// 1 行 1 項目の `key: value`（UTF-8・LF）。キーは ASCII 固定。基準線でも同じキーを出す。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            RunMode::Baseline => "baseline",
            RunMode::Fault => "fault",
        };
        writeln!(f, "format: {FAULT_RECORD_FORMAT}")?;
        writeln!(f, "run_mode: {mode}")?;
        writeln!(f, "run_started_at: {}", self.started_at)?;
        writeln!(f, "recorded_at: {}", self.recorded_at)?;
        writeln!(f, "injection_count: {}", self.events.len())?;
        writeln!(f, "unreleased_count: {}", self.unreleased)?;
        for (i, e) in self.events.iter().enumerate() {
            let phase = match e.phase {
                FaultPhase::Applied => "applied",
                FaultPhase::Released => "released",
            };
            write!(
                f,
                "injection: {} {phase} {} target={} at={} offset_ms={}{}",
                i + 1,
                e.kind.category().name(),
                e.kind.target().name(),
                e.at,
                e.offset_ms,
                e.kind
            )?;
            if let Some(n) = e.log_pos {
                write!(f, " log_pos={n}")?;
            }
            writeln!(f)?;
        }
        writeln!(
            f,
            "note: 記録は事実の出力で、合否の判定でも spec の確定でもない（ACPT-7）"
        )?;
        if self
            .events
            .iter()
            .any(|e| e.kind.target() == FaultTarget::HubStub)
        {
            writeln!(
                f,
                "note: stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）"
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault::HubStubPort;
    use std::time::Duration;

    fn tl() -> FaultTimeline {
        FaultTimeline::start(
            Instant::now(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        )
        .unwrap()
    }

    fn stub_delay() -> FaultKind {
        FaultKind::hub_stub_delay(HubStubPort::Jwks, Duration::from_millis(5)).unwrap()
    }

    /// 確定の失敗は `Box<dyn Error>` へ変換でき、文言は理由と一致し、`source` が理由を返す（TASK-13 の 8/8）。
    #[test]
    fn finish_failure_is_an_error() {
        let failure = tl().finish(UtcTimestamp::from_unix_seconds(2)).unwrap_err();
        let expected = failure.error.to_string();
        let boxed: Box<dyn std::error::Error> = failure;
        assert_eq!(boxed.to_string(), expected);
        assert!(boxed.source().is_some());
        assert_eq!(
            format!("{boxed:?}"),
            "FaultFinishFailure { error: NoInjectionInFaultRun, .. }"
        );
    }

    /// 位置は Hub の stub の事象だけに付く。
    #[test]
    fn log_pos_is_only_for_hub_stub_events() {
        let mut t = tl();
        let db = FaultKind::db_delay(Duration::from_millis(5)).unwrap();
        assert_eq!(
            t.record_now_with_log_pos(db, FaultPhase::Applied, Some(0)),
            Err(FaultRecordError::LogPosNotAllowed)
        );
        assert!(t.events.is_empty());
    }

    /// 位置の逆行は拒否し、何も積まない。同じ位置は受ける。
    #[test]
    fn log_pos_must_not_go_backwards() {
        let mut t = tl();
        let k = stub_delay();
        t.record_now_with_log_pos(k, FaultPhase::Applied, Some(3))
            .unwrap();
        assert_eq!(
            t.record_now_with_log_pos(k, FaultPhase::Released, Some(2)),
            Err(FaultRecordError::LogPosWentBackwards)
        );
        assert_eq!(t.events.len(), 1);
        t.record_now_with_log_pos(k, FaultPhase::Released, Some(3))
            .unwrap();
    }

    /// Hub の stub の事象は公開の直接記録で常に拒否し、何も積まない（結合の有無によらない）。
    #[test]
    fn public_entries_always_reject_hub_stub_events() {
        let mut t = tl();
        let k = stub_delay();
        assert_eq!(
            t.record_now(k, FaultPhase::Applied),
            Err(FaultRecordError::HubStubEventOutsideInjector)
        );
        assert_eq!(
            t.record_at(
                k,
                FaultPhase::Applied,
                UtcTimestamp::from_unix_seconds(1),
                0
            ),
            Err(FaultRecordError::HubStubEventOutsideInjector)
        );
        assert!(t.events.is_empty());
        assert_eq!(
            t.check_finishable(),
            Err(FaultRecordError::NoInjectionInFaultRun)
        );
    }

    /// 内部の入口でも Hub の stub の事象は `log_pos` を必須にする。
    #[test]
    fn hub_stub_event_requires_log_pos() {
        let mut t = tl();
        assert_eq!(
            t.push(
                stub_delay(),
                FaultPhase::Applied,
                UtcTimestamp::from_unix_seconds(1),
                0,
                None
            ),
            Err(FaultRecordError::LogPosRequired)
        );
        assert!(t.events.is_empty());
    }

    /// 記録の形式: Hub の stub の事象行（`log_pos` 付き）と注記。時刻は固定の人工の値（決定的）。
    #[test]
    fn hub_stub_event_lines_format() {
        let mut t = tl();
        let at = UtcTimestamp::from_unix_seconds(1_700_000_001);
        let d = stub_delay();
        let r = FaultKind::hub_stub_refusal(
            HubStubPort::Logout,
            crate::fault::HubStubRefusalKind::Status(503),
        )
        .unwrap();
        t.push(d, FaultPhase::Applied, at, 0, Some(0)).unwrap();
        t.push(r, FaultPhase::Applied, at, 1, Some(2)).unwrap();
        let rec = t.into_record(UtcTimestamp::from_unix_seconds(1_700_000_060));
        assert_eq!(rec.unreleased_count(), 2);
        let s = rec.to_string();
        assert!(s.contains(
            "injection: 1 applied hub_stub_delay target=hub_stub at=2023-11-14T22:13:21Z offset_ms=0 port=jwks delay_ms=5 log_pos=0\n"
        ));
        assert!(s.contains(
            "injection: 2 applied hub_stub_refusal target=hub_stub at=2023-11-14T22:13:21Z offset_ms=1 port=logout refusal=status_503 log_pos=2\n"
        ));
        assert!(s.contains("note: stub の合格を実物の合格に数えない"));
    }

    /// 注入器を結べるのは 1 回だけ。
    #[test]
    fn injector_binds_once() {
        let t = tl();
        assert!(t.bind_injector().unwrap().is_some());
        assert!(t.bind_injector().unwrap().is_none());
    }
}
