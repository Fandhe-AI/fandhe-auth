//! TASK-13 (5/8): 障害の全 8 種類を 1 つの記録に入れて、`/2` の形式を確かめる試験。Docker・ネットワークなし。
//! Hub の stub の 2 種類は注入器を通して足す（`log_pos` 付き）。他の 6 種類は時刻を固定して足す。
//! 記録は事実の出力で合否の判定ではない（ACPT-7）。値は試験の中だけの仮の値（THRT-29・A-2026-10-28-1）。

use std::time::{Duration, Instant, UNIX_EPOCH};

use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutTokenReceiver};
use fandhe_auth_it_harness::fault::{
    FaultPhase, HubStubFaultInjector, HubStubPort, HubStubRefusalKind,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{FaultKind, FaultTimeline};

#[test]
fn all_eight_categories_in_one_record_with_injector() {
    let ms = Duration::from_millis;
    let mut tl = FaultTimeline::start(
        Instant::now(),
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    )
    .unwrap();
    let mut inj = HubStubFaultInjector::new(|_: Duration| {}, &tl).unwrap();
    // 1・2 件目: Hub の stub。2 件目の前に受け口の呼び出しを 1 回挟み、`log_pos` を 0 と 1 に分ける。
    inj.apply(
        &mut tl,
        FaultKind::hub_stub_delay(HubStubPort::Jwks, ms(30)).unwrap(),
    )
    .unwrap();
    let mut lo = inj.bind_receiver(LogoutTokenReceiver::new(
        Vec::new(),
        LogoutExpectations {
            issuer: "https://auth.example.test".to_owned(),
            audience: "hub-client.test".to_owned(),
        },
    ));
    let _ = inj
        .receive_logout(&tl, &mut lo, "not-a-token", 1_000)
        .unwrap();
    inj.apply(
        &mut tl,
        FaultKind::hub_stub_refusal(HubStubPort::Logout, HubStubRefusalKind::Status(503)).unwrap(),
    )
    .unwrap();
    // 3〜8 件目: 他の 6 種類。注入器の事象より後の単調時計の値（大きい offset）で固定する。
    let others = [
        FaultKind::DbBlock,
        FaultKind::db_delay(ms(250)).unwrap(),
        FaultKind::InstanceSigterm,
        FaultKind::InstanceSigkill,
        FaultKind::ExternalStoreBlock,
        FaultKind::clock_skew(-2000).unwrap(),
    ];
    for (i, k) in others.iter().enumerate() {
        tl.record_at(
            *k,
            FaultPhase::Applied,
            UtcTimestamp::from_unix_seconds(1_700_000_001),
            1_000_000 + i as u64,
        )
        .unwrap();
    }
    let (r, log) = inj
        .finish(tl, UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap();
    assert_eq!(log.records().len(), 1);
    // SIGTERM・SIGKILL は解除が無いので数えない（8 件のうち 6 件）。
    assert_eq!(r.unreleased_count(), 6);
    let text = r.to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "format: fandhe-auth-it-fault-record/2");
    assert_eq!(lines[1], "run_mode: fault");
    assert_eq!(lines[2], "run_started_at: 2023-11-14T22:13:20Z");
    assert_eq!(lines[3], "recorded_at: 2023-11-14T22:14:20Z");
    assert_eq!(lines[4], "injection_count: 8");
    assert_eq!(lines[5], "unreleased_count: 6");
    // 通し番号・種類・対象・パラメータ・`log_pos`（Hub の stub だけ）。時刻はハーネスの時計の値なので接頭と接尾で確かめる。
    let expect: [(&str, &str); 8] = [
        (
            "injection: 1 applied hub_stub_delay target=hub_stub at=",
            " port=jwks delay_ms=30 log_pos=0",
        ),
        (
            "injection: 2 applied hub_stub_refusal target=hub_stub at=",
            " port=logout refusal=status_503 log_pos=1",
        ),
        (
            "injection: 3 applied db_block target=database at=2023-11-14T22:13:21Z offset_ms=1000000",
            "",
        ),
        (
            "injection: 4 applied db_delay target=database at=2023-11-14T22:13:21Z offset_ms=1000001",
            " delay_ms=250",
        ),
        (
            "injection: 5 applied instance_sigterm target=instance at=2023-11-14T22:13:21Z offset_ms=1000002",
            "",
        ),
        (
            "injection: 6 applied instance_sigkill target=instance at=2023-11-14T22:13:21Z offset_ms=1000003",
            "",
        ),
        (
            "injection: 7 applied external_store_block target=external_store at=2023-11-14T22:13:21Z offset_ms=1000004",
            "",
        ),
        (
            "injection: 8 applied clock_skew target=instance at=2023-11-14T22:13:21Z offset_ms=1000005",
            " skew_ms=-2000",
        ),
    ];
    for (i, (head, tail)) in expect.iter().enumerate() {
        let l = lines[6 + i];
        assert!(l.starts_with(head), "{l}");
        assert!(l.ends_with(tail), "{l}");
        // `log_pos` は Hub の stub の行だけに付く。
        assert_eq!(l.contains("log_pos="), i < 2, "{l}");
    }
    assert_eq!(
        lines[14],
        "note: 記録は事実の出力で、合否の判定でも spec の確定でもない（ACPT-7）"
    );
    assert_eq!(
        lines[15],
        "note: stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）"
    );
    assert_eq!(lines.len(), 16);
}
