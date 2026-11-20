//! 負荷の生成の単体試験（TASK-12 5/10）。時間は偽の時計を手で進め、実際の経過・sleep・時間の値では判定しない。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fandhe_auth_load_harness::{
    FailureKind, IntervalName, LoadConfig, LoadError, LoopModel, MAX_CONCURRENCY, MeterError,
    MonotonicClock, Outcome, RunMeter, run_closed_loop, summarize_run,
};

#[derive(Clone)]
struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
    fn advance(&self, d: u64) {
        self.0.fetch_add(d, Ordering::SeqCst);
    }
}

impl MonotonicClock for FakeClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

/// 期限付きの待ち合わせ。ワーカーの起動に失敗して参加者が揃わなくても、期限で戻って試験を終えられる
/// （判定は件数・最大同時実行数の断定で行い、期限の長さでは行わない）。
struct Rendezvous {
    arrived: AtomicU64,
    parties: u64,
}

impl Rendezvous {
    fn new(parties: u64) -> Self {
        Self {
            arrived: AtomicU64::new(0),
            parties,
        }
    }
    fn wait(&self) {
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.arrived.load(Ordering::SeqCst) < self.parties && Instant::now() < deadline {
            std::thread::yield_now();
        }
    }
}

fn iv() -> IntervalName {
    IntervalName::new("op").unwrap()
}

fn meter(clock: &FakeClock) -> RunMeter {
    RunMeter::new(Box::new(clock.clone()), &[iv()]).unwrap()
}

fn cfg(c: u64, n: u64, w: u64) -> LoadConfig {
    LoadConfig::new(c, n, w).unwrap()
}

#[test]
fn config_rejects_invalid() {
    for (c, n, w) in [
        (0, 1, 0),
        (1, 0, 0),
        (MAX_CONCURRENCY + 1, 1, 0),
        (1, u64::MAX, 1),
    ] {
        assert_eq!(LoadConfig::new(c, n, w), Err(LoadError::InvalidConfig));
    }
}

#[test]
fn config_accepts_boundaries() {
    assert!(LoadConfig::new(1, 1, 0).is_ok());
    let c = LoadConfig::new(MAX_CONCURRENCY, 5, 2).unwrap();
    assert_eq!(
        (c.concurrency(), c.trials_planned(), c.warmup()),
        (MAX_CONCURRENCY, 5, 2)
    );
}

#[test]
fn serial_run_counts_and_summarizes() {
    let clock = FakeClock::new();
    let name = iv();
    let run = run_closed_loop(meter(&clock), &cfg(1, 100, 0), |t| {
        t.measure(&name, || clock.advance(10)).unwrap();
        Outcome::Success
    })
    .unwrap();
    assert_eq!(run.loop_model, LoopModel::Closed);
    assert_eq!(run.loop_model.as_str(), "closed");
    let s = &run.samples;
    assert_eq!((s.executed, s.success, s.max_in_flight), (100, 100, 1));
    assert_eq!(s.success_samples[&name].len(), 100);
    let sum = summarize_run(s, std::slice::from_ref(&name)).unwrap();
    assert_eq!(sum.success_metrics[&name].n, 100);
}

#[test]
fn reaches_configured_concurrency() {
    let clock = FakeClock::new();
    let name = iv();
    let barrier = Rendezvous::new(4);
    let run = run_closed_loop(meter(&clock), &cfg(4, 4, 0), |t| {
        t.measure(&name, || {
            barrier.wait();
        })
        .unwrap();
        Outcome::Success
    })
    .unwrap();
    assert_eq!(run.samples.max_in_flight, 4);
}

#[test]
fn never_exceeds_limits() {
    for (c, n) in [(8, 3), (4, 200)] {
        let clock = FakeClock::new();
        let name = iv();
        let run = run_closed_loop(meter(&clock), &cfg(c, n, 0), |t| {
            t.measure(&name, || clock.advance(1)).unwrap();
            Outcome::Success
        })
        .unwrap();
        assert!(run.samples.max_in_flight <= c.min(n));
        assert_eq!(run.samples.executed, n);
    }
}

#[test]
fn warmup_is_excluded() {
    let clock = FakeClock::new();
    let name = iv();
    let run = run_closed_loop(meter(&clock), &cfg(2, 10, 3), |t| {
        t.measure(&name, || clock.advance(1)).unwrap();
        Outcome::Success
    })
    .unwrap();
    let s = &run.samples;
    assert_eq!((s.excluded, s.executed, s.success), (3, 10, 10));
    assert_eq!(s.success_samples[&name].len(), 10);
    assert!(s.success_samples[&name].iter().all(|x| x.trial >= 4));
}

#[test]
fn warmup_timeout_with_open_interval_is_excluded() {
    let clock = FakeClock::new();
    let name = iv();
    let run = run_closed_loop(meter(&clock), &cfg(2, 4, 3), |t| {
        if t.id() <= 3 {
            t.start(&name).unwrap();
            Outcome::Timeout
        } else {
            t.measure(&name, || clock.advance(1)).unwrap();
            Outcome::Success
        }
    })
    .unwrap();
    let s = &run.samples;
    assert_eq!((s.excluded, s.executed, s.timeout, s.success), (3, 4, 0, 4));
}

#[test]
fn outcome_kinds_are_counted() {
    let clock = FakeClock::new();
    let name = iv();
    let bad = FailureKind::new("bad").unwrap();
    let run = run_closed_loop(meter(&clock), &cfg(3, 30, 0), |t| {
        t.measure(&name, || clock.advance(1)).unwrap();
        match t.id() % 3 {
            0 => Outcome::Failure(bad.clone()),
            1 => Outcome::Timeout,
            _ => Outcome::Success,
        }
    })
    .unwrap();
    let s = &run.samples;
    // 通し番号は暖機なしで 1..=30。3 で割った余りごとに 10 件ずつ。
    assert_eq!((s.success, s.failure, s.timeout), (10, 10, 10));
    assert_eq!(s.failure_kinds[&bad], 10);
    assert_eq!(s.executed, s.success + s.failure + s.timeout);
}

#[test]
fn excluded_in_measured_phase_is_rejected() {
    let clock = FakeClock::new();
    let calls = AtomicU64::new(0);
    let r = run_closed_loop(meter(&clock), &cfg(1, 5, 0), |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Outcome::Excluded
    });
    assert_eq!(r.unwrap_err(), LoadError::InvalidOutcome);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// #5984（仮置き。ACPT-7）: 並行数ぶんの試行が同時に走っている間の誤りでは、走り切る試行は並行数ぶんだけで、
/// 新しい試行は始まらない。全ワーカーが誤りで抜けるので件数は決まる。
#[test]
fn all_in_flight_failures_stop_without_new_trials() {
    const C: u64 = 4;
    let clock = FakeClock::new();
    let calls = AtomicU64::new(0);
    let barrier = Rendezvous::new(C);
    let r = run_closed_loop(meter(&clock), &cfg(C, 100, 0), |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        barrier.wait();
        Outcome::Excluded
    });
    assert_eq!(r.unwrap_err(), LoadError::InvalidOutcome);
    assert_eq!(calls.load(Ordering::SeqCst), C);
}

#[test]
fn unmeasured_success_is_rejected() {
    let clock = FakeClock::new();
    let calls = AtomicU64::new(0);
    let r = run_closed_loop(meter(&clock), &cfg(1, 5, 0), |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Outcome::Success
    });
    assert_eq!(
        r.unwrap_err(),
        LoadError::Meter(MeterError::IncompleteIntervals)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn panic_is_reported() {
    let clock = FakeClock::new();
    let calls = AtomicU64::new(0);
    let r = run_closed_loop(meter(&clock), &cfg(1, 5, 0), |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        panic!("canary-panic-message");
    });
    assert_eq!(r.unwrap_err(), LoadError::WorkerPanicked);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn error_text_is_fixed() {
    let cases = [
        (LoadError::InvalidConfig, "invalid load config"),
        (
            LoadError::InvalidOutcome,
            "target returned excluded outcome",
        ),
        (LoadError::Meter(MeterError::ClockReversed), "meter error"),
        (LoadError::SpawnFailed, "worker spawn failed"),
        (LoadError::WorkerPanicked, "worker panicked"),
        (LoadError::CountMismatch, "trial count mismatch"),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
    }
    assert_eq!(format!("{:?}", LoadError::WorkerPanicked), "WorkerPanicked");
}
