//! TASK-13 (6/8): 時計のずれの注入の試験。Docker・ネットワーク・sleep なし。
//! 基準の時刻は固定の仮の値で、ずれの数値も任意の小さな値（THRT-29・A-2026-10-28-1）。
//! 記録は事実の出力で合否の判定ではない。AVLB-31 の判定そのものは確かめない（ACPT-7）。
//! 根拠: AVLB-31・CLM-20・OCL-26・SESS-18・SESS-19・SECR-23。

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{
    ClockSkewError, ClockSkewInjector, FaultRecordError, HubStubFaultError, HubStubFaultInjector,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{FaultRecord, FaultTimeline};

const BASE_SECS: u64 = 1_000_000;

fn injector() -> ClockSkewInjector {
    ClockSkewInjector::with_base_clock(|| UNIX_EPOCH + Duration::from_secs(BASE_SECS))
}

fn timeline() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), SystemTime::now()).unwrap()
}

fn finish(inj: &mut ClockSkewInjector, tl: FaultTimeline) -> FaultRecord {
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    inj.finish(tl, at).unwrap()
}

fn injection_lines(r: &FaultRecord) -> Vec<String> {
    r.to_string()
        .lines()
        .filter(|l| l.starts_with("injection:"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn apply_and_release_shift_the_clock_and_record() {
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    let base = UNIX_EPOCH + Duration::from_secs(BASE_SECS);
    assert_eq!(clock.now(), Ok(base));
    inj.apply(&mut tl, 1_500).unwrap();
    assert_eq!(inj.active_skew_ms(), Some(1_500));
    assert_eq!(clock.now(), Ok(base + Duration::from_millis(1_500)));
    assert_eq!(clock.unix_seconds(), Ok(BASE_SECS + 1));
    inj.release(&mut tl).unwrap();
    assert_eq!(clock.now(), Ok(base));
    assert_eq!(inj.active_skew_ms(), None);
    let r = finish(&mut inj, tl);
    assert_eq!(r.unreleased_count(), 0);
    let lines = injection_lines(&r);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains(" applied clock_skew target=instance "));
    assert!(lines[0].ends_with(" skew_ms=+1500"));
    assert!(lines[1].contains(" released clock_skew "));
    assert!(lines[1].ends_with(" skew_ms=+1500"));
}

#[test]
fn negative_skew_floors_unix_seconds() {
    let mut inj = injector();
    let mut tl = timeline();
    inj.apply(&mut tl, -1_500).unwrap();
    // 1_000_000 s - 1.5 s = 999_998.5 s -> 切り捨てで 999_998。
    assert_eq!(inj.clock().unix_seconds(), Ok(BASE_SECS - 2));
    let r = {
        inj.release(&mut tl).unwrap();
        finish(&mut inj, tl)
    };
    assert!(injection_lines(&r)[0].ends_with(" skew_ms=-1500"));
}

#[test]
fn step_records_release_then_apply() {
    let mut inj = injector();
    let mut tl = timeline();
    inj.apply(&mut tl, 1_000).unwrap();
    inj.step(&mut tl, 3_000).unwrap();
    assert_eq!(inj.clock().skew_ms(), 3_000);
    assert_eq!(inj.step(&mut tl, 3_000), Err(ClockSkewError::InvalidParam));
    assert_eq!(inj.step(&mut tl, 0), Err(ClockSkewError::InvalidParam));
    inj.release(&mut tl).unwrap();
    let lines = injection_lines(&finish(&mut inj, tl));
    let kinds: Vec<_> = lines
        .iter()
        .map(|l| {
            let phase = if l.contains(" applied ") { "A" } else { "R" };
            let skew = l.rsplit(' ').next().unwrap().to_owned();
            format!("{phase}{skew}")
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "Askew_ms=+1000",
            "Rskew_ms=+1000",
            "Askew_ms=+3000",
            "Rskew_ms=+3000"
        ]
    );
}

#[test]
fn baseline_rejects_and_leaves_clock_untouched() {
    let mut inj = injector();
    let mut tl = FaultTimeline::baseline(Instant::now(), SystemTime::now()).unwrap();
    assert_eq!(
        inj.apply(&mut tl, 1_000),
        Err(ClockSkewError::Record(FaultRecordError::EventsInBaseline))
    );
    assert_eq!(inj.active_skew_ms(), None);
    assert_eq!(inj.clock().skew_ms(), 0);
}

#[test]
fn state_errors_record_nothing() {
    let mut inj = injector();
    let mut tl = timeline();
    assert_eq!(inj.release(&mut tl), Err(ClockSkewError::NotActive));
    assert_eq!(inj.step(&mut tl, 5), Err(ClockSkewError::NotActive));
    assert_eq!(inj.apply(&mut tl, 0), Err(ClockSkewError::InvalidParam));
    inj.apply(&mut tl, 10).unwrap();
    assert_eq!(inj.apply(&mut tl, 20), Err(ClockSkewError::AlreadyActive));
    assert_eq!(inj.clock().skew_ms(), 10);
    inj.release(&mut tl).unwrap();
    assert_eq!(injection_lines(&finish(&mut inj, tl)).len(), 2);
}

#[test]
fn out_of_range_records_nothing() {
    let mut inj = ClockSkewInjector::with_base_clock(|| UNIX_EPOCH + Duration::from_secs(5));
    let mut tl = timeline();
    assert_eq!(inj.apply(&mut tl, -6_000), Err(ClockSkewError::OutOfRange));
    assert_eq!(
        inj.apply(&mut tl, i64::MIN),
        Err(ClockSkewError::OutOfRange)
    );
    assert_eq!(inj.clock().skew_ms(), 0);
    // 記録が 0 件のまま確定すると fault の実行は拒否される。
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    assert!(tl.finish(at).is_err());
}

#[test]
fn unreleased_skew_is_counted_and_clones_share_state() {
    let mut inj = injector();
    let mut tl = timeline();
    let a = inj.clock();
    let b = a.clone();
    inj.apply(&mut tl, 2_000).unwrap();
    let h = std::thread::spawn(move || b.now().unwrap());
    let seen = h.join().unwrap();
    assert_eq!(seen, UNIX_EPOCH + Duration::from_secs(BASE_SECS + 2));
    // 別のインスタンスの時計は影響を受けない（AVLB-31 の「1 つのインスタンス」の形）。
    let other = injector().clock();
    assert_eq!(
        a.now().unwrap().duration_since(other.now().unwrap()).ok(),
        Some(Duration::from_secs(2))
    );
    assert_eq!(finish(&mut inj, tl).unreleased_count(), 1);
}

#[test]
fn debug_and_error_do_not_leak_values() {
    let mut inj = injector();
    let mut tl = timeline();
    inj.apply(&mut tl, 987_654).unwrap();
    let s = format!("{inj:?} {:?}", inj.clock());
    assert!(!s.contains("987654") && !s.contains(&BASE_SECS.to_string()));
    for e in [
        ClockSkewError::InvalidParam,
        ClockSkewError::OutOfRange,
        ClockSkewError::Record(FaultRecordError::EventsInBaseline),
    ] {
        let t = format!("{e} {e:?}");
        assert!(!t.contains("987654"));
        assert!(format!("{e}").starts_with("clock skew fault error: "));
    }
}

#[test]
fn drop_without_finish_clears_shared_skew_and_keeps_unreleased_fact() {
    // AVLB-31: 終了操作を通らずに注入器を破棄しても、配布済みの時計がずれたまま残らない。
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    inj.apply(&mut tl, 1_000).unwrap();
    assert_eq!(clock.skew_ms(), 1_000);
    drop(inj);
    assert_eq!(clock.skew_ms(), 0);
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    // 実行は時計の注入器を結んだままなので、通常の finish は拒否される（記録を確定するのは注入器の経路）。
    assert_eq!(
        tl.finish(at).unwrap_err().error,
        FaultRecordError::ClockInjectorBound
    );
}

#[test]
fn abandon_clears_shared_skew_without_recording_release() {
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    inj.apply(&mut tl, -2_000).unwrap();
    inj.abandon();
    assert_eq!(clock.skew_ms(), 0);
    assert_eq!(inj.active_skew_ms(), None);
    assert_eq!(inj.release(&mut tl), Err(ClockSkewError::NotActive));
    // 終端操作: 同じ実行への再適用は受けない（古い Applied が未解除のまま残るため）。
    assert_eq!(inj.apply(&mut tl, -2_000), Err(ClockSkewError::Abandoned));
    assert_eq!(clock.skew_ms(), 0);
    let r = finish(&mut inj, tl);
    assert_eq!(r.unreleased_count(), 1);
    assert_eq!(injection_lines(&r).len(), 1);
}

#[test]
fn rejected_apply_leaves_no_binding() {
    // 基準線への適用が拒否されても、注入器は別の実行へ結べる（RunMismatch にならない）。
    let mut inj = injector();
    let mut base = FaultTimeline::baseline(Instant::now(), SystemTime::now()).unwrap();
    assert!(inj.apply(&mut base, 1_000).is_err());
    let mut tl = timeline();
    inj.apply(&mut tl, 1_000).unwrap();
    inj.release(&mut tl).unwrap();
    // offset が逆行して Applied の記録が拒否された実行へは、別の注入器が AlreadyBound にならず試せる。
    let mut tl2 = timeline();
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    tl2.record_at(
        fandhe_auth_it_harness::fault::FaultKind::InstanceSigterm,
        fandhe_auth_it_harness::fault::FaultPhase::Applied,
        at,
        u64::MAX / 2,
    )
    .unwrap();
    let mut a = injector();
    let mut b = injector();
    assert_eq!(
        a.apply(&mut tl2, 1_000),
        Err(ClockSkewError::Record(
            FaultRecordError::OffsetWentBackwards
        ))
    );
    assert_eq!(
        b.apply(&mut tl2, 1_000),
        Err(ClockSkewError::Record(
            FaultRecordError::OffsetWentBackwards
        ))
    );
}

#[test]
fn injector_finish_confirms_record_and_clears_shared_skew() {
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    inj.apply(&mut tl, 1_000).unwrap();
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    let r = inj.finish(tl, at).unwrap();
    assert_eq!(r.unreleased_count(), 1);
    assert_eq!(clock.skew_ms(), 0);
    assert_eq!(inj.active_skew_ms(), None);
}

#[test]
fn plain_timeline_finish_is_rejected_for_clock_bound_run() {
    // AVLB-31: 時計だけの実行を、時計を戻さない経路で確定させない。失敗してもタイムラインを失わない。
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    inj.apply(&mut tl, 1_000).unwrap();
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    let failure = tl.finish(at).unwrap_err();
    assert_eq!(failure.error, FaultRecordError::ClockInjectorBound);
    assert_eq!(clock.skew_ms(), 1_000);
    let r = inj.finish(failure.timeline, at).unwrap();
    assert_eq!(r.unreleased_count(), 1);
    assert_eq!(clock.skew_ms(), 0);
}

#[test]
fn finish_with_other_run_timeline_returns_it_and_keeps_skew() {
    let mut a = injector();
    let mut b = injector();
    let mut tl_a = timeline();
    let mut tl_b = timeline();
    let clock_b = b.clock();
    a.apply(&mut tl_a, 1_000).unwrap();
    b.apply(&mut tl_b, 2_000).unwrap();
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    // A に B の実行のタイムラインを渡すと拒否され、タイムラインが戻る。B の時計は触られない。
    let failure = a.finish(tl_b, at).unwrap_err();
    assert_eq!(failure.error, FaultRecordError::RunMismatch);
    assert_eq!(clock_b.skew_ms(), 2_000);
    // 戻ったタイムラインで B が正しく確定できる。
    let r = b.finish(failure.timeline, at).unwrap();
    assert_eq!(r.unreleased_count(), 1);
    assert_eq!(clock_b.skew_ms(), 0);
    // 時計を結んでいない注入器でも、時計を結んだ実行は確定させない。
    let mut c = injector();
    let failure = c.finish(tl_a, at).unwrap_err();
    assert_eq!(failure.error, FaultRecordError::ClockInjectorBound);
}

#[test]
fn run_with_both_injectors_finishes_record_and_resets_clock_together() {
    // AVLB-31: 両方を結んだ実行は、どちらか片方の finish では確定せず、`finish_with_clock` で記録とリセットを一緒に行う。
    let mut inj = injector();
    let mut tl = timeline();
    let clock = inj.clock();
    let hub = HubStubFaultInjector::new(|_: Duration| {}, &tl).unwrap();
    inj.apply(&mut tl, 1_000).unwrap();
    let at = UtcTimestamp::from_system_time(SystemTime::now()).unwrap();
    let failure = inj.finish(tl, at).unwrap_err();
    assert_eq!(failure.error, FaultRecordError::InjectorBound);
    assert_eq!(clock.skew_ms(), 1_000);
    let failure = hub.finish(failure.timeline, at).unwrap_err();
    assert_eq!(
        failure.error,
        HubStubFaultError::Record(FaultRecordError::ClockInjectorBound)
    );
    assert_eq!(clock.skew_ms(), 1_000);
    let (hub, tl) = (failure.injector, failure.timeline);
    // 別の実行の時計の注入器は受けない。
    let mut other = injector();
    let failure = hub.finish_with_clock(&mut other, tl, at).unwrap_err();
    assert_eq!(
        failure.error,
        HubStubFaultError::Record(FaultRecordError::ClockInjectorBound)
    );
    assert_eq!(clock.skew_ms(), 1_000);
    let (record, _log) = failure
        .injector
        .finish_with_clock(&mut inj, failure.timeline, at)
        .unwrap();
    assert_eq!(record.unreleased_count(), 1);
    assert_eq!(clock.skew_ms(), 0);
}
