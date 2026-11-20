//! 計測の部品の単体試験（TASK-12 3/10）。実際に経過した時間は見ず、偽の時計を手で進める。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fandhe_auth_load_harness::{
    FailureKind, IntervalName, MeterError, MonotonicClock, Outcome, RunMeter, Sample,
    StdMonotonicClock,
};

#[derive(Clone)]
struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
    fn set(&self, v: u64) {
        self.0.store(v, Ordering::SeqCst);
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

fn iv(s: &str) -> IntervalName {
    IntervalName::new(s).unwrap()
}

fn kind(s: &str) -> FailureKind {
    FailureKind::new(s).unwrap()
}

fn meter(clock: &FakeClock, names: &[&str]) -> RunMeter {
    let v: Vec<IntervalName> = names.iter().map(|n| iv(n)).collect();
    RunMeter::new(Box::new(clock.clone()), &v).unwrap()
}

#[test]
fn names_accept_valid() {
    assert!(IntervalName::new("a").is_ok());
    assert!(IntervalName::new("client_response").is_ok());
    assert!(IntervalName::new(&"a".repeat(64)).is_ok());
    assert!(FailureKind::new("bad_request_1").is_ok());
}

#[test]
fn names_reject_invalid() {
    let long = "a".repeat(65);
    let cases = [
        "",
        &long,
        "A",
        "a-b",
        "a b",
        "a\n",
        "a\r",
        "a\t",
        "a\u{2028}",
        "a\u{2029}",
        "é",
        "a\0",
    ];
    for c in cases {
        assert_eq!(IntervalName::new(c).unwrap_err(), MeterError::InvalidName);
        assert_eq!(FailureKind::new(c).unwrap_err(), MeterError::InvalidName);
    }
}

#[test]
fn interval_declaration_rules() {
    let c = FakeClock::new();
    assert_eq!(
        RunMeter::new(Box::new(c.clone()), &[]).err(),
        Some(MeterError::InvalidIntervals)
    );
    assert_eq!(
        RunMeter::new(Box::new(c), &[iv("a"), iv("a")]).err(),
        Some(MeterError::InvalidIntervals)
    );
}

#[test]
fn measures_integer_ns_and_sequential_ids() {
    let c = FakeClock::new();
    let m = meter(&c, &["a", "b"]);
    let (a, b) = (iv("a"), iv("b"));
    for i in 1..=2u64 {
        let mut t = m.begin_trial().unwrap();
        assert_eq!(t.id(), i);
        c.advance(10);
        t.start(&a).unwrap();
        c.advance(7);
        t.end(&a).unwrap();
        t.measure(&b, || c.advance(3)).unwrap();
        t.finish(Outcome::Success).unwrap();
    }
    let s = m.finish().unwrap();
    assert_eq!(
        s.success_samples[&a],
        vec![Sample { trial: 1, ns: 7 }, Sample { trial: 2, ns: 7 }]
    );
    assert_eq!(s.success_samples[&b][0], Sample { trial: 1, ns: 3 });
    assert_eq!(
        (s.executed, s.success, s.failure, s.timeout, s.excluded),
        (2, 2, 0, 0, 0)
    );
    assert_eq!(s.max_in_flight, 1);
}

#[test]
fn interval_misuse_is_error() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let a = iv("a");
    let mut t = m.begin_trial().unwrap();
    assert_eq!(t.start(&iv("zzz")), Err(MeterError::UndeclaredInterval));
    assert_eq!(t.end(&a), Err(MeterError::NotStarted));
    t.start(&a).unwrap();
    assert_eq!(t.start(&a), Err(MeterError::DuplicateStart));
    t.end(&a).unwrap();
    assert_eq!(t.end(&a), Err(MeterError::NotStarted));
    t.finish(Outcome::Success).unwrap();
}

#[test]
fn clock_reversal_is_error() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let a = iv("a");
    let mut t = m.begin_trial().unwrap();
    c.set(100);
    t.start(&a).unwrap();
    c.set(50);
    assert_eq!(t.end(&a), Err(MeterError::ClockReversed));
    drop(t);
}

#[test]
fn open_interval_cannot_finish_and_is_counted_unfinished() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let mut t = m.begin_trial().unwrap();
    t.start(&iv("a")).unwrap();
    assert_eq!(
        t.finish(Outcome::Success),
        Err(MeterError::IncompleteIntervals)
    );
    let s = m.snapshot().unwrap();
    assert_eq!((s.executed, s.failure), (1, 1));
    assert_eq!(s.failure_kinds[&kind("unfinished")], 1);
    assert!(s.success_samples.is_empty());
}

#[test]
fn success_requires_all_declared_intervals() {
    let c = FakeClock::new();
    let m = meter(&c, &["a", "b"]);
    let mut t = m.begin_trial().unwrap();
    t.measure(&iv("a"), || ()).unwrap();
    assert_eq!(
        t.finish(Outcome::Success),
        Err(MeterError::IncompleteIntervals)
    );
}

#[test]
fn outcomes_are_counted_separately() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let a = iv("a");
    let run = |o: Outcome, d: u64| {
        let mut t = m.begin_trial().unwrap();
        t.measure(&a, || c.advance(d)).unwrap();
        t.finish(o).unwrap();
    };
    run(Outcome::Success, 5);
    run(Outcome::Failure(kind("bad")), 900);
    run(Outcome::Failure(kind("bad")), 901);
    run(Outcome::Failure(kind("other")), 902);
    run(Outcome::Timeout, 999);
    run(Outcome::Excluded, 998);
    let s = m.finish().unwrap();
    assert_eq!(
        (s.executed, s.success, s.failure, s.timeout, s.excluded),
        (5, 1, 3, 1, 1)
    );
    assert_eq!(s.executed, s.success + s.failure + s.timeout);
    assert_eq!(s.failure_kinds[&kind("bad")], 2);
    assert_eq!(s.failure_kinds[&kind("other")], 1);
    assert_eq!(s.failure_kinds.values().sum::<u64>(), s.failure);
    // 失敗の時間は成功の標本に混ざらず、時間切れ・除外は標本を持たない。
    assert_eq!(s.success_samples[&a], vec![Sample { trial: 1, ns: 5 }]);
    assert_eq!(s.failure_samples[&a].len(), 3);
    assert!(
        s.failure_samples[&a]
            .iter()
            .all(|x| x.ns >= 900 && x.ns <= 902)
    );
}

#[test]
fn max_in_flight_counts_open_trials() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let ts: Vec<_> = (0..3).map(|_| m.begin_trial().unwrap()).collect();
    assert_eq!(m.snapshot().unwrap_err(), MeterError::TrialsInFlight);
    for t in ts {
        t.finish(Outcome::Excluded).unwrap();
    }
    assert_eq!(m.finish().unwrap().max_in_flight, 3);
}

#[test]
fn dropped_trial_is_not_silent() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    drop(m.begin_trial().unwrap());
    let s = m.finish().unwrap();
    assert_eq!((s.executed, s.failure), (1, 1));
    assert_eq!(s.failure_kinds[&kind("unfinished")], 1);
}

#[test]
fn concurrent_trials_keep_counts() {
    const N: usize = 8;
    const PER: usize = 50;
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let a = iv("a");
    std::thread::scope(|sc| {
        for _ in 0..N {
            sc.spawn(|| {
                for _ in 0..PER {
                    let mut t = m.begin_trial().unwrap();
                    t.measure(&a, || ()).unwrap();
                    t.finish(Outcome::Success).unwrap();
                }
            });
        }
    });
    let s = m.finish().unwrap();
    assert_eq!(s.success, (N * PER) as u64);
    assert_eq!(s.executed, s.success);
    assert!(s.max_in_flight >= 1 && s.max_in_flight <= N as u64);
    let mut ids: Vec<u64> = s.success_samples[&a].iter().map(|x| x.trial).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), N * PER);
    assert_eq!(ids[0], 1);
}

#[test]
fn timeout_midway_discards_open_interval_and_counts() {
    let c = FakeClock::new();
    let m = meter(&c, &["a", "b"]);
    let mut t = m.begin_trial().unwrap();
    t.measure(&iv("a"), || c.advance(7)).unwrap();
    t.start(&iv("b")).unwrap();
    t.finish(Outcome::Timeout).unwrap();
    let s = m.finish().unwrap();
    assert_eq!((s.executed, s.timeout, s.failure), (1, 1, 0));
    assert!(s.failure_kinds.is_empty());
    assert!(s.success_samples.is_empty() && s.failure_samples.is_empty());
}

#[test]
fn snapshot_is_consistent_under_concurrency() {
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let a = iv("a");
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|sc| {
        let worker = sc.spawn(|| {
            for _ in 0..2000 {
                let mut t = m.begin_trial().unwrap();
                t.measure(&a, || ()).unwrap();
                t.finish(Outcome::Success).unwrap();
            }
            stop.store(true, Ordering::SeqCst);
        });
        while !stop.load(Ordering::SeqCst) {
            match m.snapshot() {
                Err(e) => assert_eq!(e, MeterError::TrialsInFlight),
                // 成功のあるスナップショットは、必ず最大同時実行が 1 以上（件数だけ先に見えない）。
                Ok(s) => assert_eq!(s.executed > 0, s.max_in_flight > 0),
            }
        }
        worker.join().unwrap();
    });
    let s = m.finish().unwrap();
    assert_eq!((s.executed, s.max_in_flight), (2000, 1));
}

#[test]
fn std_clock_is_monotonic() {
    let c = StdMonotonicClock::new();
    let a = c.now_ns().unwrap();
    let b = c.now_ns().unwrap();
    assert!(b >= a);
}

#[test]
fn errors_do_not_leak_name_contents() {
    let canary = "CANARY_zzz_Q";
    let e = IntervalName::new(canary).unwrap_err();
    let shown = format!("{e} {e:?}");
    assert!(!shown.contains(canary));
    let c = FakeClock::new();
    let m = meter(&c, &["a"]);
    let mut t = m.begin_trial().unwrap();
    let e = t.start(&iv("canary_unknown")).unwrap_err();
    assert!(!format!("{e} {e:?}").contains("canary_unknown"));
}
