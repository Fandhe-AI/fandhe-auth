//! 集計の処理（TASK-12 4/10）。
//!
//! 役割: 計測の部品（[`crate::meter`]）が集めた [`RunSamples`] から、区間ごとの百分位・最大・件数を作る純粋な処理。
//! 呼び出し元: 記録の出力の段・段階を回す処理（8/10）・単体試験。呼び出し先は無い（時計・ファイル・環境変数を読まない）。
//!
//! 百分位は最近順位法（補間なし）で、順位は整数演算で求める（`docs/harness/load.md` の「p95・p99」）。
//! 不変条件を満たさない入力・宣言外の区間は黙って丸めず誤りにする（fail-closed）。
//!
//! 範囲外: `fandhe-auth-load-record/1` の出力・`class=` の付与、負荷の生成、合否（`ACPT-7`・`TASK-21`・`AUDP-29`）。
//! 安全: 誤りは種類だけを持ち、名前の中身・値を載せない（`SECR-11`・`SECR-23`）。検証を外す分岐は作らない（`DPOL-30`〜`32`）。

use std::collections::BTreeMap;
use std::fmt;

use crate::meter::{FailureKind, IntervalName, RunSamples, Sample};

/// 集計の誤りの種類。値・名前の中身は載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateError {
    /// 区間の宣言が 0 件、または重複している。
    InvalidIntervals,
    /// 宣言していない区間（標本・比べる区間）。
    UndeclaredInterval,
    /// 回が 1 つも無い。
    NoRuns,
    /// 件数・順位の計算が溢れた。
    CounterOverflow,
    /// 件数・分布の不変条件を満たさない。
    InvariantViolated,
}

impl fmt::Display for AggregateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidIntervals => "invalid interval declaration",
            Self::UndeclaredInterval => "undeclared interval",
            Self::NoRuns => "no runs",
            Self::CounterOverflow => "counter overflow",
            Self::InvariantViolated => "aggregate invariant violated",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for AggregateError {}

/// 1 つの区間の分布の要約。`n = 0` のとき値はすべて `None`（0 と書かない）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalStats {
    pub n: u64,
    pub p50_ns: Option<u64>,
    pub p95_ns: Option<u64>,
    pub p99_ns: Option<u64>,
    pub max_ns: Option<u64>,
    /// 最大を持つ標本の通し番号。同値は通し番号の最も小さいもの（仮置き）。
    pub max_trial: Option<u64>,
}

/// 最近順位法の値。k = ceil(p * n / 100) を整数で求め、x(k) を返す。
fn nearest_rank(sorted: &[u64], p: u128) -> Result<u64, AggregateError> {
    let n = u128::try_from(sorted.len()).map_err(|_| AggregateError::CounterOverflow)?;
    let k = p
        .checked_mul(n)
        .and_then(|v| v.checked_add(99))
        .ok_or(AggregateError::CounterOverflow)?
        / 100;
    if k < 1 || k > n {
        return Err(AggregateError::InvariantViolated);
    }
    let idx = usize::try_from(k - 1).map_err(|_| AggregateError::CounterOverflow)?;
    sorted
        .get(idx)
        .copied()
        .ok_or(AggregateError::InvariantViolated)
}

/// 標本の並びから分布を要約する。入力の並びに依らない。
///
/// # Errors
/// 件数の変換が溢れたとき、または順位・単調性の不変条件が崩れたとき。
pub fn summarize_interval(samples: &[Sample]) -> Result<IntervalStats, AggregateError> {
    let n = u64::try_from(samples.len()).map_err(|_| AggregateError::CounterOverflow)?;
    if samples.is_empty() {
        return Ok(IntervalStats {
            n,
            p50_ns: None,
            p95_ns: None,
            p99_ns: None,
            max_ns: None,
            max_trial: None,
        });
    }
    let mut sorted: Vec<u64> = samples.iter().map(|s| s.ns).collect();
    sorted.sort_unstable();
    let max_ns = sorted
        .last()
        .copied()
        .ok_or(AggregateError::InvariantViolated)?;
    let max_trial = samples
        .iter()
        .filter(|s| s.ns == max_ns)
        .map(|s| s.trial)
        .min();
    let p50 = nearest_rank(&sorted, 50)?;
    let p95 = nearest_rank(&sorted, 95)?;
    let p99 = nearest_rank(&sorted, 99)?;
    if !(p50 <= p95 && p95 <= p99 && p99 <= max_ns) {
        return Err(AggregateError::InvariantViolated);
    }
    Ok(IntervalStats {
        n,
        p50_ns: Some(p50),
        p95_ns: Some(p95),
        p99_ns: Some(p99),
        max_ns: Some(max_ns),
        max_trial,
    })
}

/// 1 回の「回」の要約。成功の分布（`metric` 行の材料）と失敗の分布（`failure_metric` 行の材料）は別。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSummary {
    /// 宣言した区間すべてを持つ（`n = 0` を含む）。
    pub success_metrics: BTreeMap<IntervalName, IntervalStats>,
    pub failure_metrics: BTreeMap<IntervalName, IntervalStats>,
    pub executed: u64,
    pub success: u64,
    pub failure: u64,
    pub timeout: u64,
    pub excluded: u64,
    pub failure_kinds: BTreeMap<FailureKind, u64>,
    pub max_in_flight: u64,
}

fn summarize_map(
    src: &BTreeMap<IntervalName, Vec<Sample>>,
    intervals: &[IntervalName],
    limit: u64,
) -> Result<BTreeMap<IntervalName, IntervalStats>, AggregateError> {
    if src.keys().any(|k| !intervals.contains(k)) {
        return Err(AggregateError::UndeclaredInterval);
    }
    let mut out = BTreeMap::new();
    for name in intervals {
        let stats = match src.get(name) {
            Some(v) => summarize_interval(v)?,
            None => summarize_interval(&[])?,
        };
        if stats.n > limit {
            return Err(AggregateError::InvariantViolated);
        }
        out.insert(name.clone(), stats);
    }
    Ok(out)
}

/// 1 回分の標本から要約を作る。`intervals` は宣言した区間（全試行が時間切れでも `n = 0` の行を作るため）。
///
/// # Errors
/// 宣言が 0 件・重複のとき [`AggregateError::InvalidIntervals`]、宣言外の標本は [`AggregateError::UndeclaredInterval`]、
/// 件数の不変条件違反は [`AggregateError::InvariantViolated`]。
pub fn summarize_run(
    samples: &RunSamples,
    intervals: &[IntervalName],
) -> Result<RunSummary, AggregateError> {
    if intervals.is_empty() {
        return Err(AggregateError::InvalidIntervals);
    }
    for (i, a) in intervals.iter().enumerate() {
        if intervals.iter().skip(i + 1).any(|b| a == b) {
            return Err(AggregateError::InvalidIntervals);
        }
    }
    let total = samples
        .success
        .checked_add(samples.failure)
        .and_then(|v| v.checked_add(samples.timeout))
        .ok_or(AggregateError::CounterOverflow)?;
    if total != samples.executed {
        return Err(AggregateError::InvariantViolated);
    }
    let kinds = samples
        .failure_kinds
        .values()
        .try_fold(0u64, |acc, v| acc.checked_add(*v))
        .ok_or(AggregateError::CounterOverflow)?;
    if kinds != samples.failure {
        return Err(AggregateError::InvariantViolated);
    }
    let success_metrics = summarize_map(&samples.success_samples, intervals, samples.success)?;
    let failure_metrics = summarize_map(&samples.failure_samples, intervals, samples.failure)?;
    Ok(RunSummary {
        success_metrics,
        failure_metrics,
        executed: samples.executed,
        success: samples.success,
        failure: samples.failure,
        timeout: samples.timeout,
        excluded: samples.excluded,
        failure_kinds: samples.failure_kinds.clone(),
        max_in_flight: samples.max_in_flight,
    })
}

/// 回を比べる指標。既定は置かない（呼び出し元が明示する）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKey {
    P50Ns,
    P95Ns,
    P99Ns,
    MaxNs,
}

impl MetricKey {
    /// 記録の `by=` のトークン。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::P50Ns => "p50_ns",
            Self::P95Ns => "p95_ns",
            Self::P99Ns => "p99_ns",
            Self::MaxNs => "max_ns",
        }
    }

    fn pick(&self, s: &IntervalStats) -> Option<u64> {
        match self {
            Self::P50Ns => s.p50_ns,
            Self::P95Ns => s.p95_ns,
            Self::P99Ns => s.p99_ns,
            Self::MaxNs => s.max_ns,
        }
    }
}

/// 最悪の回の通し番号（1 始まり）を返す。成功の分布だけで比べ、値の大きい回を悪いとする。
/// 同値は小さい回を採る。標本の無い回は候補にしない。全回が標本無しなら `None`。
///
/// # Errors
/// 回が空は [`AggregateError::NoRuns`]、どの回にも無い区間は [`AggregateError::UndeclaredInterval`]。
pub fn worst_run(
    runs: &[RunSummary],
    by: MetricKey,
    interval: &IntervalName,
) -> Result<Option<u64>, AggregateError> {
    if runs.is_empty() {
        return Err(AggregateError::NoRuns);
    }
    if runs
        .iter()
        .all(|r| !r.success_metrics.contains_key(interval))
    {
        return Err(AggregateError::UndeclaredInterval);
    }
    let mut best: Option<(u64, usize)> = None;
    for (i, r) in runs.iter().enumerate() {
        let Some(v) = r.success_metrics.get(interval).and_then(|s| by.pick(s)) else {
            continue;
        };
        // 厳密に大きいときだけ置き換えるので、同値は先の（小さい）回が残る。
        let better = match best {
            None => true,
            Some((bv, _)) => v > bv,
        };
        if better {
            best = Some((v, i));
        }
    }
    match best {
        None => Ok(None),
        Some((_, i)) => {
            let n = u64::try_from(i)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or(AggregateError::CounterOverflow)?;
            Ok(Some(n))
        }
    }
}
