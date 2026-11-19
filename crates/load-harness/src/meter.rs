//! 計測の部品の本体（TASK-12 3/10）。
//!
//! 呼び出し元: 負荷を生成するハーネス（5/10 以降）と単体試験。呼び出し先は注入された [`MonotonicClock`] だけ。
//! 時間は整数ナノ秒で持ち、算術は `checked` にして、溢れ・逆転は黙って丸めず誤りにする。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

/// 名前の最大の長さ。`docs/harness/load.md` の 6（差し込み対策）。
const NAME_MAX_LEN: usize = 64;

/// 締めずに捨てた試行を数える失敗の種類名。実行 = 成功 + 失敗 + 時間切れの不変条件を崩さないために失敗へ入れる。
/// 仮置き（人の確認待ち）。
const UNFINISHED_KIND: &str = "unfinished";

/// 誤りの種類。値・名前の中身・パスは載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterError {
    /// 名前が `[a-z0-9_]`・1〜64 文字を満たさない。
    InvalidName,
    /// 区間の宣言が 0 件、または重複している。
    InvalidIntervals,
    /// 宣言していない区間名。
    UndeclaredInterval,
    /// 同じ区間を 2 回始めた。
    DuplicateStart,
    /// 始めていない区間、または終わった区間を終えようとした。
    NotStarted,
    /// 終点が始点より前（時計の逆転）。
    ClockReversed,
    /// 時計の値が `u64` ナノ秒に収まらない。
    ClockOverflow,
    /// 開いたままの区間、または成功なのに測っていない区間がある。
    IncompleteIntervals,
    /// 通し番号・件数が溢れた。
    CounterOverflow,
    /// 試行が残ったまま取り出そうとした。
    TrialsInFlight,
    /// 件数の不変条件を満たさない。
    InvariantViolated,
    /// 内部の状態が壊れている（ロックの汚染）。
    StatePoisoned,
}

impl fmt::Display for MeterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidName => "invalid name",
            Self::InvalidIntervals => "invalid interval declaration",
            Self::UndeclaredInterval => "undeclared interval",
            Self::DuplicateStart => "interval already started",
            Self::NotStarted => "interval not started",
            Self::ClockReversed => "clock went backwards",
            Self::ClockOverflow => "clock value out of range",
            Self::IncompleteIntervals => "incomplete intervals",
            Self::CounterOverflow => "counter overflow",
            Self::TrialsInFlight => "trials still in flight",
            Self::InvariantViolated => "count invariant violated",
            Self::StatePoisoned => "meter state poisoned",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for MeterError {}

fn validate_name(s: &str) -> Result<(), MeterError> {
    let ok = !s.is_empty()
        && s.len() <= NAME_MAX_LEN
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(MeterError::InvalidName)
    }
}

/// 区間名。ASCII の `[a-z0-9_]`・1〜64 文字を作る時点で検査する（記録への差し込み対策）。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntervalName(String);

impl IntervalName {
    /// # Errors
    /// 規則を満たさないとき [`MeterError::InvalidName`]。
    pub fn new(s: &str) -> Result<Self, MeterError> {
        validate_name(s)?;
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IntervalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IntervalName({})", self.0)
    }
}

/// 失敗の種類名。規則は [`IntervalName`] と同じ。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FailureKind(String);

impl FailureKind {
    /// # Errors
    /// 規則を満たさないとき [`MeterError::InvalidName`]。
    pub fn new(s: &str) -> Result<Self, MeterError> {
        validate_name(s)?;
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FailureKind({})", self.0)
    }
}

/// 時計の注入点。時計の原点からの経過を整数ナノ秒で返す。壁時計は差に使わない。
/// 試験は手で進める偽の時計を渡し、実際に経過した時間を見ない。
pub trait MonotonicClock: Send + Sync {
    fn now_ns(&self) -> Result<u64, MeterError>;
}

/// 本番の時計。作った時点の `Instant` を原点にする。
#[derive(Debug)]
pub struct StdMonotonicClock {
    origin: Instant,
}

impl StdMonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for StdMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for StdMonotonicClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        u64::try_from(self.origin.elapsed().as_nanos()).map_err(|_| MeterError::ClockOverflow)
    }
}

/// 1 つの標本。通し番号（1 始まり）と所要時間（ナノ秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub trial: u64,
    pub ns: u64,
}

/// 1 試行の結果の種類。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 成功。区間の時間を成功の標本へ入れる。
    Success,
    /// 失敗。区間の時間を失敗の標本へ入れる（成功とは別）。
    Failure(FailureKind),
    /// 時間切れ。件数だけを数え、時間は持たない。
    Timeout,
    /// 除外（暖機など）。実行の外の件数だけを数える。
    Excluded,
}

/// 取り出した結果。4/10 の集計の入力。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSamples {
    pub success_samples: BTreeMap<IntervalName, Vec<Sample>>,
    pub failure_samples: BTreeMap<IntervalName, Vec<Sample>>,
    pub executed: u64,
    pub success: u64,
    pub failure: u64,
    pub timeout: u64,
    pub excluded: u64,
    pub failure_kinds: BTreeMap<FailureKind, u64>,
    pub max_in_flight: u64,
}

fn bump(c: &mut u64) -> Result<(), MeterError> {
    *c = c.checked_add(1).ok_or(MeterError::CounterOverflow)?;
    Ok(())
}

/// 1 回の「回」の計測。`Send + Sync`。共有の状態のロックは試行を締めるときだけ取り、測る区間の中では取らない。
pub struct RunMeter {
    clock: Box<dyn MonotonicClock>,
    intervals: BTreeSet<IntervalName>,
    next_trial: AtomicU64,
    in_flight: AtomicU64,
    max_in_flight: AtomicU64,
    state: Mutex<RunSamples>,
}

impl RunMeter {
    /// 計測する区間名の集まりを宣言して作る。宣言外の区間は記録できない（fail-closed）。
    ///
    /// # Errors
    /// 0 件・重複のとき [`MeterError::InvalidIntervals`]。
    pub fn new(
        clock: Box<dyn MonotonicClock>,
        intervals: &[IntervalName],
    ) -> Result<Self, MeterError> {
        let set: BTreeSet<IntervalName> = intervals.iter().cloned().collect();
        if set.is_empty() || set.len() != intervals.len() {
            return Err(MeterError::InvalidIntervals);
        }
        Ok(Self {
            clock,
            intervals: set,
            next_trial: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            max_in_flight: AtomicU64::new(0),
            state: Mutex::new(RunSamples::default()),
        })
    }

    /// 試行を始める。通し番号（1 始まり）を振り、同時実行の数とその最大を更新する。
    ///
    /// # Errors
    /// 通し番号・同時実行の数が溢れたとき [`MeterError::CounterOverflow`]。
    pub fn begin_trial(&self) -> Result<Trial<'_>, MeterError> {
        let prev = self
            .next_trial
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_add(1))
            .map_err(|_| MeterError::CounterOverflow)?;
        let id = prev.checked_add(1).ok_or(MeterError::CounterOverflow)?;
        let prev_in = self
            .in_flight
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| v.checked_add(1))
            .map_err(|_| MeterError::CounterOverflow)?;
        let now = prev_in.checked_add(1).ok_or(MeterError::CounterOverflow)?;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        Ok(Trial {
            meter: self,
            id,
            slots: BTreeMap::new(),
            done: false,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, RunSamples>, MeterError> {
        self.state.lock().map_err(|_| MeterError::StatePoisoned)
    }

    /// ここまでの結果を取り出す。不変条件を検査してから返す。
    ///
    /// # Errors
    /// 試行が残っていれば [`MeterError::TrialsInFlight`]、不変条件を満たさなければ
    /// [`MeterError::InvariantViolated`]。
    pub fn snapshot(&self) -> Result<RunSamples, MeterError> {
        if self.in_flight.load(Ordering::SeqCst) != 0 {
            return Err(MeterError::TrialsInFlight);
        }
        let mut out = self.lock()?.clone();
        out.max_in_flight = self.max_in_flight.load(Ordering::SeqCst);
        check_invariants(&out)?;
        Ok(out)
    }

    /// 計測を終えて結果を取り出す。
    ///
    /// # Errors
    /// [`RunMeter::snapshot`] と同じ。
    pub fn finish(self) -> Result<RunSamples, MeterError> {
        self.snapshot()
    }
}

fn check_invariants(s: &RunSamples) -> Result<(), MeterError> {
    let executed = s
        .success
        .checked_add(s.failure)
        .and_then(|v| v.checked_add(s.timeout))
        .ok_or(MeterError::CounterOverflow)?;
    let kinds = s
        .failure_kinds
        .values()
        .try_fold(0u64, |a, v| a.checked_add(*v))
        .ok_or(MeterError::CounterOverflow)?;
    if executed != s.executed || kinds != s.failure {
        return Err(MeterError::InvariantViolated);
    }
    Ok(())
}

/// 測り終えた区間の時間を、試行の通し番号つきの標本として分布の入れ物へ足す。
fn collect_samples(
    map: &mut BTreeMap<IntervalName, Vec<Sample>>,
    slots: &BTreeMap<IntervalName, Slot>,
    id: u64,
) {
    for (n, slot) in slots {
        if let Some(ns) = slot.ns {
            map.entry(n.clone())
                .or_default()
                .push(Sample { trial: id, ns });
        }
    }
}

#[derive(Default)]
struct Slot {
    start: u64,
    ns: Option<u64>,
}

/// 1 試行。締めずに捨てると `unfinished` の失敗として数える（黙って落とさない。仮置き）。
pub struct Trial<'a> {
    meter: &'a RunMeter,
    id: u64,
    slots: BTreeMap<IntervalName, Slot>,
    done: bool,
}

impl Trial<'_> {
    /// 通し番号（1 始まり）。
    pub fn id(&self) -> u64 {
        self.id
    }

    fn declared(&self, name: &IntervalName) -> Result<(), MeterError> {
        if self.meter.intervals.contains(name) {
            Ok(())
        } else {
            Err(MeterError::UndeclaredInterval)
        }
    }

    /// 区間の始点を記録する。
    ///
    /// # Errors
    /// 宣言外は [`MeterError::UndeclaredInterval`]、二重の開始は [`MeterError::DuplicateStart`]。
    pub fn start(&mut self, name: &IntervalName) -> Result<(), MeterError> {
        self.declared(name)?;
        if self.slots.contains_key(name) {
            return Err(MeterError::DuplicateStart);
        }
        let now = self.meter.clock.now_ns()?;
        self.slots.insert(
            name.clone(),
            Slot {
                start: now,
                ns: None,
            },
        );
        Ok(())
    }

    /// 区間の終点を記録する。終点が始点より前なら誤り（0 に丸めない）。
    ///
    /// # Errors
    /// 始めていない・終えた区間は [`MeterError::NotStarted`]、逆転は [`MeterError::ClockReversed`]。
    pub fn end(&mut self, name: &IntervalName) -> Result<(), MeterError> {
        self.declared(name)?;
        let now = self.meter.clock.now_ns()?;
        let slot = self.slots.get_mut(name).ok_or(MeterError::NotStarted)?;
        if slot.ns.is_some() {
            return Err(MeterError::NotStarted);
        }
        slot.ns = Some(
            now.checked_sub(slot.start)
                .ok_or(MeterError::ClockReversed)?,
        );
        Ok(())
    }

    /// 閉包を区間として測る。
    ///
    /// # Errors
    /// [`Trial::start`]・[`Trial::end`] と同じ。
    pub fn measure<R>(
        &mut self,
        name: &IntervalName,
        f: impl FnOnce() -> R,
    ) -> Result<R, MeterError> {
        self.start(name)?;
        let r = f();
        self.end(name)?;
        Ok(r)
    }

    /// 結果の種類を付けて締める。誤りのときその試行は `unfinished` の失敗として数える。
    ///
    /// # Errors
    /// 開いたままの区間、成功なのに測っていない区間は [`MeterError::IncompleteIntervals`]。
    pub fn finish(mut self, outcome: Outcome) -> Result<(), MeterError> {
        if self.slots.values().any(|s| s.ns.is_none()) {
            return Err(MeterError::IncompleteIntervals);
        }
        if matches!(outcome, Outcome::Success) && self.slots.len() != self.meter.intervals.len() {
            return Err(MeterError::IncompleteIntervals);
        }
        let id = self.id;
        let mut s = self.meter.lock()?;
        match outcome {
            Outcome::Success => {
                bump(&mut s.executed)?;
                bump(&mut s.success)?;
                collect_samples(&mut s.success_samples, &self.slots, id);
            }
            Outcome::Failure(kind) => {
                bump(&mut s.executed)?;
                bump(&mut s.failure)?;
                bump(s.failure_kinds.entry(kind).or_insert(0))?;
                collect_samples(&mut s.failure_samples, &self.slots, id);
            }
            Outcome::Timeout => {
                bump(&mut s.executed)?;
                bump(&mut s.timeout)?;
            }
            Outcome::Excluded => bump(&mut s.excluded)?,
        }
        drop(s);
        self.done = true;
        Ok(())
    }
}

impl Drop for Trial<'_> {
    fn drop(&mut self) {
        if !self.done {
            // 締めずに捨てた試行は失敗 `unfinished` として数える。ロックが汚染されていても数える。
            let mut s = self.meter.state.lock().unwrap_or_else(|e| e.into_inner());
            let kind = FailureKind(UNFINISHED_KIND.to_owned());
            // 溢れたときは取り出しの不変条件の検査で誤りになる。
            let _ = bump(&mut s.executed);
            let _ = bump(&mut s.failure);
            let _ = bump(s.failure_kinds.entry(kind).or_insert(0));
        }
        self.meter.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}
