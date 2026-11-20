//! 対象の呼び出しの単体試験（TASK-12 6/10）。時間は偽の時計で、実際の経過・sleep・時間の値では判定しない。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fandhe_auth_load_harness::{
    CallResult, FailureKind, IntervalName, LoadConfig, MeterError, MonotonicClock, TargetError,
    TargetName, TargetSpec, run_target,
};

#[derive(Clone)]
struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
}

impl MonotonicClock for FakeClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        Ok(self.0.fetch_add(10, Ordering::SeqCst))
    }
}

/// 呼ぶたびに誤りを返す時計。
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

fn cfg(c: u64, n: u64, w: u64) -> LoadConfig {
    LoadConfig::new(c, n, w).unwrap()
}

fn kind(s: &str) -> FailureKind {
    FailureKind::new(s).unwrap()
}

#[test]
fn all_ok_counts_success_and_excludes_warmup() {
    let r = run_target(Box::new(FakeClock::new()), &spec(), &cfg(3, 12, 2), || {
        CallResult::Ok
    })
    .unwrap();
    let s = &r.run.samples;
    assert_eq!((s.executed, s.success, s.excluded), (12, 12, 2));
    assert_eq!(r.summary.success_metrics[spec().interval()].n, 12);
    assert_eq!(r.target.as_str(), "route_a");
}

#[test]
fn mixed_results_are_mapped_to_fixed_vocabulary() {
    // 通し番号で結果を決める。暖機なし。4 種が 5 回ずつ。
    let seq = AtomicU64::new(0);
    let r = run_target(
        Box::new(FakeClock::new()),
        &spec(),
        &cfg(2, 20, 0),
        || match seq.fetch_add(1, Ordering::SeqCst) % 4 {
            0 => CallResult::Ok,
            1 => CallResult::Rejected,
            2 => CallResult::Unavailable,
            _ => CallResult::TimedOut,
        },
    )
    .unwrap();
    let s = &r.run.samples;
    assert_eq!(s.success, 5);
    assert_eq!(s.failure, 10);
    assert_eq!(s.timeout, 5);
    assert_eq!(s.executed, s.success + s.failure + s.timeout);
    assert_eq!(s.failure_kinds[&kind("rejected")], 5);
    assert_eq!(s.failure_kinds[&kind("unavailable")], 5);
    // 失敗の時間は失敗の指標に入り、時間切れは標本を持たない。
    assert_eq!(r.summary.failure_metrics[spec().interval()].n, 10);
    assert_eq!(r.summary.success_metrics[spec().interval()].n, 5);
}

#[test]
fn warmup_is_excluded_regardless_of_result() {
    let r = run_target(Box::new(FakeClock::new()), &spec(), &cfg(2, 6, 4), || {
        CallResult::Rejected
    })
    .unwrap();
    let s = &r.run.samples;
    assert_eq!((s.excluded, s.executed, s.failure), (4, 6, 6));
    assert_eq!(s.success, 0);
}

#[test]
fn clock_error_is_returned_without_panic() {
    let r = run_target(Box::new(BrokenClock), &spec(), &cfg(2, 4, 0), || {
        CallResult::Ok
    });
    assert!(matches!(r, Err(TargetError::Meter(_))));
}

#[test]
fn clock_error_in_warmup_is_not_silently_counted() {
    let r = run_target(Box::new(BrokenClock), &spec(), &cfg(2, 4, 3), || {
        CallResult::Ok
    });
    assert!(matches!(r, Err(TargetError::Meter(_))));
}

/// 最初の 1 回だけ誤りを返し、その後は回復する時計。
struct FlakyClock(AtomicU64);

impl MonotonicClock for FlakyClock {
    fn now_ns(&self) -> Result<u64, MeterError> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(MeterError::ClockOverflow)
        } else {
            Ok(n * 10)
        }
    }
}

#[test]
fn transient_clock_error_in_warmup_stops_before_measured_phase() {
    let calls = AtomicU64::new(0);
    let r = run_target(
        Box::new(FlakyClock(AtomicU64::new(0))),
        &spec(),
        &cfg(1, 5, 3),
        || {
            calls.fetch_add(1, Ordering::SeqCst);
            CallResult::Ok
        },
    );
    assert!(matches!(r, Err(TargetError::Meter(_))));
    // 最初の時計の読み取りで失敗するので、対象は 1 回も呼ばれない。
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn target_name_validation() {
    for bad in ["", "Abc", "a\nb", "a\u{2028}b", "a-b", &"a".repeat(65)] {
        assert_eq!(TargetName::new(bad), Err(MeterError::InvalidName));
    }
    for ok in ["a", "a_0", &"a".repeat(64)] {
        assert!(TargetName::new(ok).is_ok());
    }
}

#[test]
fn error_display_and_debug_carry_no_canary() {
    // SECR-23: 識別用の文字列を含む無効な名前を実際の API に渡し、返された誤りに入力が出ないことを見る。
    let canary = "CANARY-zq9x";
    let e = TargetName::new(canary).unwrap_err();
    let ie = IntervalName::new(canary).unwrap_err();
    let wrapped = TargetError::from(e);
    let text = format!("{e} {e:?} {ie} {ie:?} {wrapped} {wrapped:?}");
    assert!(!text.to_lowercase().contains("canary"));
    assert!(!text.contains("zq9x"));
    assert_eq!(wrapped.to_string(), "meter error in target");
}
