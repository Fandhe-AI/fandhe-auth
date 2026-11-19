//! TASK-7 #64: 生存確認の受け口の模擬の最小の確認（網羅は #68）。値は試験の中だけの仮の値。

use std::time::Duration;

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, HeartbeatReceiver, Liveness};

const TH: u64 = 10;
const T0: u64 = 1_000;

fn config(threshold: Duration) -> HubStubConfig {
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
        heartbeat_loss_threshold: StubValue::test(threshold),
    }
}

fn recv() -> HeartbeatReceiver {
    HeartbeatReceiver::new(&config(Duration::from_secs(TH)))
}

fn hb(instance: &str) -> Heartbeat {
    Heartbeat {
        instance: instance.to_owned(),
        db: Some(DbState::Ok),
        pending: Some(0),
    }
}

#[test]
fn never_seen_is_not_alive() {
    // Hub AUTHC-21: 未受信は生存と数えない
    let r = recv();
    assert_eq!(r.status("a.test", T0), Liveness::NeverSeen);
    assert!(!r.any_alive(T0));
    assert!(!r.all_alive(T0));
}

#[test]
fn alive_until_threshold_then_lost() {
    // Hub AUTHC-21: ちょうど閾値は生存、超えたら非生存
    let mut r = recv();
    r.receive(&hb("a.test"), T0);
    assert_eq!(r.status("a.test", T0), Liveness::Alive);
    assert_eq!(r.status("a.test", T0 + TH), Liveness::Alive);
    assert_eq!(r.status("a.test", T0 + TH + 1), Liveness::Lost);
}

#[test]
fn recovers_after_loss() {
    let mut r = recv();
    r.receive(&hb("a.test"), T0);
    assert_eq!(r.status("a.test", T0 + 100), Liveness::Lost);
    r.receive(&hb("a.test"), T0 + 100);
    assert_eq!(r.status("a.test", T0 + 101), Liveness::Alive);
}

#[test]
fn clock_rewind_is_lost() {
    let mut r = recv();
    r.receive(&hb("a.test"), T0);
    assert_eq!(r.status("a.test", T0 - 1), Liveness::Lost);
}

#[test]
fn instances_are_tracked_separately() {
    let mut r = recv();
    r.receive(&hb("a.test"), T0);
    r.receive(&hb("b.test"), T0 + 8);
    let now = T0 + 12;
    assert_eq!(r.status("a.test", now), Liveness::Lost);
    assert_eq!(r.status("b.test", now), Liveness::Alive);
    assert!(r.any_alive(now));
    assert!(!r.all_alive(now));
}

#[test]
fn db_state_does_not_affect_liveness() {
    // Hub AUTHC-10・RVPG-62: DB の状態・件数の不明は生存の判定に使わない
    let mut r = recv();
    r.receive(
        &Heartbeat {
            instance: "a.test".to_owned(),
            db: Some(DbState::Error),
            pending: None,
        },
        T0,
    );
    assert_eq!(r.status("a.test", T0 + 1), Liveness::Alive);
    assert_eq!(r.records()[0].pending, None);
}

#[test]
fn older_receive_does_not_rewind_last_received() {
    let mut r = recv();
    r.receive(&hb("a.test"), T0 + 50);
    r.receive(&hb("a.test"), T0);
    assert_eq!(r.records().len(), 2);
    assert_eq!(r.status("a.test", T0 + 55), Liveness::Alive);
}

#[test]
fn debug_does_not_leak_instance() {
    // SECR-23〜27
    let mut r = recv();
    let h = hb("secret-instance-name.test");
    r.receive(&h, T0);
    assert!(!format!("{r:?}").contains("secret-instance-name"));
    assert!(!format!("{h:?}").contains("secret-instance-name"));
}

#[test]
fn sub_second_threshold_rounds_up() {
    let mut r = HeartbeatReceiver::new(&config(Duration::from_millis(500)));
    r.receive(&hb("a.test"), T0);
    assert_eq!(r.status("a.test", T0 + 1), Liveness::Alive);
    assert_eq!(r.status("a.test", T0 + 2), Liveness::Lost);
}
