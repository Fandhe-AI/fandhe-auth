//! TASK-10 (3/6): Hub の stub をハーネスから起動し、つながって呼べること・実物でない旨が記録に残ることの試験。
//! Docker・ネットワークなし。値は試験の中だけの仮の値（A-2026-10-28-1）。
//! 最小の組込み先（TASK-716）から stub への接続は本段では未達で、試験内の Fake の Host を達成に数えない（ACPT-7）。

use std::time::Duration;

use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, Liveness};
use fandhe_auth_it_harness::{
    ComponentKind, Harness, HarnessConfig, HarnessErrorKind, HubStubComponent,
};

const T0: u64 = 1_000;

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

#[test]
fn hub_stub_starts_connects_and_records_not_real() {
    let comp = HubStubComponent::new(config());
    let handle = comp.handle();
    assert!(!handle.is_running());
    assert!(handle.with(|_| ()).is_none());

    let running = Harness::start(HarnessConfig::new().with(Box::new(comp))).unwrap();
    assert_eq!(running.kinds(), vec![ComponentKind::HubStub]);
    assert!(handle.is_running());

    // JWKS の往復
    let key = AssertionSigningKey::generate("kid-1.test").unwrap();
    let resp = handle
        .with(|s| {
            s.jwks.publish(&key);
            s.jwks.serve(T0)
        })
        .unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("kid-1.test"));

    // 生存確認の往復
    let live = handle
        .with(|s| {
            s.heartbeat.receive(
                &Heartbeat {
                    instance: "a.test".to_owned(),
                    db: Some(DbState::Ok),
                    pending: Some(0),
                },
                T0,
            );
            s.heartbeat.status("a.test", T0)
        })
        .unwrap();
    assert_eq!(live, Liveness::Alive);

    // 実物でない旨の記録。値は載せない。
    let rec = running.composition_record();
    assert_eq!(rec.entries.len(), 1);
    assert!(rec.entries[0].stub);
    let text = rec.to_string();
    assert!(text.contains("実物ではない"));
    assert!(text.contains("A-2026-10-28-9"));
    assert!(!text.contains("hub.example.test"));
    let prov = handle.provisional_record().unwrap().to_string();
    assert!(prov.contains("仮置き"));
    assert!(!prov.contains("hub.example.test"));

    running.stop().unwrap();
    assert!(!handle.is_running());
    assert!(handle.with(|_| ()).is_none());
    assert!(handle.provisional_record().is_none());
}

#[test]
fn is_stub_covers_every_kind() {
    for k in ComponentKind::START_ORDER {
        let expected = match k {
            ComponentKind::HubStub | ComponentKind::ConsoleStub => true,
            ComponentKind::Database | ComponentKind::Host => false,
        };
        assert_eq!(k.is_stub(), expected);
    }
}

#[test]
fn double_start_fails_and_debug_hides_values() {
    use fandhe_auth_it_harness::Component;
    let mut comp = HubStubComponent::new(config());
    comp.start().unwrap();
    let err = comp.start().unwrap_err();
    assert_eq!(err.kind(), HarnessErrorKind::StartFailed);
    assert_eq!(err.component(), ComponentKind::HubStub);
    let dbg = format!("{comp:?} {:?}", comp.handle());
    assert!(!dbg.contains("hub.example.test"));
    assert!(!dbg.contains("hub-client.test"));
    comp.stop().unwrap();
    comp.stop().unwrap();
}
