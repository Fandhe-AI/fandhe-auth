//! 並行数を段階的に変える実行の単体試験（TASK-12 8/10）。時間は偽の時計で、実際の経過・sleep・時間の値では判定しない。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use fandhe_auth_load_harness::{
    CallResult, IntervalName, LoadConfig, MeterError, MetricKey, MonotonicClock, RECORD_INTERVALS,
    StageConfig, StageError, StageLimits, StagePlan, TargetError, TargetName, TargetSpec,
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
        IntervalName::new("client_response").unwrap(),
    )
}

fn limits() -> StageLimits {
    StageLimits::new(64, 1024, 1_000_000, u64::MAX).unwrap()
}

fn sc(c: u64, n: u64, w: u64, reps: u64) -> StageConfig {
    StageConfig::new(LoadConfig::new(c, n, w).unwrap(), reps).unwrap()
}

#[test]
fn plan_validation() {
    let one = sc(1, 1, 0, 1);
    let lim = StageLimits::new(3, 5, 12, u64::MAX).unwrap();
    let new = |v: Vec<StageConfig>, l: StageLimits| StagePlan::new(v, MetricKey::P99Ns, l);
    assert_eq!(new(vec![], lim), Err(StageError::InvalidPlan));
    assert_eq!(new(vec![one; 4], lim), Err(StageError::InvalidPlan));
    assert!(new(vec![one; 3], lim).is_ok());
    // 上限は呼び出し元の設定で決まる。
    assert!(new(vec![one; 4], StageLimits::new(4, 5, 12, u64::MAX).unwrap()).is_ok());
    let load = LoadConfig::new(1, 1, 0).unwrap();
    assert_eq!(StageConfig::new(load, 0), Err(StageError::InvalidPlan));
    assert_eq!(new(vec![sc(1, 1, 0, 6)], lim), Err(StageError::InvalidPlan));
    assert!(new(vec![sc(1, 1, 0, 5)], lim).is_ok());
    assert_eq!(StageLimits::new(0, 1, 1, 1), Err(StageError::InvalidPlan));
    assert_eq!(StageLimits::new(1, 0, 1, 1), Err(StageError::InvalidPlan));
    assert_eq!(StageLimits::new(1, 1, 0, 1), Err(StageError::InvalidPlan));
    // 全体の試行数の溢れ。
    let big = StageConfig::new(LoadConfig::new(1, u64::MAX / 2, 0).unwrap(), 3).unwrap();
    assert_eq!(
        new(
            vec![big],
            StageLimits::new(1, 3, u64::MAX, u64::MAX).unwrap()
        ),
        Err(StageError::InvalidPlan)
    );
    // 回ごとの試行数の予算（ウォームアップ + 計測）。ちょうどは通り、1 超過は拒否する。
    let cfg = vec![sc(1, 10, 2, 1)];
    assert!(new(cfg.clone(), StageLimits::new(1, 1, 12, u64::MAX).unwrap()).is_ok());
    assert_eq!(
        new(cfg, StageLimits::new(1, 1, 11, u64::MAX).unwrap()),
        Err(StageError::InvalidPlan)
    );
    // 計画全体の試行数の上限（段階 2 × 繰り返し 3 × 回 12 = 72）。ちょうどは通り、1 超過は拒否する。
    let two = vec![sc(1, 10, 2, 3), sc(1, 10, 2, 3)];
    assert!(new(two.clone(), StageLimits::new(2, 3, 12, 72).unwrap()).is_ok());
    assert_eq!(
        new(two, StageLimits::new(2, 3, 12, 71).unwrap()),
        Err(StageError::InvalidPlan)
    );
}

/// SECR-23・#6038: 固定語彙は空でなく、各要素が区間名の形を満たし、重複が無い。
#[test]
fn record_intervals_are_well_formed() {
    assert!(!RECORD_INTERVALS.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    for n in RECORD_INTERVALS {
        assert!(IntervalName::new(n).is_ok());
        assert!(seen.insert(n));
    }
}

#[test]
fn interval_outside_vocabulary_is_rejected_before_any_run() {
    let plan = StagePlan::new(vec![sc(1, 2, 0, 1)], MetricKey::P99Ns, limits()).unwrap();
    // 固定語彙の外の区間名（設定由来の値の代わり）。語彙を足す口は無い。
    let spec = TargetSpec::new(
        TargetName::new("route_a").unwrap(),
        IntervalName::new("other").unwrap(),
    );
    let calls = AtomicU64::new(0);
    let clocks = AtomicU64::new(0);
    let r = run_stages(
        || {
            clocks.fetch_add(1, Ordering::SeqCst);
            Ok(step_clock(10))
        },
        &spec,
        &plan,
        || {
            calls.fetch_add(1, Ordering::SeqCst);
            CallResult::Ok
        },
    );
    assert_eq!(r, Err(StageError::InvalidPlan));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(clocks.load(Ordering::SeqCst), 0);
}

#[test]
fn results_are_kept_per_stage() {
    let cfgs = vec![sc(1, 5, 1, 2), sc(2, 6, 0, 3), sc(4, 8, 2, 1)];
    let plan = StagePlan::new(cfgs.clone(), MetricKey::P99Ns, limits()).unwrap();
    let r = run_stages(|| Ok(step_clock(10)), &spec(), &plan, || CallResult::Ok).unwrap();
    assert_eq!(r.stages().len(), 3);
    assert_eq!(r.target().as_str(), "route_a");
    for (i, st) in r.stages().iter().enumerate() {
        assert_eq!(st.stage(), u64::try_from(i).unwrap() + 1);
        assert_eq!(*st.config(), cfgs[i]);
        assert_eq!(
            u64::try_from(st.runs().len()).unwrap(),
            cfgs[i].repetitions()
        );
        for run in st.runs() {
            let l = cfgs[i].load();
            assert_eq!(run.executed, l.trials_planned());
            assert_eq!(run.excluded, l.warmup());
            assert!(run.max_in_flight <= l.concurrency());
        }
        assert!(st.worst_run().is_some());
    }
    assert!(r.stages()[0].runs().iter().all(|x| x.max_in_flight == 1));
}

#[test]
fn stages_and_runs_do_not_overlap() {
    // 回ごとの時計を作る時点で、前の回の全試行（暖機を含む）が終了済みで、実行中の試行が 0 件であることを確かめる。
    // 回ごとの試行数（warmup + trials_planned）。段階 1 は 2 回、段階 2 は 3 回。
    let per_run: Vec<u64> = vec![7, 7, 4, 4, 4];
    let plan = StagePlan::new(
        vec![sc(3, 6, 1, 2), sc(2, 4, 0, 3)],
        MetricKey::P99Ns,
        limits(),
    )
    .unwrap();
    let in_flight = AtomicU64::new(0);
    let finished = AtomicU64::new(0);
    let clock_calls = AtomicU64::new(0);
    let violations = AtomicU64::new(0);
    let r = run_stages(
        || {
            let k = usize::try_from(clock_calls.fetch_add(1, Ordering::SeqCst)).unwrap();
            let expected_done: u64 = per_run[..k].iter().sum();
            if in_flight.load(Ordering::SeqCst) != 0
                || finished.load(Ordering::SeqCst) != expected_done
            {
                violations.fetch_add(1, Ordering::SeqCst);
            }
            Ok(step_clock(10))
        },
        &spec(),
        &plan,
        || {
            in_flight.fetch_add(1, Ordering::SeqCst);
            finished.fetch_add(1, Ordering::SeqCst);
            in_flight.fetch_sub(1, Ordering::SeqCst);
            CallResult::Ok
        },
    );
    assert!(r.is_ok());
    assert_eq!(clock_calls.load(Ordering::SeqCst), 5);
    assert_eq!(violations.load(Ordering::SeqCst), 0);
    // 最後の回の終了後も、全試行が終了済みで実行中は 0 件。
    assert_eq!(finished.load(Ordering::SeqCst), per_run.iter().sum::<u64>());
    assert_eq!(in_flight.load(Ordering::SeqCst), 0);
}

#[test]
fn error_stops_and_discards_finished_stages() {
    let plan = StagePlan::new(
        vec![sc(1, 3, 0, 2), sc(1, 3, 0, 2), sc(1, 3, 0, 2)],
        MetricKey::P99Ns,
        limits(),
    )
    .unwrap();
    let clock_calls = AtomicU64::new(0);
    let calls = AtomicU64::new(0);
    let r = run_stages(
        || {
            let n = clock_calls.fetch_add(1, Ordering::SeqCst) + 1;
            // 3 番目（2 段階目の 1 回目）だけ壊れた時計。
            if n == 3 {
                Ok(Box::new(BrokenClock) as Box<dyn MonotonicClock>)
            } else {
                Ok(step_clock(10))
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
    let plan = StagePlan::new(vec![sc(1, 4, 0, 3)], MetricKey::P99Ns, limits()).unwrap();
    let steps = [10, 30, 30];
    let n = AtomicU64::new(0);
    let r = run_stages(
        || {
            let i = usize::try_from(n.fetch_add(1, Ordering::SeqCst)).unwrap();
            Ok(step_clock(steps[i]))
        },
        &spec(),
        &plan,
        || CallResult::Ok,
    )
    .unwrap();
    assert_eq!(r.stages()[0].worst_run(), Some(2));
}

#[test]
fn all_timeouts_give_none_and_none_tokens() {
    let plan = StagePlan::new(vec![sc(1, 3, 0, 1)], MetricKey::P99Ns, limits()).unwrap();
    let r = run_stages(
        || Ok(step_clock(10)),
        &spec(),
        &plan,
        || CallResult::TimedOut,
    )
    .unwrap();
    assert_eq!(r.stages()[0].worst_run(), None);
    let text = r.to_string();
    assert!(text.contains(
        "worst_run: 1 by=p99_ns interval=client_response run=none class=measured refs=measured\n"
    ));
    assert!(text.contains(
        "metric: 1.1 interval=client_response class=measured n=0 p50_ns=none p95_ns=none p99_ns=none max_ns=none max_trial=none refs=measured\n"
    ));
}

#[test]
fn display_lines_in_order() {
    let plan = StagePlan::new(vec![sc(1, 4, 0, 1)], MetricKey::MaxNs, limits()).unwrap();
    let seq = AtomicU64::new(0);
    let r = run_stages(
        || Ok(step_clock(10)),
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
metric: 1.1 interval=client_response class=measured n=1 p50_ns=10 p95_ns=10 p99_ns=10 max_ns=10 max_trial=1 refs=measured
failure_metric: 1.1 interval=client_response class=measured n=2 p50_ns=10 p95_ns=10 p99_ns=10 max_ns=10 max_trial=2 refs=measured
worst_run: 1 by=max_ns interval=client_response run=1 class=measured refs=measured
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
    let c = StageError::Clock {
        stage: 7,
        run: 9,
        error: MeterError::ClockOverflow,
    };
    assert_eq!(c.to_string(), "clock error in stage");
}

#[test]
fn clock_factory_error_is_numbered_and_stops() {
    let plan = StagePlan::new(
        vec![sc(1, 3, 0, 3), sc(1, 3, 0, 2)],
        MetricKey::P99Ns,
        limits(),
    )
    .unwrap();
    let clocks = AtomicU64::new(0);
    let calls = AtomicU64::new(0);
    let r = run_stages(
        || {
            // 2 回目（段階 1 の 2 回目）の生成だけが誤りを返す。後ろに同じ段階の回と次の段階がある。
            if clocks.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                Err(MeterError::ClockOverflow)
            } else {
                Ok(step_clock(10))
            }
        },
        &spec(),
        &plan,
        || {
            calls.fetch_add(1, Ordering::SeqCst);
            CallResult::Ok
        },
    );
    let e = r.unwrap_err();
    assert_eq!(
        e,
        StageError::Clock {
            stage: 1,
            run: 2,
            error: MeterError::ClockOverflow
        }
    );
    assert_eq!(e.to_string(), "clock error in stage");
    assert_eq!(clocks.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

/// 生成の誤りの経路は対象の名前を運ばない。目印の対象名で回し、誤りのどの表示にも目印が出ないことを固定する（SECR-23）。
#[test]
fn clock_error_display_and_debug_hide_target_name() {
    let canary = "canary_clock_err_target_qv";
    let spec = TargetSpec::new(
        TargetName::new(canary).unwrap(),
        IntervalName::new("client_response").unwrap(),
    );
    // 目印が対象の名前まで届いていること（届いていなければ出ないのは当然になる）。
    assert_eq!(spec.name().as_str(), canary);
    let plan = StagePlan::new(vec![sc(1, 1, 0, 1)], MetricKey::P99Ns, limits()).unwrap();
    let kinds = [
        MeterError::InvalidName,
        MeterError::InvalidIntervals,
        MeterError::UndeclaredInterval,
        MeterError::DuplicateStart,
        MeterError::NotStarted,
        MeterError::ClockReversed,
        MeterError::ClockOverflow,
        MeterError::IncompleteIntervals,
        MeterError::CounterOverflow,
        MeterError::TrialsInFlight,
        MeterError::InvariantViolated,
        MeterError::StatePoisoned,
    ];
    for kind in kinds {
        let e = run_stages(|| Err(kind), &spec, &plan, || CallResult::Ok).unwrap_err();
        assert_eq!(
            e,
            StageError::Clock {
                stage: 1,
                run: 1,
                error: kind
            }
        );
        let shown = [
            e.to_string(),
            format!("{e:?}"),
            format!("{e:#?}"),
            format!("{kind}"),
        ];
        for text in &shown {
            assert!(!text.contains(canary));
        }
        assert_eq!(shown[0], "clock error in stage");
        assert_eq!(
            shown[1],
            format!("Clock {{ stage: 1, run: 1, error: {kind:?} }}")
        );
        assert!(std::error::Error::source(&e).is_none());
    }
}

/// 生成関数の panic は捕捉せず、そのまま呼び出し元へ伝わる（誤りに変えない。生成関数のために panic hook を触らない）。
/// 文言を伏せる保証は `Result` の経路だけで、panic の文言は生成関数の書き手が持つ（`run_stages` の doc）。
#[test]
fn clock_factory_panic_propagates_unchanged() {
    let plan = StagePlan::new(vec![sc(1, 3, 0, 3)], MetricKey::P99Ns, limits()).unwrap();
    let clocks = AtomicU64::new(0);
    let calls = AtomicU64::new(0);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_stages(
            || {
                // 2 回目（段階 1 の 2 回目）の生成だけが panic する。
                if clocks.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                    std::panic::panic_any(FactoryPanic);
                }
                Ok(step_clock(10))
            },
            &spec(),
            &plan,
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                CallResult::Ok
            },
        )
    }));
    // 誤りの値として返らず、生成関数の panic の値がそのまま届く。
    let payload = caught.unwrap_err();
    assert!(payload.downcast_ref::<FactoryPanic>().is_some());
    // panic の後は以後の回を始めない（1 回目の 3 件だけ）。
    assert_eq!(clocks.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

/// 生成関数が panic で投げる目印の値（文言を持たない）。
struct FactoryPanic;

/// 破棄で panic する時計が投げる目印の値（文言を持たない）。
struct ClockDropPanic;

/// 破棄で panic する時計。時計は `run_target` に渡した後、呼び出し側のスレッドで破棄される。
struct PanicOnDropClock(StepClock);

impl MonotonicClock for PanicOnDropClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        self.0.now_ns()
    }
}

impl Drop for PanicOnDropClock {
    fn drop(&mut self) {
        std::panic::panic_any(ClockDropPanic);
    }
}

/// 破棄で panic する値（文言を持たない目印を投げる）。生成関数の閉包に捕まえさせる。
struct GeneratorDropPanic;

struct PanicOnDropValue;

impl Drop for PanicOnDropValue {
    fn drop(&mut self) {
        std::panic::panic_any(GeneratorDropPanic);
    }
}

/// 生成関数が所有する値の破棄の panic も捕捉せず、誤りに変わらずにその値のまま伝わる。
#[test]
fn generator_drop_panic_propagates_unchanged() {
    let plan = StagePlan::new(vec![sc(1, 3, 0, 2)], MetricKey::P99Ns, limits()).unwrap();
    let clocks = AtomicU64::new(0);
    let calls = AtomicU64::new(0);
    let held = PanicOnDropValue;
    let made = &clocks;
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_stages(
            move || {
                let _keep = &held;
                made.fetch_add(1, Ordering::SeqCst);
                Ok(step_clock(10))
            },
            &spec(),
            &plan,
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                CallResult::Ok
            },
        )
    }));
    let payload = caught.unwrap_err();
    assert!(payload.downcast_ref::<GeneratorDropPanic>().is_some());
    // 全回を回した後の破棄で起きる（実行の途中では止めていない）。
    assert_eq!(clocks.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}

/// 作った時計の破棄の panic も捕捉せず、誤り（`WorkerPanicked` 等）に変わらずにそのまま伝わる。
/// 並行数 1 と 2 以上の両方で確かめる（ワーカーの中で破棄されないこと）。
#[test]
fn clock_drop_panic_propagates_unchanged() {
    for concurrency in [1_u64, 3] {
        let plan =
            StagePlan::new(vec![sc(concurrency, 6, 1, 2)], MetricKey::P99Ns, limits()).unwrap();
        let clocks = AtomicU64::new(0);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_stages(
                || {
                    clocks.fetch_add(1, Ordering::SeqCst);
                    Ok(Box::new(PanicOnDropClock(StepClock {
                        now: AtomicU64::new(0),
                        step: 10,
                    })) as Box<dyn MonotonicClock>)
                },
                &spec(),
                &plan,
                || CallResult::Ok,
            )
        }));
        let payload = caught.unwrap_err();
        assert!(payload.downcast_ref::<ClockDropPanic>().is_some());
        // 1 回目の時計の破棄で止まり、2 回目の生成をしない。
        assert_eq!(clocks.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn staged_run_debug_hides_target_name() {
    // SECR-23: 対象名に目印を渡し、Debug の出力に含まれないことを確かめる。
    let canary = "canary_target_zq";
    let spec = TargetSpec::new(
        TargetName::new(canary).unwrap(),
        IntervalName::new("client_response").unwrap(),
    );
    let plan = StagePlan::new(vec![sc(1, 1, 0, 1)], MetricKey::P99Ns, limits()).unwrap();
    let run = run_stages(|| Ok(step_clock(10)), &spec, &plan, || CallResult::Ok).unwrap();
    let out = format!("{run:?}");
    assert!(!out.contains(canary));
    assert_eq!(out, "StagedRun(..)");
    // 目印が結果の対象名まで届いていることを確かめた上で、その Debug が名前を出さないことを固定する。
    assert_eq!(run.target().as_str(), canary);
    let target_out = format!("{:?}", run.target());
    assert!(!target_out.contains(canary));
    assert_eq!(target_out, "TargetName(..)");
}

#[test]
fn plan_and_result_debug_hide_interval_names() {
    // SECR-23: 区間名に目印を渡し、計画・結果（回の集計のキーを含む）の Debug に含まれないことを確かめる。
    // 目印は語彙の区間名（語彙の外は run_stages が拒否するため、語彙の中の名前で Debug の秘匿を見る）。
    let canary = "client_response";
    let name = IntervalName::new(canary).unwrap();
    let spec = TargetSpec::new(TargetName::new("op_target").unwrap(), name.clone());
    let plan = StagePlan::new(vec![sc(1, 1, 0, 1)], MetricKey::P99Ns, limits()).unwrap();
    let plan_out = format!("{plan:?}");
    assert!(!plan_out.contains(canary));
    assert_eq!(plan_out, "StagePlan(..)");
    let run = run_stages(|| Ok(step_clock(10)), &spec, &plan, || CallResult::Ok).unwrap();
    let res_out = format!("{:?}", run.stages()[0]);
    assert!(!res_out.contains(canary));
    assert_eq!(res_out, "StageResult(..)");
    // 公開アクセサーの戻り値（回の集計のキー・区間名・対象名）の Debug にも出ない。
    let acc = format!(
        "{:?} {:?} {:?}",
        run.stages()[0].runs(),
        run.interval(),
        run.target()
    );
    assert!(!acc.contains(canary));
}

#[test]
fn multi_stage_display_numbers_runs_per_stage_and_worst_run_stays_in_stage() {
    // load.md の 3・4: run の番号は「段階.回」で段階ごとに 1 から始まり、worst_run は同じ段階内の回を指す。
    let plan = StagePlan::new(
        vec![sc(1, 2, 0, 3), sc(1, 3, 0, 2)],
        MetricKey::MaxNs,
        limits(),
    )
    .unwrap();
    // 回の実行順（段階 1 の 3 回、段階 2 の 2 回）に対応する時計の刻み。最大が段階 1 は 2 回目、段階 2 は 1 回目。
    let steps = [10, 30, 20, 50, 40];
    let n = AtomicU64::new(0);
    let r = run_stages(
        || {
            let i = usize::try_from(n.fetch_add(1, Ordering::SeqCst)).unwrap();
            Ok(step_clock(steps[i]))
        },
        &spec(),
        &plan,
        || CallResult::Ok,
    )
    .unwrap();
    let text = r.to_string();
    let heads: Vec<String> = text
        .lines()
        .filter(|l| l.starts_with("stage: ") || l.starts_with("run: "))
        .map(|l| l.split(' ').take(2).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        heads,
        [
            "stage: 1", "run: 1.1", "run: 1.2", "run: 1.3", "stage: 2", "run: 2.1", "run: 2.2"
        ]
    );
    let worst: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("worst_run: "))
        .collect();
    assert_eq!(
        worst,
        [
            "worst_run: 1 by=max_ns interval=client_response run=2 class=measured refs=measured",
            "worst_run: 2 by=max_ns interval=client_response run=1 class=measured refs=measured",
        ]
    );
    // 第 2 段階の metric 行は第 2 段階の番号で出る。
    assert!(text.contains("metric: 2.1 interval=client_response class=measured n=3 "));
    assert!(text.contains("metric: 2.2 interval=client_response class=measured n=3 "));
}

/// 同時に実行中の呼び出し数の最大値を記録する（共有ロックを持たない対象。重なりの観測だけに使う）。
struct Overlap {
    in_flight: AtomicU64,
    max: AtomicU64,
}

impl Overlap {
    fn enter(&self) {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(now, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn concurrency_one_stage_never_overlaps() {
    // 並行数 1 の段階では呼び出しが重ならない（記録した同時実行数の最大が 1）。
    let plan = StagePlan::new(vec![sc(1, 20, 2, 2)], MetricKey::P99Ns, limits()).unwrap();
    let ov = Overlap {
        in_flight: AtomicU64::new(0),
        max: AtomicU64::new(0),
    };
    let r = run_stages(
        || Ok(step_clock(10)),
        &spec(),
        &plan,
        || {
            ov.enter();
            ov.leave();
            CallResult::Ok
        },
    )
    .unwrap();
    assert_eq!(ov.max.load(Ordering::SeqCst), 1);
    assert!(r.stages()[0].runs().iter().all(|x| x.max_in_flight == 1));
}

#[test]
fn multi_worker_stages_really_overlap() {
    // 並行数 4・3 の段階では、各回の最初の「並行数」件の呼び出しが待ち合わせて全員そろうまで抜けない。
    // 全員が実際に同時に実行中になった場合だけ待ち合わせが成立する（sleep に頼らず重なりを確定させる）。
    // 直列の回帰では待ち合わせが成立せず、上限の時間（失敗の検出だけに使う。成功の経路は時間に依らない）で打ち切る。
    // 回の通し番号 k から段階の添字への対応（各段階 2 回ずつ）。
    let run_stage = [0_usize, 0, 1, 1];
    let parties = [4_u64, 3];
    let plan = StagePlan::new(
        vec![sc(4, 8, 0, 2), sc(3, 6, 0, 2)],
        MetricKey::P99Ns,
        limits(),
    )
    .unwrap();
    // (到着数, 今の回の待ち合わせ人数)
    let gate = (Mutex::new((0_u64, 0_u64)), Condvar::new());
    let timed_out = AtomicBool::new(false);
    let ov = Overlap {
        in_flight: AtomicU64::new(0),
        max: AtomicU64::new(0),
    };
    // 対象呼び出し側で観測した最大同時実行数を段階ごとに持つ（各段階は 2 回ずつ回す）。
    let per_stage_max = [AtomicU64::new(0), AtomicU64::new(0)];
    let current_stage = AtomicU64::new(0);
    let runs_started = AtomicU64::new(0);
    let r = run_stages(
        || {
            let k = usize::try_from(runs_started.fetch_add(1, Ordering::SeqCst)).unwrap();
            *gate.0.lock().unwrap() = (0, parties[run_stage[k]]);
            current_stage.store(u64::try_from(run_stage[k]).unwrap(), Ordering::SeqCst);
            Ok(step_clock(10))
        },
        &spec(),
        &plan,
        || {
            ov.enter();
            let idx = usize::try_from(current_stage.load(Ordering::SeqCst)).unwrap();
            per_stage_max[idx].fetch_max(ov.in_flight.load(Ordering::SeqCst), Ordering::SeqCst);
            let mut g = gate.0.lock().unwrap();
            // 最初のタイムアウトで失敗を記録した後は待たない（直列の回帰で 60 秒を何度も待たないため）。
            if g.0 < g.1 && !timed_out.load(Ordering::SeqCst) {
                g.0 += 1;
                if g.0 >= g.1 {
                    gate.1.notify_all();
                } else {
                    let (_g, res) = gate
                        .1
                        .wait_timeout_while(g, Duration::from_secs(60), |s| {
                            s.0 < s.1 && !timed_out.load(Ordering::SeqCst)
                        })
                        .unwrap();
                    if res.timed_out() {
                        timed_out.store(true, Ordering::SeqCst);
                        // 待機中の全員を起こす（判定の条件は変えない）。
                        gate.1.notify_all();
                    }
                }
            }
            ov.leave();
            CallResult::Ok
        },
    )
    .unwrap();
    assert!(!timed_out.load(Ordering::SeqCst));
    assert_eq!(runs_started.load(Ordering::SeqCst), 4);
    assert_eq!(ov.max.load(Ordering::SeqCst), 4);
    // 段階ごとの対象呼び出し側の最大同時実行数（全体の最大だけでは第 2 段階の上限を検証できない）。
    let target_side: Vec<u64> = per_stage_max
        .iter()
        .map(|m| m.load(Ordering::SeqCst))
        .collect();
    assert_eq!(target_side, [4, 3]);
    let observed: Vec<Vec<u64>> = r
        .stages()
        .iter()
        .map(|s| s.runs().iter().map(|x| x.max_in_flight).collect())
        .collect();
    assert_eq!(observed, [[4, 4], [3, 3]]);
}
