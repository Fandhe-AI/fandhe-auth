//! 集計の処理の単体試験（TASK-12 4/10）。期待値は `docs/harness/load.md` の最近順位法から導く。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fandhe_auth_load_harness::{
    AggregateError, FailureKind, IntervalName, MeterError, MetricKey, MonotonicClock, Outcome,
    RunMeter, RunSamples, RunSummary, Sample, summarize_interval, summarize_run, worst_run,
};

fn iv(s: &str) -> IntervalName {
    IntervalName::new(s).unwrap()
}

fn samples(vals: &[u64]) -> Vec<Sample> {
    vals.iter()
        .enumerate()
        .map(|(i, v)| Sample {
            trial: i as u64 + 1,
            ns: *v,
        })
        .collect()
}

#[test]
fn hundred_values() {
    let v: Vec<u64> = (1..=100).collect();
    let s = summarize_interval(&samples(&v)).unwrap();
    assert_eq!(s.n, 100);
    assert_eq!(s.p50_ns, Some(50));
    assert_eq!(s.p95_ns, Some(95));
    assert_eq!(s.p99_ns, Some(99));
    assert_eq!(s.max_ns, Some(100));
    assert_eq!(s.max_trial, Some(100));
}

#[test]
fn ten_values() {
    let v: Vec<u64> = (1..=10).collect();
    let s = summarize_interval(&samples(&v)).unwrap();
    assert_eq!(
        (s.p50_ns, s.p95_ns, s.p99_ns, s.max_ns),
        (Some(5), Some(10), Some(10), Some(10))
    );
}

#[test]
fn rank_boundaries() {
    let v: Vec<u64> = (1..=20).collect();
    assert_eq!(summarize_interval(&samples(&v)).unwrap().p95_ns, Some(19));
    let v: Vec<u64> = (1..=101).collect();
    let s = summarize_interval(&samples(&v)).unwrap();
    assert_eq!((s.p95_ns, s.p99_ns), (Some(96), Some(100)));
    let s = summarize_interval(&samples(&[7])).unwrap();
    assert_eq!(
        (s.p50_ns, s.p95_ns, s.p99_ns, s.max_ns),
        (Some(7), Some(7), Some(7), Some(7))
    );
}

#[test]
fn empty_is_none() {
    let s = summarize_interval(&[]).unwrap();
    assert_eq!(s.n, 0);
    assert_eq!(
        (s.p50_ns, s.p95_ns, s.p99_ns, s.max_ns, s.max_trial),
        (None, None, None, None, None)
    );
}

#[test]
fn order_independent_and_duplicates() {
    let a = samples(&[5, 1, 3, 3, 9, 2]);
    let mut b = a.clone();
    b.reverse();
    assert_eq!(summarize_interval(&a), summarize_interval(&b));
    let s = summarize_interval(&samples(&[4, 4, 4])).unwrap();
    assert_eq!(s.p50_ns, Some(4));
}

#[test]
fn max_ties_pick_smallest_trial() {
    let v = vec![
        Sample { trial: 5, ns: 10 },
        Sample { trial: 2, ns: 10 },
        Sample { trial: 1, ns: 3 },
        Sample { trial: 9, ns: 10 },
    ];
    let s = summarize_interval(&v).unwrap();
    assert_eq!(s.max_trial, Some(2));
}

#[test]
fn u64_max_no_overflow() {
    let s = summarize_interval(&samples(&[1, u64::MAX, u64::MAX - 1])).unwrap();
    assert_eq!(s.max_ns, Some(u64::MAX));
    assert!(s.p50_ns <= s.p95_ns && s.p95_ns <= s.p99_ns && s.p99_ns <= s.max_ns);
}

#[derive(Clone)]
struct FakeClock(Arc<AtomicU64>);

impl MonotonicClock for FakeClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn end_to_end_with_meter() {
    let c = FakeClock(Arc::new(AtomicU64::new(0)));
    let m = RunMeter::new(Box::new(c.clone()), &[iv("resp")]).unwrap();
    for (d, ok) in [(10u64, true), (30, true), (50, false)] {
        let mut t = m.begin_trial().unwrap();
        t.start(&iv("resp")).unwrap();
        c.0.fetch_add(d, Ordering::SeqCst);
        t.end(&iv("resp")).unwrap();
        let o = if ok {
            Outcome::Success
        } else {
            Outcome::Failure(FailureKind::new("bad").unwrap())
        };
        t.finish(o).unwrap();
    }
    let t = m.begin_trial().unwrap();
    t.finish(Outcome::Timeout).unwrap();
    let rs = m.finish().unwrap();
    let sum = summarize_run(&rs, &[iv("resp")]).unwrap();
    let s = &sum.success_metrics[&iv("resp")];
    assert_eq!((s.n, s.max_ns), (2, Some(30)));
    let f = &sum.failure_metrics[&iv("resp")];
    assert_eq!((f.n, f.max_ns), (1, Some(50)));
    assert_eq!(
        (sum.executed, sum.success, sum.failure, sum.timeout),
        (4, 2, 1, 1)
    );
    assert_eq!(sum.failure_kinds[&FailureKind::new("bad").unwrap()], 1);
    assert!(sum.max_in_flight >= 1);
}

fn run_with(success: u64, vals: &[u64]) -> RunSamples {
    let mut r = RunSamples {
        success,
        executed: success,
        ..RunSamples::default()
    };
    if !vals.is_empty() {
        r.success_samples.insert(iv("a"), samples(vals));
    }
    r
}

#[test]
fn all_timeouts_give_n0_row() {
    let r = RunSamples {
        timeout: 3,
        executed: 3,
        ..RunSamples::default()
    };
    let s = summarize_run(&r, &[iv("a")]).unwrap();
    assert_eq!(s.success_metrics[&iv("a")].n, 0);
    assert_eq!(s.success_metrics[&iv("a")].p99_ns, None);
}

#[test]
fn run_errors() {
    let r = run_with(1, &[1, 2]);
    assert_eq!(
        summarize_run(&r, &[iv("a")]).unwrap_err(),
        AggregateError::InvariantViolated
    );
    let mut r = run_with(2, &[1, 2]);
    r.executed = 5;
    assert_eq!(
        summarize_run(&r, &[iv("a")]).unwrap_err(),
        AggregateError::InvariantViolated
    );
    let r = run_with(2, &[1, 2]);
    assert_eq!(
        summarize_run(&r, &[iv("b")]).unwrap_err(),
        AggregateError::UndeclaredInterval
    );
    assert_eq!(
        summarize_run(&r, &[]).unwrap_err(),
        AggregateError::InvalidIntervals
    );
    assert_eq!(
        summarize_run(&r, &[iv("a"), iv("a")]).unwrap_err(),
        AggregateError::InvalidIntervals
    );
}

fn runs(vals: &[&[u64]]) -> Vec<RunSummary> {
    vals.iter()
        .map(|v| summarize_run(&run_with(v.len() as u64, v), &[iv("a")]).unwrap())
        .collect()
}

#[test]
fn worst_run_cases() {
    let a = iv("a");
    assert_eq!(
        worst_run(&runs(&[&[1, 2]]), MetricKey::P99Ns, &a).unwrap(),
        Some(1)
    );
    assert_eq!(
        worst_run(&runs(&[&[5], &[5]]), MetricKey::P99Ns, &a).unwrap(),
        Some(1)
    );
    // 指標で選ばれる回が変わる: 回 2 は最大が大きいが p99 は回 1 と同じ（同値は小さい回）。
    let mut with_big = vec![1u64; 99];
    with_big.push(1000);
    let r = runs(&[&[1, 1, 1, 1, 1, 1, 1, 1, 1, 1], &with_big]);
    assert_eq!(worst_run(&r, MetricKey::MaxNs, &a).unwrap(), Some(2));
    assert_eq!(worst_run(&r, MetricKey::P99Ns, &a).unwrap(), Some(1));
    // n=0 の回は候補にならない
    let r = runs(&[&[], &[3]]);
    assert_eq!(worst_run(&r, MetricKey::P99Ns, &a).unwrap(), Some(2));
    let r = runs(&[&[], &[]]);
    assert_eq!(worst_run(&r, MetricKey::P99Ns, &a).unwrap(), None);
    assert_eq!(
        worst_run(&[], MetricKey::P99Ns, &a).unwrap_err(),
        AggregateError::NoRuns
    );
    assert_eq!(
        worst_run(&r, MetricKey::P99Ns, &iv("zz")).unwrap_err(),
        AggregateError::UndeclaredInterval
    );
}

#[test]
fn metric_tokens() {
    assert_eq!(MetricKey::P50Ns.as_str(), "p50_ns");
    assert_eq!(MetricKey::P95Ns.as_str(), "p95_ns");
    assert_eq!(MetricKey::P99Ns.as_str(), "p99_ns");
    assert_eq!(MetricKey::MaxNs.as_str(), "max_ns");
}

#[test]
fn errors_do_not_leak_names() {
    let r = run_with(2, &[1, 2]);
    let e = summarize_run(&r, &[iv("canary_zzz")]).unwrap_err();
    assert!(!format!("{e} {e:?}").contains("canary"));
}
