//! TASK-13 (5/8): Hub の stub の遅れと拒否の注入の試験。Docker・ネットワーク・鍵の生成なし。
//! 値は予約 TLD `.test` の仮の値で、遅れの数値も任意の小さな値（THRT-29・A-2026-10-28-1）。
//! 鍵が無いので、提示・失効の通知は壊れた token で「内側に届いたか」を確かめる。
//! 記録は事実の出力で合否の判定ではない。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
//! 根拠: AVLB-18・AVLB-22・RVPG-28・OCL-36・Hub AUTHC-21・SECR-23。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::fault::{InjectedRefusal, InjectionLog};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, HeartbeatReceiver, Liveness};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutRejection, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationExpectations, PresentationRejection,
};
use fandhe_auth_it_harness::fault::{
    FaultKind, FaultRecordError, HubStubFaultError, HubStubFaultInjector, HubStubPort,
    HubStubRefusalKind,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{
    FaultRecord, FaultTimeline, Harness, HarnessConfig, HubStubComponent,
};

const NOW: u64 = 1_000;
const CANARY: &str = "canary-token-value.test";

fn config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test("hub-client.test".to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn timeline() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), t0()).unwrap()
}

fn finish(tl: FaultTimeline) -> FaultRecord {
    tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap()
}

type Waits = Rc<RefCell<Vec<Duration>>>;

fn injector() -> (HubStubFaultInjector<impl FnMut(Duration)>, Waits) {
    let waits: Waits = Rc::default();
    let w = Rc::clone(&waits);
    (
        HubStubFaultInjector::new(move |d: Duration| w.borrow_mut().push(d)),
        waits,
    )
}

fn presenter() -> IdentityTokenReceiver {
    IdentityTokenReceiver::new(
        Vec::new(),
        PresentationExpectations {
            issuer: "https://auth.example.test".to_owned(),
            audience: "hub-client.test".to_owned(),
        },
    )
}

fn logout_rx() -> LogoutTokenReceiver {
    LogoutTokenReceiver::new(
        Vec::new(),
        LogoutExpectations {
            issuer: "https://auth.example.test".to_owned(),
            audience: "hub-client.test".to_owned(),
        },
    )
}

fn hb() -> Heartbeat {
    Heartbeat {
        instance: "a.test".to_owned(),
        db: Some(DbState::Ok),
        pending: Some(0),
    }
}

fn refusal(port: HubStubPort, r: HubStubRefusalKind) -> FaultKind {
    FaultKind::hub_stub_refusal(port, r).unwrap()
}

fn delay(port: HubStubPort, ms: u64) -> FaultKind {
    FaultKind::hub_stub_delay(port, Duration::from_millis(ms)).unwrap()
}

fn injected_flags(log: &InjectionLog) -> Vec<bool> {
    log.records().iter().map(|r| r.injected.is_some()).collect()
}

/// OCL-36: JWKS の拒否は状態もキャッシュも変えず、解除の後は 200 で鍵が含まれる。
#[test]
fn jwks_status_refusal_then_release() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let mut jwks = HubJwksEndpoint::new(&config());
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    inj.apply(&mut tl, k).unwrap();
    let v = jwks.current_version();
    let n = jwks.records().len();
    assert_eq!(
        inj.serve_jwks(&mut jwks, NOW).unwrap_err(),
        InjectedRefusal::Status(503)
    );
    assert_eq!(jwks.current_version(), v);
    assert_eq!(jwks.records().len(), n);
    inj.release(&mut tl, k).unwrap();
    assert_eq!(inj.active_count(), 0);
    let ok = inj.serve_jwks(&mut jwks, NOW + 1).unwrap();
    assert_eq!(ok.status, 200);
    assert_eq!(jwks.records().len(), n + 1);
    assert_eq!(injected_flags(inj.call_log()), [true, false]);
}

#[test]
fn jwks_malformed_body_is_200_but_unparsable() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let mut jwks = HubJwksEndpoint::new(&config());
    inj.apply(
        &mut tl,
        refusal(HubStubPort::Jwks, HubStubRefusalKind::MalformedBody),
    )
    .unwrap();
    let r = inj.serve_jwks(&mut jwks, NOW).unwrap();
    assert_eq!(r.status, 200);
    assert!(!r.body.ends_with('}'));
}

/// Hub AUTHC-21: 拒否の期間は途絶、解除の後の受理で生存に戻る。
#[test]
fn heartbeat_refusal_makes_loss_and_release_recovers() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let mut hbr = HeartbeatReceiver::new(&config());
    let k = refusal(HubStubPort::Heartbeat, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, k).unwrap();
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert!(hbr.records().is_empty());
    assert_eq!(hbr.status("a.test", NOW + 1), Liveness::NeverSeen);
    inj.release(&mut tl, k).unwrap();
    inj.receive_heartbeat(&mut hbr, &hb(), NOW + 2).unwrap();
    assert_eq!(hbr.status("a.test", NOW + 3), Liveness::Alive);
    assert_eq!(hbr.status("a.test", NOW + 2 + 100), Liveness::Lost);
}

/// RVPG-28 の前提: 拒否は内側に届かず、解除の後は内側に届く（壊れた token なので Malformed）。
#[test]
fn logout_and_presentation_refusal_do_not_reach_inner() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let mut lo = logout_rx();
    let mut pr = presenter();
    let kl = refusal(HubStubPort::Logout, HubStubRefusalKind::Status(500));
    let kp = refusal(HubStubPort::Presentation, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, kl).unwrap();
    inj.apply(&mut tl, kp).unwrap();
    assert_eq!(
        inj.receive_logout(&mut lo, CANARY, NOW).unwrap_err(),
        InjectedRefusal::Status(500)
    );
    assert_eq!(
        inj.present(&mut pr, CANARY, NOW).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    inj.release(&mut tl, kl).unwrap();
    inj.release(&mut tl, kp).unwrap();
    assert_eq!(
        inj.receive_logout(&mut lo, CANARY, NOW + 1).unwrap(),
        Err(LogoutRejection::Malformed)
    );
    assert_eq!(
        inj.present(&mut pr, CANARY, NOW + 1).unwrap(),
        Err(PresentationRejection::Malformed)
    );
    assert!(lo.received().is_empty());
    assert!(pr.received().is_empty());
}

#[test]
fn delay_reaches_wait_on_all_ports_and_passes_result_through() {
    let (mut inj, waits) = injector();
    let mut tl = timeline();
    let mut lo = logout_rx();
    let mut pr = presenter();
    let mut hbr = HeartbeatReceiver::new(&config());
    let mut jwks = HubJwksEndpoint::new(&config());
    for p in [
        HubStubPort::Presentation,
        HubStubPort::Logout,
        HubStubPort::Heartbeat,
        HubStubPort::Jwks,
    ] {
        inj.apply(&mut tl, delay(p, 7)).unwrap();
    }
    assert!(inj.present(&mut pr, CANARY, NOW).unwrap().is_err());
    assert!(inj.receive_logout(&mut lo, CANARY, NOW).unwrap().is_err());
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(inj.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    assert_eq!(*waits.borrow(), vec![Duration::from_millis(7); 4]);
    assert_eq!(hbr.status("a.test", NOW), Liveness::Alive);
}

/// 同じ口の遅れと拒否は 1 つの規則に合成される。片方が黙って落ちない。
#[test]
fn delay_and_refusal_on_same_port_are_combined() {
    let (mut inj, waits) = injector();
    let mut tl = timeline();
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.apply(&mut tl, delay(HubStubPort::Heartbeat, 5))
        .unwrap();
    inj.apply(
        &mut tl,
        refusal(HubStubPort::Heartbeat, HubStubRefusalKind::Status(502)),
    )
    .unwrap();
    assert_eq!(inj.active_count(), 2);
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap_err(),
        InjectedRefusal::Status(502)
    );
    assert_eq!(*waits.borrow(), vec![Duration::from_millis(5)]);
    assert!(hbr.records().is_empty());
}

/// 基準線では記録が拒否し、注入もしない。
#[test]
fn baseline_rejects_apply_and_injects_nothing() {
    let (mut inj, waits) = injector();
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let mut jwks = HubJwksEndpoint::new(&config());
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    assert_eq!(
        inj.apply(&mut tl, k).unwrap_err(),
        HubStubFaultError::Record(FaultRecordError::EventsInBaseline)
    );
    assert_eq!(inj.active_count(), 0);
    assert_eq!(inj.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    assert_eq!(injected_flags(inj.call_log()), [false]);
    assert!(waits.borrow().is_empty());
    let text = finish(tl).to_string();
    assert!(text.contains("run_mode: baseline"));
    assert!(text.contains("injection_count: 0"));
}

#[test]
fn invalid_params_are_rejected_without_recording() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let bad = [
        FaultKind::DbBlock,
        FaultKind::HubStubRefusal {
            port: HubStubPort::Logout,
            refusal: HubStubRefusalKind::MalformedBody,
        },
        FaultKind::HubStubDelay {
            port: HubStubPort::Jwks,
            delay_ms: 0,
        },
        FaultKind::HubStubRefusal {
            port: HubStubPort::Jwks,
            refusal: HubStubRefusalKind::Status(99),
        },
    ];
    for k in bad {
        assert_eq!(
            inj.apply(&mut tl, k).unwrap_err(),
            HubStubFaultError::InvalidParam
        );
    }
    assert_eq!(inj.active_count(), 0);
    assert_eq!(
        tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
            .unwrap_err(),
        FaultRecordError::NoInjectionInFaultRun
    );
}

#[test]
fn state_errors_do_not_record() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let k = delay(HubStubPort::Jwks, 5);
    assert_eq!(
        inj.release(&mut tl, k).unwrap_err(),
        HubStubFaultError::NotActive
    );
    inj.apply(&mut tl, k).unwrap();
    assert_eq!(
        inj.apply(&mut tl, k).unwrap_err(),
        HubStubFaultError::AlreadyActive
    );
    assert_eq!(
        inj.release(&mut tl, delay(HubStubPort::Jwks, 6))
            .unwrap_err(),
        HubStubFaultError::NotActive
    );
    inj.release(&mut tl, k).unwrap();
    assert_eq!(finish(tl).events().len(), 2);
}

#[test]
fn record_lines_and_no_token_leak() {
    let (mut inj, _) = injector();
    let mut tl = timeline();
    let mut lo = logout_rx();
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    let kl = refusal(HubStubPort::Logout, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, k).unwrap();
    inj.apply(&mut tl, kl).unwrap();
    let _ = inj.receive_logout(&mut lo, CANARY, NOW);
    let text = finish(tl).to_string();
    assert!(text.contains("hub_stub_refusal target=hub_stub"));
    assert!(text.contains("port=jwks refusal=status_503"));
    assert!(text.contains("A-2026-10-28-9"));
    assert!(!text.contains(CANARY));
    assert!(!format!("{inj:?}").contains(CANARY));
    assert!(!format!("{:?}", inj.call_log()).contains(CANARY));
    for e in [
        HubStubFaultError::InvalidParam,
        HubStubFaultError::AlreadyActive,
        HubStubFaultError::NotActive,
    ] {
        assert!(!e.to_string().contains(CANARY));
    }
}

/// ハーネスで起動した stub のセッションの口へ注入できる。
#[test]
fn injects_into_harness_started_stub() {
    let comp = HubStubComponent::new(config());
    let handle = comp.handle();
    let _running = Harness::start(HarnessConfig::new().with(Box::new(comp))).unwrap();
    let (mut inj, _) = injector();
    let mut tl = timeline();
    inj.apply(
        &mut tl,
        refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503)),
    )
    .unwrap();
    let r = handle.with(|s| inj.serve_jwks(&mut s.jwks, NOW)).unwrap();
    assert_eq!(r.unwrap_err(), InjectedRefusal::Status(503));
}
