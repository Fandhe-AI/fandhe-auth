//! TASK-7・A-2026-10-28-1: Hub の値を設定で受けられることの最小の確認。
//! 値は試験の中だけの仮の値で、予約 TLD `.test` の名前を使う。

use std::time::Duration;

use fandhe_auth_hub_stub::config::HubStubConfig;

#[test]
fn config_carries_values_given_by_caller() {
    let c = HubStubConfig {
        client_id: "hub-client.test".to_owned(),
        public_host: "hub.example.test".to_owned(),
        jwks_uri: "https://hub.example.test/jwks".to_owned(),
        logout_receiver_uri: "https://hub.example.test/logout".to_owned(),
        jwks_max_age: Duration::from_secs(1),
        jwks_fetch_min_interval: Duration::from_secs(2),
        jwks_cache_max: Duration::from_secs(3),
        assertion_max_lifetime: Duration::from_secs(4),
        feed_fetch_min_interval: Duration::from_secs(5),
    };
    assert_eq!(c.client_id, "hub-client.test");
    assert_eq!(c.public_host, "hub.example.test");
    assert_eq!(c.jwks_uri, "https://hub.example.test/jwks");
    assert_eq!(c.logout_receiver_uri, "https://hub.example.test/logout");
    assert_eq!(c.jwks_max_age, Duration::from_secs(1));
    assert_eq!(c.jwks_fetch_min_interval, Duration::from_secs(2));
    assert_eq!(c.jwks_cache_max, Duration::from_secs(3));
    assert_eq!(c.assertion_max_lifetime, Duration::from_secs(4));
    assert_eq!(c.feed_fetch_min_interval, Duration::from_secs(5));
    assert_eq!(c.clone(), c);
}
