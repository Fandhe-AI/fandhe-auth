//! TASK-10 (5/6): 実行の記録（版・構成・日時）の出力と、秘密値・設定値が入らないことの試験。
//! Docker・ネットワークなし。値は試験の中だけの仮の値（A-2026-10-28-1）。
//! 記録は事実の出力で、合否の判定ではない（ACPT-7）。Host は未起動のまま `not_started` と残る（TASK-716）。

use std::time::Duration;

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{
    ConsoleStubComponent, Harness, HarnessConfig, HubStubComponent, VersionInfo,
};

// 記録に出てはいけない目印（canary）。
const URL_MARK: &str = "canary-host.example.test";
const CLIENT_MARK: &str = "canary-client-id.test";
const TOKEN_MARK: &str = "canary-opaque-token";

fn hub_config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test(CLIENT_MARK.to_owned()),
        public_host: StubValue::test(URL_MARK.to_owned()),
        jwks_uri: StubValue::test(format!("https://{URL_MARK}/jwks")),
        logout_receiver_uri: StubValue::test(format!("https://{URL_MARK}/logout")),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn console_config() -> ConsoleStubConfig {
    ConsoleStubConfig {
        issuer: format!("https://{URL_MARK}"),
        audience: TOKEN_MARK.to_owned(),
        client_id: CLIENT_MARK.to_owned(),
        introspection_endpoint: format!("https://{URL_MARK}/introspect"),
        validity_endpoint: format!("https://{URL_MARK}/validity"),
        delivery_window_max: Duration::from_secs(30),
    }
}

fn credential_config() -> AgentCredentialConfig {
    AgentCredentialConfig {
        client_id: CLIENT_MARK.to_owned(),
        device_authorization_endpoint: format!("https://{URL_MARK}/device"),
        token_endpoint: format!("https://{URL_MARK}/token"),
        revocation_endpoint: format!("https://{URL_MARK}/revoke"),
        registration_revocation_endpoint: format!("https://{URL_MARK}/revoke-registration"),
    }
}

fn is_hex_rev(v: &str) -> bool {
    (v.len() == 40 || v.len() == 64) && v.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn run_record_has_version_composition_time_and_no_values() {
    let hub = HubStubComponent::new(hub_config());
    let hub_handle = hub.handle();
    let console = ConsoleStubComponent::new(console_config(), credential_config());
    let console_handle = console.handle();
    let running = Harness::start(
        HarnessConfig::new()
            .with(Box::new(hub))
            .with(Box::new(console)),
    )
    .unwrap();

    let version = VersionInfo::detect().unwrap();
    let record = running
        .run_record(version)
        .unwrap()
        .with_hub_stub_record(&hub_handle.provisional_record().unwrap())
        .unwrap()
        .with_console_contract(&console_handle.contract_record().unwrap())
        .unwrap();
    let text = record.to_string();

    // 版・日時
    let value = |key: &str| -> String {
        let prefix = format!("{key}: ");
        text.lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("missing key {key}"))
            .to_owned()
    };
    assert_eq!(value("format"), "fandhe-auth-it-run-record/1");
    assert!(value("started_at").ends_with('Z'));
    assert!(value("recorded_at").ends_with('Z'));
    assert!(!value("harness_version").is_empty());
    assert!(is_hex_rev(&value("source_revision_at_record")));
    assert!(is_hex_rev(&value("spec_revision_at_record")));
    assert!(value("rustc_at_build").starts_with("rustc "));
    assert!(matches!(
        value("source_dirty_at_record").as_str(),
        "true" | "false"
    ));

    // 構成（Host・Database は起動していない）
    assert!(text.contains("component: Database not_started\n"));
    assert!(text.contains("component: Host not_started\n"));
    assert!(text.contains("component: HubStub started stub\n"));
    assert!(text.contains("component: ConsoleStub started stub\n"));
    assert!(text.contains("note: "));
    assert!(text.contains("hub_stub_field: "));
    assert!(text.contains("console_contract: "));

    // 値が入らない（Display・Debug とも）
    let debug = format!("{record:?}");
    for mark in [
        URL_MARK,
        CLIENT_MARK,
        TOKEN_MARK,
        env!("CARGO_MANIFEST_DIR"),
    ] {
        assert!(!text.contains(mark), "Display に値が出た");
        assert!(!debug.contains(mark), "Debug に値が出た");
    }

    // ファイルへの出力。2 回書くと別のファイルで、先のファイルを上書きしない。
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("it-run-record-test");
    let p1 = record.write_new(&dir).unwrap();
    let p2 = record.write_new(&dir).unwrap();
    assert_ne!(p1, p2);
    assert_eq!(std::fs::read_to_string(&p1).unwrap(), text);
    assert_eq!(std::fs::read_to_string(&p2).unwrap(), text);
    std::fs::remove_file(&p1).unwrap();
    std::fs::remove_file(&p2).unwrap();

    running.stop().unwrap();
}
