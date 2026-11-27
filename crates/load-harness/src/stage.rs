//! 並行数を段階的に変える実行の部品（TASK-12 8/10）。名前・語・値はすべて仮置きで、人が確認する。
//!
//! 役割: 並行数・件数・暖機・繰り返しの違う複数の段階を順に回し、結果を段階ごとに分けて [`StagedRun`] に残す。
//! 各回は [`crate::target::run_target`] で回し、段階の全回が終わったら [`crate::aggregate::worst_run`] で段階の最悪の回を求める。
//! [`StagedRun`] の `Display` は `docs/harness/load.md` の 3・4 の `stage`・`run`・`failure_kind`・`metric`・`failure_metric`・
//! `worst_run` の行を段階ごとに出す。
//! 呼び出し元: 結合試験のハーネスの上で構成要素へ負荷をかける試験（試験ビルド）と、記録の元を作る
//! [`crate::record::RunSession::run`]（9/10）。
//! 呼び出し先: 対象の呼び出し（`target`）と集計（`aggregate`）だけ。時計は回ごとに作る関数（`Result` を返す）で受け、
//! 環境変数・設定ファイル・壁時計を読まない（DPOL-32）。
//! 時計の生成の誤りは段階と回の番号付きの [`StageError::Clock`] で返す。生成関数の panic は捕捉せず、そのまま伝わる
//! （生成関数のために panic hook を差し替え・抑止しない。境界は [`run_stages`] の doc）。
//! 誤りの型を `MeterError` に固定したことと variant 名は仮置きで、人の確認待ち（`docs/harness/load.md`）。
//!
//! 段階と回は順に回し、重ならない（`run_target` は自分のワーカーを join して戻る）。
//! 誤り（fail-closed・仮置き）: 最初の誤りで以後の回・段階を始めず、終わった段階の結果も返さない
//! （部分の結果を正しい結果に見せないため）。
//! 範囲外: `fandhe-auth-load-record/1` のファイル全体の出力（`format:`・`run_record_file:`・`judgment:`。担当の段は未決）、
//! 開ループ、合否の判定（ACPT-7）。機材と構成（`condition`）は記録の単位で 1 つ持つので、ここには入れない。
//! 安全: 行に出すのは検査済みの名前・固定の語・整数だけで、自由文を出さない（`load.md` の 6）。
//! 誤りは種類と整数の番号だけを持つ（SECR-11・SECR-23）。検証を外す意味の分岐・feature は作らない（DPOL-30〜32）。
//! TODO(TASK-716): 実物の経路の呼び出しは最小の組込み先の完成後に足す。stub の合格を実物の合格に数えない
//! （A-2026-10-28-9・ACPT-7）。

use std::fmt;

use crate::aggregate::{AggregateError, IntervalStats, MetricKey, RunSummary, worst_run};
use crate::generate::LoadConfig;
use crate::meter::{IntervalName, MeterError, MonotonicClock};
use crate::target::{CallResult, TargetError, TargetName, TargetSpec, run_target};

/// 記録に出してよい区間名の固定語彙（仮置き。人が確認する。SECR-23・`load.md` の 6）。
/// 呼び出し元は足せない（語彙を受ける引数・feature・環境変数を作らない。DPOL-30〜32）。
/// 記録に載る区間名を固定語彙に限るため、名前を渡す入力から語彙を独立させる。検査は完全一致で、語彙にない文字列を拒否する（入力の由来は見ない。#6038）。
/// 区間名の検査は記録へ出す入口の [`run_stages`] だけで行う（`IntervalName::new` は記録を出さない部品でも使うため一般の名前のまま）。
/// 中身は `load.md` に出る名前と試し実行の区間だけの最小で、足すのは人（PR のレビュー）。
pub const RECORD_INTERVALS: [&str; 2] = ["client_response", "stub_jwks"];

/// 計画の上限。呼び出し元が実行環境に合わせて明示する（既定値・固定の上限は置かない。DPOL-32）。
/// 値の導出は本 crate の範囲外。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageLimits {
    max_stages: u64,
    max_repetitions: u64,
    max_trials_per_run: u64,
    max_total_trials: u64,
}

impl StageLimits {
    /// `max_stages`: 段階の数、`max_repetitions`: 1 段階の繰り返し、
    /// `max_trials_per_run`: 1 回の試行数（ウォームアップ + 計測。回の終了まで生標本を保持するため）、
    /// `max_total_trials`: 計画全体の試行数（繰り返しを含む合計。回ごとの集計を全回ぶん保持するため）。
    ///
    /// # Errors
    /// どれかが 0 のとき [`StageError::InvalidPlan`]。
    pub fn new(
        max_stages: u64,
        max_repetitions: u64,
        max_trials_per_run: u64,
        max_total_trials: u64,
    ) -> Result<Self, StageError> {
        if max_stages == 0
            || max_repetitions == 0
            || max_trials_per_run == 0
            || max_total_trials == 0
        {
            return Err(StageError::InvalidPlan);
        }
        Ok(Self {
            max_stages,
            max_repetitions,
            max_trials_per_run,
            max_total_trials,
        })
    }
}

/// 段階の実行の誤り。値・名前の中身・元の誤りの文言は載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageError {
    /// 計画が不正（段階が 0 件・上限超過・繰り返しが 0 または上限超過・試行数の溢れ・区間名が語彙外）。
    InvalidPlan,
    /// 回の実行の誤り（段階・回は 1 始まり）。
    Target {
        stage: u64,
        run: u64,
        error: TargetError,
    },
    /// 段階の最悪の回の集計の誤り。
    Aggregate { stage: u64, error: AggregateError },
    /// 時計を作る関数が誤りを返した（段階・回は 1 始まり）。回を始める前の失敗で、その回の対象は呼んでいない。
    /// 誤りは中身を持たない [`MeterError`] の種類だけで、対象の名前を運ばない（SECR-23）。
    Clock {
        stage: u64,
        run: u64,
        error: MeterError,
    },
    /// 番号の計算が溢れた。
    CounterOverflow,
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidPlan => "invalid stage plan",
            Self::Target { .. } => "target error in stage",
            Self::Aggregate { .. } => "aggregate error in stage",
            Self::Clock { .. } => "clock error in stage",
            Self::CounterOverflow => "counter overflow",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for StageError {}

/// 1 段階の設定。負荷の設定と繰り返しの数。既定値は置かない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageConfig {
    load: LoadConfig,
    repetitions: u64,
}

impl StageConfig {
    /// # Errors
    /// `repetitions` が 0 のとき [`StageError::InvalidPlan`]。上限は [`StagePlan::new`] が [`StageLimits`] で検査する。
    pub fn new(load: LoadConfig, repetitions: u64) -> Result<Self, StageError> {
        if repetitions == 0 {
            return Err(StageError::InvalidPlan);
        }
        Ok(Self { load, repetitions })
    }

    pub fn load(&self) -> &LoadConfig {
        &self.load
    }

    pub fn repetitions(&self) -> u64 {
        self.repetitions
    }
}

/// 段階の並びと、最悪の回を比べる指標（明示。既定は置かない）。
#[derive(Clone, PartialEq, Eq)]
pub struct StagePlan {
    stages: Vec<StageConfig>,
    by: MetricKey,
}

/// SECR-23: 固定文字列にする（値・長さ・ハッシュも出さない）。
impl fmt::Debug for StagePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StagePlan(..)")
    }
}

impl StagePlan {
    /// 段階の並びに制約（並行数の単調増加等）は置かない。
    ///
    /// 記録に出してよい区間名の語彙は crate の固定の定数 [`RECORD_INTERVALS`] で、呼び出し元は渡せない。
    /// [`run_stages`] は対象の区間名がこの語彙に無ければ、最初の回の前に拒否する
    /// （`load.md` の 6・SECR-23）。
    ///
    /// # Errors
    /// 段階が 0 件または `limits` の段階数超、繰り返しが `limits` 超、全体の試行数の計算が溢れる、
    /// 1 回の試行数（ウォームアップ + 計測）が `limits` の予算超の段階があるとき、
    /// 全体の試行数の合計が `limits` の上限超のとき [`StageError::InvalidPlan`]。
    pub fn new(
        stages: Vec<StageConfig>,
        by: MetricKey,
        limits: StageLimits,
    ) -> Result<Self, StageError> {
        let n = u64::try_from(stages.len()).map_err(|_| StageError::InvalidPlan)?;
        if n == 0 || n > limits.max_stages {
            return Err(StageError::InvalidPlan);
        }
        let mut total: u64 = 0;
        for s in &stages {
            if s.repetitions > limits.max_repetitions {
                return Err(StageError::InvalidPlan);
            }
            let per_run = s
                .load
                .warmup()
                .checked_add(s.load.trials_planned())
                .ok_or(StageError::InvalidPlan)?;
            if per_run > limits.max_trials_per_run {
                return Err(StageError::InvalidPlan);
            }
            let t = s
                .repetitions
                .checked_mul(per_run)
                .ok_or(StageError::InvalidPlan)?;
            total = total.checked_add(t).ok_or(StageError::InvalidPlan)?;
            // 対象を呼ぶ前に計画全体の合計を検査する（全回の集計を保持するため）。
            if total > limits.max_total_trials {
                return Err(StageError::InvalidPlan);
            }
        }
        Ok(Self { stages, by })
    }

    pub fn stages(&self) -> &[StageConfig] {
        &self.stages
    }

    pub fn by(&self) -> MetricKey {
        self.by
    }
}

/// 1 段階の結果。フィールドは非公開で、読み取りだけを許す
/// （回の削除・並べ替えで `Display` の番号と `worst_run` がずれないようにする。`load.md` の 3・4）。
/// 回ごとに持つのは集計 [`RunSummary`] だけで、各試行の生標本は回の終了時に捨てる
/// （1 回の中の試行数に比例した生標本を回をまたいで持たないため）。
/// 保持する集計は全回ぶんなので、結果の保持量は段階数・繰り返し（回数）に応じて増える
/// （計画全体の上限は [`StageLimits`] の `max_total_trials`）。
#[derive(Clone, PartialEq, Eq)]
pub struct StageResult {
    stage: u64,
    config: StageConfig,
    runs: Vec<RunSummary>,
    worst_run: Option<u64>,
}

/// SECR-23: 回の集計の指標のキー（区間名）を出さない固定文字列にする。
impl fmt::Debug for StageResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StageResult(..)")
    }
}

impl StageResult {
    /// 段階の通し番号（1 始まり）。
    pub fn stage(&self) -> u64 {
        self.stage
    }

    pub fn config(&self) -> &StageConfig {
        &self.config
    }

    /// 回の集計。添字 + 1 が段階の中の回の通し番号。
    pub fn runs(&self) -> &[RunSummary] {
        &self.runs
    }

    /// 段階の中の最悪の回の通し番号（1 始まり）。比べるのは対象区間の成功標本の分布だけで、失敗の標本は比べない。
    /// どの回にも対象区間の成功標本が無いとき（全回が失敗・タイムアウトで、失敗の時間標本があっても）`None`。
    pub fn worst_run(&self) -> Option<u64> {
        self.worst_run
    }
}

/// 段階を変える実行の結果。段階ごとに分けて持つ。フィールドは非公開で読み取りだけを許す。
#[derive(Clone, PartialEq, Eq)]
pub struct StagedRun {
    target: TargetName,
    interval: IntervalName,
    by: MetricKey,
    stages: Vec<StageResult>,
}

/// SECR-23: 対象名・区間名・段階の中身を出さない固定文字列にする（値・長さ・ハッシュも出さない）。
impl fmt::Debug for StagedRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StagedRun(..)")
    }
}

impl StagedRun {
    pub fn target(&self) -> &TargetName {
        &self.target
    }

    pub fn interval(&self) -> &IntervalName {
        &self.interval
    }

    pub fn by(&self) -> MetricKey {
        self.by
    }

    pub fn stages(&self) -> &[StageResult] {
        &self.stages
    }
}

/// 段階を計画の順に 1 つずつ回す。各回の時計は `new_clock` で作る（回ごとに新しい計測を始めるため）。
///
/// `new_clock` は回ごとに `run_target` の直前に 1 度、計画の順に呼ばれる。`Err` は [`StageError::Clock`] として
/// 段階・回の番号付きで返し、以後の生成・対象の呼び出しをせず、終わった段階の結果も返さない。
/// この形（`Result` で返す・誤りの型を [`MeterError`] に固定）は仮置きで、型と variant 名は人の確認待ちである。
///
/// # Panics
/// `new_clock` の panic と、`new_clock`・作った時計の破棄の panic は捕捉せず、そのまま呼び出し元へ伝わる
/// （これらの panic のために panic hook を差し替え・抑止しない。hook はプロセス全体の共有状態で、ハーネスが握り続けることを
/// 保証できないため）。
/// アンワインド中に生成関数や時計の破棄が続けて panic すると、二重 panic でプロセスが中断し、何も伝わらない。
/// 対象の閉包の panic の文言の抑止は、[`run_target`] が使う `run_closed_loop` の既存の `install_quiet_hook` に依る。
/// これも panic hook に依るので同じ限界を持つ（呼び出し元が後から hook を差し替えた場合など）。その扱いはこの PR（#6017）の範囲外で、ここでは変えない。
/// 対象の名前を伏せることをハーネスが保証するのは、`Result` の誤りの経路（[`StageError`] の `Display`・`Debug`）だけである。
/// panic の文言に対象の名前を入れないのは生成関数の書き手の責任で、伏せたい失敗は panic ではなく `Err` で返す（SECR-23）。
///
/// # Errors
/// 計画の不正・時計の生成の誤り・回の誤り・集計の誤り。最初の誤りで止め、終わった段階の結果は返さない。
pub fn run_stages<C, F>(
    mut new_clock: C,
    spec: &TargetSpec,
    plan: &StagePlan,
    call: F,
) -> Result<StagedRun, StageError>
where
    C: FnMut() -> Result<Box<dyn MonotonicClock>, MeterError>,
    F: Fn() -> CallResult + Sync,
{
    // 語彙外の区間名は記録に出さず、最初の回の前に拒否する（固定語彙。呼び出し元は足せない）。
    if !RECORD_INTERVALS.contains(&spec.interval().as_str()) {
        return Err(StageError::InvalidPlan);
    }
    let mut stages = Vec::with_capacity(plan.stages.len());
    for (si, cfg) in plan.stages.iter().enumerate() {
        let stage = number(si)?;
        // 生標本は持たず、回ごとに集計だけを残す。
        let mut runs: Vec<RunSummary> = Vec::new();
        for ri in 0..cfg.repetitions {
            let run = ri.checked_add(1).ok_or(StageError::CounterOverflow)?;
            let clock = new_clock().map_err(|error| StageError::Clock { stage, run, error })?;
            let r = run_target(clock, spec, &cfg.load, &call)
                .map_err(|error| StageError::Target { stage, run, error })?;
            runs.push(r.summary);
        }
        let worst = worst_run(&runs, plan.by, spec.interval())
            .map_err(|error| StageError::Aggregate { stage, error })?;
        stages.push(StageResult {
            stage,
            config: *cfg,
            runs,
            worst_run: worst,
        });
    }
    Ok(StagedRun {
        target: spec.name().clone(),
        interval: spec.interval().clone(),
        by: plan.by,
        stages,
    })
}

/// 添字を 1 始まりの番号にする。
fn number(index: usize) -> Result<u64, StageError> {
    u64::try_from(index)
        .ok()
        .and_then(|v| v.checked_add(1))
        .ok_or(StageError::CounterOverflow)
}

fn opt(v: Option<u64>) -> String {
    v.map_or_else(|| "none".to_owned(), |x| x.to_string())
}

fn metric_line(
    f: &mut fmt::Formatter<'_>,
    key: &str,
    id: &str,
    interval: &IntervalName,
    s: &IntervalStats,
) -> fmt::Result {
    writeln!(
        f,
        "{key}: {id} interval={} class=measured n={} p50_ns={} p95_ns={} p99_ns={} max_ns={} max_trial={} refs=measured",
        interval.as_str(),
        s.n,
        opt(s.p50_ns),
        opt(s.p95_ns),
        opt(s.p99_ns),
        opt(s.max_ns),
        opt(s.max_trial),
    )
}

impl fmt::Display for StagedRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for st in &self.stages {
            let l = st.config.load();
            writeln!(
                f,
                "stage: {} class=test_condition refs=harness_config concurrency_configured={} repetitions={} trials_planned={} warmup={}",
                st.stage,
                l.concurrency(),
                st.config.repetitions(),
                l.trials_planned(),
                l.warmup(),
            )?;
            for (i, r) in st.runs.iter().enumerate() {
                // 回の通し番号の変換に失敗したら（繰り返しの上限は呼び出し元の StageLimits で決まる）fmt::Error を返す。
                let n = number(i).map_err(|_| fmt::Error)?;
                let id = format!("{}.{}", st.stage, n);
                let s = r;
                writeln!(
                    f,
                    "run: {id} class=measured refs=measured concurrency_observed_max={} executed={} success={} failure={} timeout={} excluded={}",
                    s.max_in_flight, s.executed, s.success, s.failure, s.timeout, s.excluded,
                )?;
                for (k, c) in &s.failure_kinds {
                    writeln!(
                        f,
                        "failure_kind: {id} class=measured refs=measured kind={} count={c}",
                        k.as_str(),
                    )?;
                }
                for (name, m) in &s.success_metrics {
                    metric_line(f, "metric", &id, name, m)?;
                }
                for (name, m) in &s.failure_metrics {
                    metric_line(f, "failure_metric", &id, name, m)?;
                }
            }
            writeln!(
                f,
                "worst_run: {} by={} interval={} run={} class=measured refs=measured",
                st.stage,
                self.by.as_str(),
                self.interval.as_str(),
                opt(st.worst_run),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::StdMonotonicClock;
    use crate::record::is_fixed_shape;

    fn new_clock() -> Result<Box<dyn MonotonicClock>, MeterError> {
        Ok(Box::new(StdMonotonicClock::new()))
    }

    fn plan() -> StagePlan {
        StagePlan::new(
            vec![StageConfig::new(LoadConfig::new(1, 1, 0).unwrap(), 1).unwrap()],
            MetricKey::P99Ns,
            StageLimits::new(4, 4, 100, 100).unwrap(),
        )
        .unwrap()
    }

    /// 64 バイトの区間名の行が形の網（語・行の上限）に当たらない（SECR-23。網は正当な入力を拒否しない）。
    /// 区間名は固定語彙でしか記録に出ないため、公開の口では作れない長さを `StagedRun` の区間名の差し替えで確かめる。
    #[test]
    fn display_with_max_length_interval_passes_shape() {
        let long = "i".repeat(64);
        let long_spec = TargetSpec::new(
            TargetName::new("route_a").unwrap(),
            IntervalName::new(&long).unwrap(),
        );
        // 語彙の外は run_stages が拒否する。
        assert_eq!(
            run_stages(new_clock, &long_spec, &plan(), || CallResult::Ok).err(),
            Some(StageError::InvalidPlan)
        );
        let ok_spec = TargetSpec::new(
            TargetName::new("route_a").unwrap(),
            IntervalName::new(RECORD_INTERVALS[0]).unwrap(),
        );
        // failure_metric の行を出すため、成功と失敗を交互に返す（2 試行）。
        let two = StagePlan::new(
            vec![StageConfig::new(LoadConfig::new(1, 2, 0).unwrap(), 1).unwrap()],
            MetricKey::P99Ns,
            StageLimits::new(4, 4, 100, 100).unwrap(),
        )
        .unwrap();
        let calls = std::sync::atomic::AtomicU64::new(0);
        let mut run = run_stages(new_clock, &ok_spec, &two, || {
            if calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                .is_multiple_of(2)
            {
                CallResult::Ok
            } else {
                CallResult::Rejected
            }
        })
        .unwrap();
        run.interval = IntervalName::new(&long).unwrap();
        // metric・failure_metric の区間名は各 RunSummary のマップのキー由来のため、同じ長さのキーへ差し替える。
        for st in &mut run.stages {
            for r in &mut st.runs {
                let key = IntervalName::new(&long).unwrap();
                r.success_metrics = std::mem::take(&mut r.success_metrics)
                    .into_values()
                    .map(|v| (key.clone(), v))
                    .collect();
                r.failure_metrics = std::mem::take(&mut r.failure_metrics)
                    .into_values()
                    .map(|v| (key.clone(), v))
                    .collect();
            }
        }
        let text = run.to_string();
        assert!(text.contains(&format!("interval={long}")));
        for key in ["metric", "failure_metric", "worst_run"] {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{key}: "))
                    && l.contains(&format!("interval={long} "))),
                "{key} line with the max-length interval is missing"
            );
        }
        assert!(is_fixed_shape(&text));
    }
}
