//! 時計のずれの注入（TASK-13 の 6/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: 試験対象（instance）が読む壁時計の源 [`SkewedClock`] に、ミリ秒単位のずれを足す。ずれの開始・段階の変更・
//! 解除を制御側の [`ClockSkewInjector`] から行い、同時に 1/8 の [`FaultTimeline`] へ記録する。AVLB-31 の
//! 「1 つのインスタンスの時計を段階的にずらし、時刻に依存する判定（CLM-20・OCL-26・SESS-18・SESS-19）を調べる」
//! 操作の道具で、判定そのもの・合否はここに無い（ACPT-7）。時計の差を測ることと警報は Hub `AOPS-12`。
//! 呼び出し元は結合試験（試験ビルド）だけ。呼び出し先は [`super::record`]（記録）だけ。
//! Host（TASK-716）への結線はここに無く、[`SkewedClock`] を時刻の源として渡す口を置くだけにする
//! （TODO(TASK-716): Host の構築時に [`ClockSkewInjector::clock`] を時刻の源として渡す）。
//!
//! 設計上の約束:
//! - OS の時計は触らない（権限が要り機械全体に効くため。`unsafe` も使わない）。LD_PRELOAD・環境変数・feature・
//!   設定ファイルでずらす経路を作らない（DPOL-30〜32）。ずらすのは壁時計の源だけで、単調時計はずらさない。
//! - 記録の時刻はハーネスの基準の時計のまま（[`FaultTimeline::record_now`]）。`SkewedClock` を記録へ渡さない。
//! - 記録が成功してから状態を変える。基準線の `FaultTimeline` への注入は記録が拒否するので、ずれも入らない（fail-closed）。
//! - 基準の時刻にずれを足して表せない・UNIX 紀元より前になるときは [`ClockSkewError::OutOfRange`]。黙って丸めない。
//! - 誤り・`Debug` にずれの値・時刻を載せない（SECR-11・SECR-23）。ずれの上限は置かない（値は試験用の仮の値で
//!   要件の値ではない。THRT-29・A-2026-10-28-1）。
//! - 時刻の関係: `Applied` の記録の時刻 <= 読み取り側にずれが見える時刻 <= `apply` が戻る時刻。他の読み取りスレッドとの
//!   重なりは未定。
//!
//! 名前・形は仮置きで、人が確認する。

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::record::{FaultRecordError, FaultTimeline};
use super::{FaultKind, FaultPhase};

/// 時計のずれの注入の誤り。ずれの値・時刻を載せない（SECR-11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSkewError {
    /// パラメータの検査に落ちた（0 のずれ・同じ値への `step`）。
    InvalidParam,
    /// すでにずれを適用中。
    AlreadyActive,
    /// 適用中のずれが無い。
    NotActive,
    /// 適用した実行と別の実行の記録を渡された（状態も記録も変えない）。
    RunMismatch,
    /// 基準の時刻にずれを足すと表せない、または UNIX 紀元より前になる。
    OutOfRange,
    /// 記録に拒否された（基準線の実行等）。
    Record(FaultRecordError),
}

impl fmt::Display for ClockSkewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::InvalidParam => "invalid parameter",
            Self::AlreadyActive => "already active",
            Self::NotActive => "not active",
            Self::RunMismatch => "run mismatch",
            Self::OutOfRange => "skewed time out of range",
            Self::Record(_) => "record rejected",
        };
        write!(f, "clock skew fault error: {s}")
    }
}

impl std::error::Error for ClockSkewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Record(e) => Some(e),
            _ => None,
        }
    }
}

type BaseClock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// 基準の時刻にずれ（ミリ秒。負は遅らせる向き）を足す。紀元より前・表せない値は `OutOfRange`。
fn shifted(base: SystemTime, skew_ms: i64) -> Result<SystemTime, ClockSkewError> {
    let d = Duration::from_millis(skew_ms.unsigned_abs());
    let t = if skew_ms >= 0 {
        base.checked_add(d)
    } else {
        base.checked_sub(d)
    }
    .ok_or(ClockSkewError::OutOfRange)?;
    t.duration_since(UNIX_EPOCH)
        .map_err(|_| ClockSkewError::OutOfRange)?;
    Ok(t)
}

/// 試験対象が読む壁時計の源（読み取り側）。複製は同じずれを共有する。
#[derive(Clone)]
pub struct SkewedClock {
    skew_ms: Arc<AtomicI64>,
    base: BaseClock,
}

impl fmt::Debug for SkewedClock {
    /// ずれの有無だけ。値・時刻を出さない。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkewedClock")
            .field("skewed", &(self.skew_ms() != 0))
            .finish_non_exhaustive()
    }
}

impl SkewedClock {
    /// 現在のずれ（ミリ秒。0 はずれなし）。試験の確認用。
    pub fn skew_ms(&self) -> i64 {
        self.skew_ms.load(Ordering::SeqCst)
    }

    /// 基準の時刻に現在のずれを足した時刻。
    ///
    /// # Errors
    /// 表せない・紀元より前なら `OutOfRange`。
    pub fn now(&self) -> Result<SystemTime, ClockSkewError> {
        shifted((self.base)(), self.skew_ms())
    }

    /// `now` の UNIX 秒（切り捨て）。stub の `now: u64` に渡す用途。
    ///
    /// # Errors
    /// `now` と同じ。
    pub fn unix_seconds(&self) -> Result<u64, ClockSkewError> {
        let d = self
            .now()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockSkewError::OutOfRange)?;
        Ok(d.as_secs())
    }
}

/// 1 つのインスタンスの時計のずれの制御側。`Clone` しない。`Drop` では記録に触らず、配布済みの [`SkewedClock`] が
/// 共有するずれだけを 0 へ戻す（AVLB-31 の試験条件を実行単位で保つ。基準線の実行へ前の障害を持ち越さない）。
/// 解除していない注入は `Released` が記録に無いので、`FaultTimeline::finish` の `unreleased_count` に出る。
pub struct ClockSkewInjector {
    skew_ms: Arc<AtomicI64>,
    base: BaseClock,
    active: Option<FaultKind>,
    /// 適用した実行の識別（`active` と同時に入れ替える）。`release`・`step` は同じ実行の記録だけを受ける。
    run_id: Option<u64>,
}

impl fmt::Debug for ClockSkewInjector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClockSkewInjector")
            .field("active", &self.active.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for ClockSkewInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockSkewInjector {
    /// 基準は OS の壁時計（`SystemTime::now`。読むだけで変えない）。
    pub fn new() -> Self {
        Self::with_base_clock(SystemTime::now)
    }

    /// 基準の時計を外から渡す（決定的な試験用）。
    pub fn with_base_clock(base: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        Self {
            skew_ms: Arc::new(AtomicI64::new(0)),
            base: Arc::new(base),
            active: None,
            run_id: None,
        }
    }

    /// 読み取り口の複製。試験対象へ時刻の源として渡す。
    pub fn clock(&self) -> SkewedClock {
        SkewedClock {
            skew_ms: Arc::clone(&self.skew_ms),
            base: Arc::clone(&self.base),
        }
    }

    /// 実行の終了時に、解除の記録なしで共有のずれを 0 へ戻す。`FaultTimeline::finish` の前後どちらでも呼べる。
    /// 記録には触らないので、未解除だった事実は `unreleased_count` に残る。適用中でなければ何もしない。
    pub fn abandon(&mut self) {
        self.skew_ms.store(0, Ordering::SeqCst);
        self.active = None;
        self.run_id = None;
    }

    /// 適用中のずれ（ミリ秒）。
    pub fn active_skew_ms(&self) -> Option<i64> {
        match self.active {
            Some(FaultKind::ClockSkew { skew_ms }) => Some(skew_ms),
            _ => None,
        }
    }

    /// 検査と範囲の事前確認（何も記録しない）。
    fn prepare(&self, skew_ms: i64) -> Result<FaultKind, ClockSkewError> {
        let kind = FaultKind::clock_skew(skew_ms).map_err(|_| ClockSkewError::InvalidParam)?;
        shifted((self.base)(), skew_ms)?;
        Ok(kind)
    }

    /// ずれを開始する。記録（`Applied`）が通ってから読み取り側に見せる。
    ///
    /// # Errors
    /// 0 のずれ（`InvalidParam`）・適用中（`AlreadyActive`）・表せない時刻（`OutOfRange`）・記録の拒否（`Record`）。
    /// 誤りのときは何も記録せず状態も変えない。
    pub fn apply(
        &mut self,
        timeline: &mut FaultTimeline,
        skew_ms: i64,
    ) -> Result<(), ClockSkewError> {
        // 意図: 値の誤り（InvalidParam）を AlreadyActive より先に返す順序を保つため、`prepare` と別に先に検査する。
        FaultKind::clock_skew(skew_ms).map_err(|_| ClockSkewError::InvalidParam)?;
        if self.active.is_some() {
            return Err(ClockSkewError::AlreadyActive);
        }
        let kind = self.prepare(skew_ms)?;
        timeline
            .record_now(kind, FaultPhase::Applied)
            .map_err(ClockSkewError::Record)?;
        self.skew_ms.store(skew_ms, Ordering::SeqCst);
        self.active = Some(kind);
        self.run_id = Some(timeline.run_id());
        Ok(())
    }

    /// 適用した実行の記録かを確かめる（別の実行なら状態も記録も変えない）。
    fn check_run(&self, timeline: &FaultTimeline) -> Result<(), ClockSkewError> {
        if self.run_id == Some(timeline.run_id()) {
            Ok(())
        } else {
            Err(ClockSkewError::RunMismatch)
        }
    }

    /// ずれを解除する。`Applied` と同じ値で `Released` を記録する。
    ///
    /// # Errors
    /// 適用中でなければ `NotActive`、別の実行の記録は `RunMismatch`。記録が拒否されたら `Record` で状態を変えない。
    pub fn release(&mut self, timeline: &mut FaultTimeline) -> Result<(), ClockSkewError> {
        let Some(kind) = self.active else {
            return Err(ClockSkewError::NotActive);
        };
        self.check_run(timeline)?;
        timeline
            .record_now(kind, FaultPhase::Released)
            .map_err(ClockSkewError::Record)?;
        self.skew_ms.store(0, Ordering::SeqCst);
        self.active = None;
        self.run_id = None;
        Ok(())
    }

    /// ずれを別の値へ段階的に変える（AVLB-31）。記録は `Released(旧)` の後に `Applied(新)`。
    ///
    /// # Errors
    /// 適用中でなければ `NotActive`、別の実行の記録は `RunMismatch`（何も変えない）。0・同じ値は `InvalidParam`、表せない時刻は `OutOfRange`（いずれも何も記録しない）。
    /// `Released` の記録が拒否されたら状態を変えない。`Applied` の記録だけが拒否されたときは、記録と一致させて
    /// 「解除済み（ずれなし）」の状態で `Record` を返す。
    pub fn step(
        &mut self,
        timeline: &mut FaultTimeline,
        skew_ms: i64,
    ) -> Result<(), ClockSkewError> {
        let Some(old) = self.active else {
            return Err(ClockSkewError::NotActive);
        };
        self.check_run(timeline)?;
        // 0・同じ値の検査は基準時計の読み取り・範囲検査より先（InvalidParam を OutOfRange に化けさせない）。
        let new = FaultKind::clock_skew(skew_ms).map_err(|_| ClockSkewError::InvalidParam)?;
        if new == old {
            return Err(ClockSkewError::InvalidParam);
        }
        shifted((self.base)(), skew_ms)?;
        timeline
            .record_now(old, FaultPhase::Released)
            .map_err(ClockSkewError::Record)?;
        self.active = None;
        self.run_id = None;
        // 読み取り側に「ずれなし」の時刻を見せないため、旧い値から新しい値へ直接入れ替える。
        // 0 へ戻すのは `Applied` の記録が拒否されたときだけ（記録と一致させる）。
        if let Err(e) = timeline.record_now(new, FaultPhase::Applied) {
            self.skew_ms.store(0, Ordering::SeqCst);
            return Err(ClockSkewError::Record(e));
        }
        self.skew_ms.store(skew_ms, Ordering::SeqCst);
        self.active = Some(new);
        self.run_id = Some(timeline.run_id());
        Ok(())
    }
}

impl Drop for ClockSkewInjector {
    /// 記録には触らず、共有のずれだけを戻す（解除し忘れは `unreleased_count` に出る）。
    fn drop(&mut self) {
        self.abandon();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const BASE_SECS: u64 = 1_000_000;

    fn injector() -> ClockSkewInjector {
        ClockSkewInjector::with_base_clock(|| UNIX_EPOCH + Duration::from_secs(BASE_SECS))
    }

    fn timeline() -> FaultTimeline {
        FaultTimeline::start(Instant::now(), SystemTime::now()).unwrap()
    }

    #[test]
    fn out_of_range_is_rejected_without_state_change() {
        let mut inj = ClockSkewInjector::with_base_clock(|| UNIX_EPOCH + Duration::from_secs(1));
        let mut tl = timeline();
        assert_eq!(inj.apply(&mut tl, -2_000), Err(ClockSkewError::OutOfRange));
        assert_eq!(
            inj.apply(&mut tl, i64::MIN),
            Err(ClockSkewError::OutOfRange)
        );
        assert_eq!(inj.active_skew_ms(), None);
        assert_eq!(inj.clock().skew_ms(), 0);
        // 境界: ちょうど紀元は表せる。
        inj.apply(&mut tl, -1_000).unwrap();
        assert_eq!(inj.clock().unix_seconds(), Ok(0));
    }

    #[test]
    fn rejected_release_in_step_keeps_state() {
        let mut inj = injector();
        let mut tl = timeline();
        inj.apply(&mut tl, 1_000).unwrap();
        // 基準線の timeline は別の実行なので Released の前に拒否される: 状態は変わらない。
        let mut base = FaultTimeline::baseline(Instant::now(), SystemTime::now()).unwrap();
        assert!(matches!(
            inj.step(&mut base, 2_000),
            Err(ClockSkewError::RunMismatch)
        ));
        assert_eq!(inj.active_skew_ms(), Some(1_000));
        assert_eq!(inj.clock().skew_ms(), 1_000);
    }

    #[test]
    fn other_run_timeline_is_rejected() {
        let mut inj = injector();
        let mut tl = timeline();
        let mut other = timeline();
        inj.apply(&mut tl, 1_000).unwrap();
        assert_eq!(inj.release(&mut other), Err(ClockSkewError::RunMismatch));
        assert_eq!(
            inj.step(&mut other, 2_000),
            Err(ClockSkewError::RunMismatch)
        );
        assert_eq!(inj.active_skew_ms(), Some(1_000));
        assert_eq!(inj.clock().skew_ms(), 1_000);
        inj.release(&mut tl).unwrap();
    }

    #[test]
    fn step_same_or_zero_is_invalid_even_if_base_is_out_of_range() {
        use std::sync::atomic::AtomicU64;
        let secs = Arc::new(AtomicU64::new(BASE_SECS));
        let s2 = Arc::clone(&secs);
        let mut inj = ClockSkewInjector::with_base_clock(move || {
            UNIX_EPOCH + Duration::from_secs(s2.load(Ordering::SeqCst))
        });
        let mut tl = timeline();
        inj.apply(&mut tl, -1_000_000).unwrap();
        // 基準が後退して現在の値でも範囲外になる状態。
        secs.store(1, Ordering::SeqCst);
        assert_eq!(
            inj.step(&mut tl, -1_000_000),
            Err(ClockSkewError::InvalidParam)
        );
        assert_eq!(inj.step(&mut tl, 0), Err(ClockSkewError::InvalidParam));
    }

    #[test]
    fn debug_and_error_do_not_leak_values() {
        let mut inj = injector();
        let mut tl = timeline();
        inj.apply(&mut tl, 123_457).unwrap();
        let s = format!("{inj:?} {:?}", inj.clock());
        assert!(!s.contains("123457") && !s.contains("1000000"));
        let e = format!("{}", ClockSkewError::OutOfRange);
        assert_eq!(e, "clock skew fault error: skewed time out of range");
    }
}
