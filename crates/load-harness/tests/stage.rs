//! 並行数を段階的に変える実行の単体試験（TASK-12 8/10）。時間は偽の時計で、実際の経過・sleep・時間の値では判定しない。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fandhe_auth_load_harness::{
    CallResult, IntervalName, LoadConfig, MAX_REPETITIONS, MAX_STAGES, MeterError, MetricKey,
    MonotonicClock, StageConfig, StageError, StagePlan, TargetError, TargetName, TargetSpec,
    run_stages,
};

/// 呼ぶたびに `step` ずつ進む時計。
struct StepClock {
    now: AtomicU64,
    step: u64,
}

impl MonotonicClock for StepClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        Ok(self.now.fetch_add(self.step, Ordering::SeqCst))
    }
}

fn step_clock(step: u64) -> Box<dyn MonotonicClock> {
    Box::new(StepClock {
        now: AtomicU64::new(0),
        step,
    })
}

struct BrokenClock;

impl MonotonicClock for BrokenClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        Err(MeterError::ClockOverflow)
    }
}

fn spec() -> TargetSpec {
    TargetSpec::new(
        TargetName::new("route_a").unwrap(),
        IntervalName::new("op").unwrap(),
    )
}

fn sc(c: u64, n: u64, w: u64, reps: u64) -> StageConfig {
    StageConfig::new(LoadConfig::new(c, n, w).unwrap(), reps).unwrap()
}

#[test]
fn plan_validation() {
    let one = sc(1, 1, 0, 1);
    assert_eq!(
        StagePlan::new(vec![], MetricKey::P99Ns),
        Err(StageError::InvalidPlan)
    );
    let too_many = vec![one; usize::try_from(MAX_STAGES).unwrap() + 1];
    assert_eq!(
        StagePlan::new(too_many, MetricKey::P99Ns),
        Err(StageError::InvalidPlan)
    );
    let ok = vec![one; usize::try_from(MAX_STAGES).unwrap()];
    assert!(StagePlan::new(ok, MetricKey::P99Ns).is_ok());
    let load = LoadConfig::new(1, 1, 0).unwrap();
    assert_eq!(StageConfig::new(load, 0), Err(StageError::InvalidPlan));
    assert_eq!(
        StageConfig::new(load, MAX_REPETITIONS + 1),
        Err(StageError::InvalidPlan)
    );
    assert!(StageConfig::new(load, MAX_REPETITIONS).is_ok());
    // 全体の試行数の溢れ。
    let big = StageConfig::new(LoadConfig::new(1, u64::MAX / 2, 0).unwrap(), 3).unwrap();
    assert_eq!(
        StagePlan::new(vec![big], MetricKey::P99Ns),
        Err(StageError::InvalidPlan)
    );
}

#[test]
fn results_are_kept_per_stage() {
    let cfgs = vec![sc(1, 5, 1, 2), sc(2, 6, 0, 3), sc(4, 8, 2, 1)];
    let plan = StagePlan::new(cfgs.clone(), MetricKey::P99Ns).unwrap();
    let r = run_stages(|| step_clock(10), &spec(), &plan, || CallResult::Ok).unwrap();
    assert_eq!(r.stages.len(), 3);
    assert_eq!(r.target.as_str(), "route_a");
    for (i, st) in r.stages.iter().enumerate() {
        assert_eq!(st.stage, u64::try_from(i).unwrap() + 1);
        assert_eq!(st.config, cfgs[i]);
        assert_eq!(u64::try_from(st.runs.len()).unwrap(), cfgs[i].repetitions());
        for run in &st.runs {
            let l = cfgs[i].load();
            assert_eq!(run.summary.executed, l.trials_planned());
            assert_eq!(run.summary.excluded, l.warmup());
            assert!(run.summary.max_in_flight <= l.concurrency());
        }
        assert!(st.worst_run.is_some());
    }
    assert!(
        r.stages[0]
            .runs
            .iter()
            .all(|x| x.summary.max_in_flight == 1)
    );
}

#[test]
fn stages_and_runs_do_not_overlap() {
    let cfgs = vec![sc(3, 6, 1, 2), sc(2, 4, 0, 3)];
    let plan = StagePlan::new(cfgs, MetricKey::P99Ns).unwrap();
    let generation = Arc::new(AtomicU64::new(0));
    let clock_calls = AtomicU64::new(0);
    let seen: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let g = Arc::clone(&generation);
    let r = run_stages(
        || {
            clock_calls.fetch_add(1, Ordering::SeqCst);
            g.fetch_add(1, Ordering::SeqCst);
            step_clock(10)
        },
        &spec(),
        &plan,
        || {
            seen.lock().unwrap().push(generation.load(Ordering::SeqCst));
            CallResult::Ok
        },
    );
    assert!(r.is_ok());
    assert_eq!(clock_calls.load(Ordering::SeqCst), 2 + 3);
    let seen = seen.into_inner().unwrap();
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    // 暖機の試行は閉包を呼ぶ。1 回あたり warmup + trials_planned 回。
    let count = |g: u64| u64::try_from(seen.iter().filter(|&&x| x == g).count()).unwrap();
    assert_eq!(count(1), 7);
    assert_eq!(count(2), 7);
    assert_eq!(count(3), 4);
    assert_eq!(count(4), 4);
    assert_eq!(count(5), 4);
}

#[test]
fn error_stops_and_discards_finished_stages() {
    let plan = StagePlan::new(
        vec![sc(1, 3, 0, 2), sc(1, 3, 0, 2), sc(1, 3, 0, 2)],
        MetricKey::P99Ns,
    )
    .unwrap();
    let clock_calls = AtomicU64::new(0);
    let calls = AtomicU64::new(0);
    let r = run_stages(
        || {
            let n = clock_calls.fetch_add(1, Ordering::SeqCst) + 1;
            // 3 番目（2 段階目の 1 回目）だけ壊れた時計。
            if n == 3 {
                Box::new(BrokenClock) as Box<dyn MonotonicClock>
            } else {
                step_clock(10)
            }
        },
        &spec(),
        &plan,
        || {
            calls.fetch_add(1, Ordering::SeqCst);
            CallResult::Ok
        },
    );
    assert!(matches!(
        r,
        Err(StageError::Target {
            stage: 2,
            run: 1,
            error: TargetError::Meter(_)
        })
    ));
    assert_eq!(clock_calls.load(Ordering::SeqCst), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}

#[test]
fn worst_run_picks_largest_and_smallest_on_tie() {
    let plan = StagePlan::new(vec![sc(1, 4, 0, 3)], MetricKey::P99Ns).unwrap();
    let steps = [10, 30, 30];
    let n = AtomicU64::new(0);
    let r = run_stages(
        || {
            let i = usize::try_from(n.fetch_add(1, Ordering::SeqCst)).unwrap();
            step_clock(steps[i])
        },
        &spec(),
        &plan,
        || CallResult::Ok,
    )
    .unwrap();
    assert_eq!(r.stages[0].worst_run, Some(2));
}

#[test]
fn all_timeouts_give_none_and_none_tokens() {
    let plan = StagePlan::new(vec![sc(1, 3, 0, 1)], MetricKey::P99Ns).unwrap();
    let r = run_stages(|| step_clock(10), &spec(), &plan, || CallResult::TimedOut).unwrap();
    assert_eq!(r.stages[0].worst_run, None);
    let text = r.to_string();
    assert!(
        text.contains("worst_run: 1 by=p99_ns interval=op run=none class=measured refs=measured\n")
    );
    assert!(text.contains(
        "metric: 1.1 interval=op class=measured n=0 p50_ns=none p95_ns=none p99_ns=none max_ns=none max_trial=none refs=measured\n"
    ));
}

#[test]
fn display_lines_in_order() {
    let plan = StagePlan::new(vec![sc(1, 4, 0, 1)], MetricKey::MaxNs).unwrap();
    let seq = AtomicU64::new(0);
    let r = run_stages(
        || step_clock(10),
        &spec(),
        &plan,
        || match seq.fetch_add(1, Ordering::SeqCst) {
            0 => CallResult::Ok,
            1 => CallResult::Unavailable,
            2 => CallResult::Rejected,
            _ => CallResult::TimedOut,
        },
    )
    .unwrap();
    let expected = "\
stage: 1 class=test_condition refs=harness_config concurrency_configured=1 repetitions=1 trials_planned=4 warmup=0
run: 1.1 class=measured refs=measured concurrency_observed_max=1 executed=4 success=1 failure=2 timeout=1 excluded=0
failure_kind: 1.1 class=measured refs=measured kind=rejected count=1
failure_kind: 1.1 class=measured refs=measured kind=unavailable count=1
metric: 1.1 interval=op class=measured n=1 p50_ns=10 p95_ns=10 p99_ns=10 max_ns=10 max_trial=1 refs=measured
failure_metric: 1.1 interval=op class=measured n=2 p50_ns=10 p95_ns=10 p99_ns=10 max_ns=10 max_trial=2 refs=measured
worst_run: 1 by=max_ns interval=op run=1 class=measured refs=measured
";
    assert_eq!(r.to_string(), expected);
}

#[test]
fn error_display_is_fixed_english() {
    let e = StageError::Target {
        stage: 7,
        run: 9,
        error: TargetError::Meter(MeterError::ClockOverflow),
    };
    assert_eq!(e.to_string(), "target error in stage");
    assert_eq!(StageError::InvalidPlan.to_string(), "invalid stage plan");
}
