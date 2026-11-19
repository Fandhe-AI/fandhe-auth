//! TASK-7（#67）・A-2026-10-28-1・A-2026-10-28-9: 仮置きの印と記録の試験。
//! 値は予約 TLD `.test` の名前と試験用の `Duration` だけを使い、PoC・Hub の数値は使わない。

use std::time::Duration;

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue, ValueMark};

const FIELDS: [&str; 11] = [
    "client_id",
    "public_host",
    "jwks_uri",
    "logout_receiver_uri",
    "jwks_max_age",
    "jwks_fetch_min_interval",
    "jwks_cache_max",
    "jwks_stale_max",
    "assertion_max_lifetime",
    "feed_fetch_min_interval",
    "heartbeat_loss_threshold",
];

fn config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test("hub-client.test".to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::poc(Duration::from_millis(7777), "PoC-7"),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(4)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

#[test]
fn record_has_every_field_exactly_once() {
    let r = config().provisional_record();
    let mut names: Vec<&str> = r.entries.iter().map(|e| e.field).collect();
    names.sort_unstable();
    let mut want = FIELDS.to_vec();
    want.sort_unstable();
    assert_eq!(names, want);
}

#[test]
fn poc_mark_shows_provisional_and_poc_id_without_value() {
    let r = config().provisional_record();
    let e = r
        .entries
        .iter()
        .find(|e| e.field == "jwks_max_age")
        .unwrap();
    assert_eq!(e.mark, ValueMark::PocProvisional { poc: "PoC-7" });
    let text = r.to_string();
    assert!(text.contains("jwks_max_age: 仮置き（PoC-7）"));
    assert!(!text.contains("7777"));
}

#[test]
fn test_mark_shows_provisional() {
    let r = config().provisional_record();
    let e = r.entries.iter().find(|e| e.field == "client_id").unwrap();
    assert_eq!(e.mark, ValueMark::TestProvisional);
    assert!(r.to_string().contains("client_id: 仮置き（試験用）"));
    assert!(!r.to_string().contains("hub-client.test"));
}

#[test]
fn debug_shows_provisional() {
    let v = StubValue::test(Duration::from_secs(1));
    assert!(format!("{v:?}").contains("仮置き"));
    let p = StubValue::poc(Duration::from_secs(1), "PoC-1");
    assert!(format!("{p:?}").contains("仮置き"));
}

/// 網羅の `match`（ワイルドカードなし）。本番・確定の種類を足すとここがコンパイルできなくなる（A-2026-10-28-9）。
#[test]
fn every_mark_kind_is_labeled_provisional() {
    for m in [
        ValueMark::PocProvisional { poc: "PoC-1" },
        ValueMark::TestProvisional,
    ] {
        match m {
            ValueMark::PocProvisional { .. } | ValueMark::TestProvisional => {
                assert_eq!(m.label(), "仮置き");
            }
        }
    }
}

#[test]
fn record_display_starts_with_not_real_hub_notice() {
    let text = config().provisional_record().to_string();
    let first = text.lines().next().unwrap();
    assert!(first.contains("実物ではない"));
    assert!(first.contains("A-2026-10-28-9"));
}

#[test]
fn get_returns_the_given_value() {
    assert_eq!(
        *StubValue::test(Duration::from_secs(9)).get(),
        Duration::from_secs(9)
    );
}
