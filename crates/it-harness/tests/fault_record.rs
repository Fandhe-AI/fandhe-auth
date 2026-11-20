//! TASK-13 (1/8): 障害注入の種類と記録の形式の試験。Docker・ネットワークなし。
//! 値は試験の中だけの仮の値（THRT-29・A-2026-10-28-1）。記録は事実の出力で合否の判定ではない（ACPT-7）。

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{
    FaultPhase, FaultRecordError, FaultTarget, HubStubPort, HubStubRefusalKind,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{FaultCategory, FaultKind, FaultTimeline, RunMode};

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn ts(s: u64) -> UtcTimestamp {
    UtcTimestamp::from_unix_seconds(s)
}

fn fault() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), t0()).unwrap()
}

const HEAD: &str = "format: fandhe-auth-it-fault-record/2\n\
run_mode: {MODE}\n\
run_started_at: 2023-11-14T22:13:20Z\n\
recorded_at: 2023-11-14T22:14:20Z\n";
const NOTE: &str = "note: 記録は事実の出力で、合否の判定でも spec の確定でもない（ACPT-7）\n";

#[test]
fn baseline_uses_same_keys_with_zero_counts() {
    let r = FaultTimeline::baseline(Instant::now(), t0())
        .unwrap()
        .finish(ts(1_700_000_060))
        .unwrap();
    let expect =
        HEAD.replace("{MODE}", "baseline") + "injection_count: 0\nunreleased_count: 0\n" + NOTE;
    assert_eq!(r.to_string(), expect);
    assert_eq!(r.mode(), RunMode::Baseline);
}

#[test]
fn all_categories_in_one_record() {
    let ms = |n| Duration::from_millis(n);
    let kinds = [
        FaultKind::DbBlock,
        FaultKind::db_delay(ms(250)).unwrap(),
        FaultKind::InstanceSigterm,
        FaultKind::InstanceSigkill,
        FaultKind::ExternalStoreBlock,
        FaultKind::clock_skew(-2000).unwrap(),
    ];
    let mut tl = fault();
    for (i, k) in kinds.iter().enumerate() {
        tl.record_at(*k, FaultPhase::Applied, ts(1_700_000_001), i as u64)
            .unwrap();
    }
    let r = tl.finish(ts(1_700_000_060)).unwrap();
    // SIGTERM・SIGKILL は解除が無いので数えない。
    assert_eq!(r.unreleased_count(), 4);
    let at = "at=2023-11-14T22:13:21Z";
    let expect = HEAD.replace("{MODE}", "fault")
        + "injection_count: 6\nunreleased_count: 4\n"
        + &format!(
            "injection: 1 applied db_block target=database {at} offset_ms=0\n\
injection: 2 applied db_delay target=database {at} offset_ms=1 delay_ms=250\n\
injection: 3 applied instance_sigterm target=instance {at} offset_ms=2\n\
injection: 4 applied instance_sigkill target=instance {at} offset_ms=3\n\
injection: 5 applied external_store_block target=external_store {at} offset_ms=4\n\
injection: 6 applied clock_skew target=instance {at} offset_ms=5 skew_ms=-2000\n"
        )
        + NOTE;
    assert_eq!(r.to_string(), expect);
}

/// 公開の直接記録は Hub の stub の事象を常に拒否する（注入器を通さずに記録を確定させない）。
#[test]
fn direct_entries_reject_hub_stub_events_even_unbound() {
    let kinds = [
        FaultKind::hub_stub_delay(HubStubPort::Jwks, Duration::from_millis(30)).unwrap(),
        FaultKind::hub_stub_refusal(HubStubPort::Logout, HubStubRefusalKind::Status(503)).unwrap(),
    ];
    for k in kinds {
        let mut f = fault();
        assert_eq!(
            f.record_at(k, FaultPhase::Applied, ts(1), 0),
            Err(FaultRecordError::HubStubEventOutsideInjector)
        );
        assert_eq!(
            f.record_now(k, FaultPhase::Applied),
            Err(FaultRecordError::HubStubEventOutsideInjector)
        );
        // 事象が 0 件のままなので、直接の確定も失敗する。
        assert_eq!(
            f.finish(ts(2)).unwrap_err().error,
            FaultRecordError::NoInjectionInFaultRun
        );
    }
}

#[test]
fn catalog_is_complete_and_targets_match() {
    assert_eq!(FaultCategory::CATALOG.len(), 8);
    let t = |c: FaultCategory| c.target();
    assert_eq!(t(FaultCategory::DbBlock), FaultTarget::Database);
    assert_eq!(t(FaultCategory::DbDelay), FaultTarget::Database);
    assert_eq!(t(FaultCategory::InstanceSigterm), FaultTarget::Instance);
    assert_eq!(t(FaultCategory::InstanceSigkill), FaultTarget::Instance);
    assert_eq!(
        t(FaultCategory::ExternalStoreBlock),
        FaultTarget::ExternalStore
    );
    assert_eq!(t(FaultCategory::HubStubDelay), FaultTarget::HubStub);
    assert_eq!(t(FaultCategory::HubStubRefusal), FaultTarget::HubStub);
    assert_eq!(t(FaultCategory::ClockSkew), FaultTarget::Instance);
}

#[test]
fn rejected_cases() {
    let mut b = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    assert_eq!(
        b.record_at(FaultKind::DbBlock, FaultPhase::Applied, ts(1), 0),
        Err(FaultRecordError::EventsInBaseline)
    );
    assert_eq!(
        fault().finish(ts(1)).unwrap_err().error,
        FaultRecordError::NoInjectionInFaultRun
    );
    let mut f = fault();
    assert_eq!(
        f.record_at(FaultKind::DbBlock, FaultPhase::Released, ts(1), 0),
        Err(FaultRecordError::ReleaseWithoutApply)
    );
    assert_eq!(
        f.record_at(FaultKind::InstanceSigkill, FaultPhase::Released, ts(1), 0),
        Err(FaultRecordError::ReleaseOfInstantaneous)
    );
    f.record_at(FaultKind::DbBlock, FaultPhase::Applied, ts(1), 10)
        .unwrap();
    assert_eq!(
        f.record_at(FaultKind::DbBlock, FaultPhase::Released, ts(1), 9),
        Err(FaultRecordError::OffsetWentBackwards)
    );
    // 公開フィールドを直接組んだ 0 のパラメータも入口で拒否する。
    assert_eq!(
        f.record_at(
            FaultKind::DbDelay { delay_ms: 0 },
            FaultPhase::Applied,
            ts(1),
            11
        ),
        Err(FaultRecordError::InvalidParam)
    );
    assert_eq!(
        f.record_at(
            FaultKind::ClockSkew { skew_ms: 0 },
            FaultPhase::Applied,
            ts(1),
            11
        ),
        Err(FaultRecordError::InvalidParam)
    );
    assert!(FaultKind::db_delay(Duration::ZERO).is_err());
    assert!(FaultKind::clock_skew(0).is_err());
}

#[test]
fn applied_then_released_is_balanced() {
    let mut f = fault();
    f.record_at(FaultKind::DbBlock, FaultPhase::Applied, ts(1), 0)
        .unwrap();
    f.record_at(FaultKind::DbBlock, FaultPhase::Released, ts(2), 5)
        .unwrap();
    let r = f.finish(ts(3)).unwrap();
    assert_eq!(r.unreleased_count(), 0);
    assert!(r.to_string().contains("injection: 2 released db_block"));
    assert!(!r.to_string().contains("stub の合格"));
}

#[test]
fn record_now_uses_harness_clock() {
    let mut f = fault();
    f.record_now(FaultKind::DbBlock, FaultPhase::Applied)
        .unwrap();
    assert_eq!(f.finish(ts(1)).unwrap().events().len(), 1);
}

#[test]
fn output_has_no_paths_or_debug_leaks() {
    let mut f = fault();
    f.record_at(FaultKind::DbBlock, FaultPhase::Applied, ts(1), 0)
        .unwrap();
    let r = f.finish(ts(2)).unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fault_record");
    let p = r.write_new(&dir).unwrap();
    let p2 = r.write_new(&dir).unwrap();
    assert_ne!(p, p2);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), r.to_string());
    let probe = dir.to_string_lossy().into_owned();
    let all = format!(
        "{r}{r:?}{}{}",
        FaultRecordError::WriteFailed,
        FaultRecordError::InvalidParam
    );
    assert!(!all.contains(&probe));
    assert!(!all.contains(env!("CARGO_MANIFEST_DIR")));
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(p2);
}
