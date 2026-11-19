//! TASK-7・A-2026-10-28-1: Hub の値を設定で受けられることの最小の確認。
//! 値は試験の中だけの仮の値で、予約 TLD `.test` の名前を使う。

use std::time::Duration;

use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};

#[test]
fn config_carries_values_given_by_caller() {
    let c = HubStubConfig {
        client_id: StubValue::test("hub-client.test".to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(4)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    };
    assert_eq!(c.client_id.get(), "hub-client.test");
    assert_eq!(c.public_host.get(), "hub.example.test");
    assert_eq!(c.jwks_uri.get(), "https://hub.example.test/jwks");
    assert_eq!(
        c.logout_receiver_uri.get(),
        "https://hub.example.test/logout"
    );
    assert_eq!(*c.jwks_max_age.get(), Duration::from_secs(1));
    assert_eq!(*c.jwks_fetch_min_interval.get(), Duration::from_secs(2));
    assert_eq!(*c.jwks_cache_max.get(), Duration::from_secs(3));
    assert_eq!(*c.jwks_stale_max.get(), Duration::from_secs(6));
    assert_eq!(*c.assertion_max_lifetime.get(), Duration::from_secs(4));
    assert_eq!(*c.feed_fetch_min_interval.get(), Duration::from_secs(5));
    assert_eq!(*c.heartbeat_loss_threshold.get(), Duration::from_secs(10));
    assert_eq!(c.clone(), c);
}
