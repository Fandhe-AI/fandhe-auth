//! TASK-8・A-2026-10-28-14: console の値を設定で受けられることの最小の確認。
//! 値は試験の中だけの仮の値で、予約 TLD `.test` の名前を使う。

use std::time::Duration;

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};

#[test]
fn agent_credential_config_carries_values_given_by_caller() {
    // #75
    let c = AgentCredentialConfig {
        client_id: "agent-test-client".to_owned(),
        device_authorization_endpoint: "https://auth.example.test/device".to_owned(),
        token_endpoint: "https://auth.example.test/token".to_owned(),
        revocation_endpoint: "https://auth.example.test/revoke".to_owned(),
        registration_revocation_endpoint: "https://hub.example.test/revoke".to_owned(),
    };
    assert_eq!(c.client_id, "agent-test-client");
    assert_eq!(c.token_endpoint, "https://auth.example.test/token");
    assert_eq!(c.clone(), c);
}

#[test]
fn config_carries_values_given_by_caller() {
    let c = ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "https://auth.example.test/aud/dev-console".to_owned(),
        client_id: "console-test-client".to_owned(),
        introspection_endpoint: "https://auth.example.test/introspect".to_owned(),
        validity_endpoint: "https://console.example.test/validity".to_owned(),
        delivery_window_max: Duration::from_secs(5),
    };
    assert_eq!(c.issuer, "https://auth.example.test");
    assert_eq!(c.audience, "https://auth.example.test/aud/dev-console");
    assert_eq!(c.client_id, "console-test-client");
    assert_eq!(
        c.introspection_endpoint,
        "https://auth.example.test/introspect"
    );
    assert_eq!(c.validity_endpoint, "https://console.example.test/validity");
    assert_eq!(c.delivery_window_max, Duration::from_secs(5));
    assert_eq!(c.clone(), c);
}
