//! 負荷の対象の呼び出しの部品（TASK-12 6/10）。名前・語・値はすべて仮置きで、人が確認する。
//!
//! 役割: 経路の名前・測る区間・呼び出しを 1 組で受け、[`crate::generate::run_closed_loop`] の上で閉ループで呼び、
//! 呼び出しの結果を固定の語彙（[`CallResult`]）で [`Outcome`] に写し、結果を [`TargetRun`] に残す。
//! 呼び出し元: 結合試験のハーネスの上で構成要素へ負荷をかける試験（試験ビルド）。
//! 呼び出し先: 計測の部品・生成器・集計だけ。対象を具体の stub に結ぶ部分は呼び出し元（試験）が書く。
//! 時計は注入で受け、環境変数・設定ファイル・壁時計を読まない（DPOL-32）。
//!
//! 結果の記録: [`TargetRun`] の `run`・`summary` に残る。試行番号と時間の標本が残るのは成功と失敗（種類を問わない）だけで、
//! 失敗の種類（`rejected`・`unavailable`）・時間切れ・暖機の除外は件数だけが残り、標本は残らない。
//! `fandhe-auth-load-record/1` のファイルへの出力は未提供で、担当の段は未決（`docs/harness/load.md`）。
//! 対象は stub だけで、実物の経路は最小の組込み先の完成後に呼ぶ。
//! TODO(TASK-716): 実物の経路の呼び出しは最小の組込み先の完成後に足す。stub の合格を実物の合格に数えない
//! （A-2026-10-28-9・ACPT-7）。合否は付けない。

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::aggregate::{AggregateError, RunSummary, summarize_run};
use crate::generate::{LoadConfig, LoadError, LoadRun, run_closed_loop_until};
use crate::meter::{
    FailureKind, IntervalName, MeterError, MonotonicClock, Outcome, RunMeter, validate_name,
};

/// 拒否の失敗の種類名（仮置き）。
const KIND_REJECTED: &str = "rejected";
/// 到達不可の失敗の種類名（仮置き）。
const KIND_UNAVAILABLE: &str = "unavailable";

/// 経路の名前。規則は [`IntervalName`] と同じ（ASCII の `[a-z0-9_]`・1〜64 文字。記録への差し込み対策）。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetName(String);

impl TargetName {
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

impl fmt::Debug for TargetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TargetName({})", self.0)
    }
}

/// 1 回の呼び出しの結果。状態の番号・本文・誤りの文言は持たない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallResult {
    /// 成功。
    Ok,
    /// 対象が拒否した。
    Rejected,
    /// 対象に到達できなかった。
    Unavailable,
    /// 時間切れ。
    TimedOut,
}

impl CallResult {
    /// 写しの規則（固定。1 か所）。対象から `Excluded` を返す道は作らない（除外は生成器だけが付ける）。
    fn to_outcome(self) -> Result<Outcome, MeterError> {
        Ok(match self {
            Self::Ok => Outcome::Success,
            Self::Rejected => Outcome::Failure(FailureKind::new(KIND_REJECTED)?),
            Self::Unavailable => Outcome::Failure(FailureKind::new(KIND_UNAVAILABLE)?),
            Self::TimedOut => Outcome::Timeout,
        })
    }
}

/// 経路と、測る区間 1 つの組。既定値は置かない。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSpec {
    name: TargetName,
    interval: IntervalName,
}

impl TargetSpec {
    pub fn new(name: TargetName, interval: IntervalName) -> Self {
        Self { name, interval }
    }

    pub fn name(&self) -> &TargetName {
        &self.name
    }

    pub fn interval(&self) -> &IntervalName {
        &self.interval
    }
}

/// 結果の記録。持つのは検査済みの名前・件数・時間だけ（SECR-11・SECR-23）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetRun {
    pub target: TargetName,
    pub interval: IntervalName,
    pub run: LoadRun,
    pub summary: RunSummary,
}

/// 対象の呼び出しの誤り。値・名前の中身・panic の中身は載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetError {
    /// 負荷の生成の誤り。
    Load(LoadError),
    /// 集計の誤り。
    Aggregate(AggregateError),
    /// 対象の中で起きた計測の誤り（元の種類を保つ）。
    Meter(MeterError),
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::Load(_) => "load error",
            Self::Aggregate(_) => "aggregate error",
            Self::Meter(_) => "meter error in target",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for TargetError {}

impl From<LoadError> for TargetError {
    fn from(e: LoadError) -> Self {
        Self::Load(e)
    }
}

impl From<AggregateError> for TargetError {
    fn from(e: AggregateError) -> Self {
        Self::Aggregate(e)
    }
}

impl From<MeterError> for TargetError {
    fn from(e: MeterError) -> Self {
        Self::Meter(e)
    }
}

/// 対象を閉ループで呼び、結果を [`TargetRun`] に残す。
///
/// 閉包は試行ごとに 1 回呼ばれ、`spec` の区間で測られる。対象の中で計測の誤りが起きたら最初の誤りを残し、
/// 試行を `Excluded` で締め、停止の印（`Outcome` とは別の経路）を生成器へ渡す。印が立つと生成器は
/// 暖機・本計測のどちらでも新しい試行を始めず、本計測の段へも進まない。
///
/// 停止と呼び出しの受け付け: 閉包を呼ぶ直前（区間の始点の時計を読んだ後）に印を確かめ、立っていれば呼ばない。
/// 保証できるのは新規の受け付けの抑止までで、確認を通った受け付け済みの呼び出しは、確認の後に印が立っても
/// 止めずに最後まで続く（途中で止めない。終点の時計の誤りがあれば同じ規則で残す）。
/// 「記録より後に始まる呼び出しは 0 件」までは保証しない（確認と閉包の実行開始の間に印が立ち得る）。
/// 閉包を呼ばずに返した試行（`Outcome::Excluded`）は、停止の印が先に立った経路でだけ生じ、同時に誤りが
/// 残っているので、本計測の `InvalidOutcome` ではなく、戻りで残した誤りが先に [`TargetError::Meter`] で返る。
/// 戻ったら結果より先に残した誤りを見て、黙って数えず [`TargetError::Meter`] を返す。
/// panic・`unwrap` は使わない。
///
/// # Errors
/// 計測の誤り・生成の誤り・集計の誤り。
pub fn run_target<F>(
    clock: Box<dyn MonotonicClock>,
    spec: &TargetSpec,
    config: &LoadConfig,
    call: F,
) -> Result<TargetRun, TargetError>
where
    F: Fn() -> CallResult + Sync,
{
    let interval = spec.interval();
    let meter = RunMeter::new(clock, std::slice::from_ref(interval))?;
    let first_error: Mutex<Option<MeterError>> = Mutex::new(None);
    let failed = AtomicBool::new(false);
    let record = |e: MeterError| {
        failed.store(true, Ordering::SeqCst);
        let mut g = first_error.lock().unwrap_or_else(PoisonError::into_inner);
        if g.is_none() {
            *g = Some(e);
        }
    };
    let result = run_closed_loop_until(meter, config, &failed, |trial| {
        if failed.load(Ordering::SeqCst) {
            return Outcome::Excluded;
        }
        // 受け付けは始点の時計の後・呼び出しの直前で確かめる（時計を待つ間に立った印を見逃さない）。
        let called = match trial.measure(interval, || {
            if failed.load(Ordering::SeqCst) {
                None
            } else {
                Some(call())
            }
        }) {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::Excluded,
            Err(e) => {
                record(e);
                return Outcome::Excluded;
            }
        };
        match called.to_outcome() {
            Ok(o) => o,
            Err(e) => {
                record(e);
                Outcome::Excluded
            }
        }
    });
    let recorded = *first_error.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(e) = recorded {
        return Err(TargetError::Meter(e));
    }
    let run = result?;
    let summary = summarize_run(&run.samples, std::slice::from_ref(interval))?;
    Ok(TargetRun {
        target: spec.name().clone(),
        interval: interval.clone(),
        run,
        summary,
    })
}
