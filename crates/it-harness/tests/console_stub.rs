//! TASK-10 (4/6): console の stub をハーネスから起動し、つながって呼べること・実物でない旨が記録に残ることの試験。
//! Docker・ネットワークなし。値は試験の中だけの仮の値（A-2026-10-28-14）。token は不透明な文字列。
//! 最小の組込み先（TASK-716）から stub への接続は本段では未達で、Scripted の口を達成に数えない（ACPT-7）。

use std::time::Duration;

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, DeliveryContext, DeliveryDecision, DeliveryRejection, DeliveryScript,
    ScriptedDeliveryPorts, ValidityAnswer, ValidityTarget,
};
use fandhe_auth_console_stub::introspection::AuthorityRequirement;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_it_harness::{
    ComponentKind, ConsoleStubComponent, Harness, HarnessConfig, HarnessErrorKind, HubStubComponent,
};

const NOW: u64 = 1_000;
const TOKEN: &str = "opaque-token-for-test";
const URL_MARK: &str = "console.example.test";

fn config() -> ConsoleStubConfig {
    ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "aud-for-test".to_owned(),
        client_id: "console-client.test".to_owned(),
        introspection_endpoint: format!("https://{URL_MARK}/introspect"),
        validity_endpoint: format!("https://{URL_MARK}/validity"),
        delivery_window_max: Duration::from_secs(30),
    }
}

fn credential_config() -> AgentCredentialConfig {
    AgentCredentialConfig {
        client_id: "console-client.test".to_owned(),
        device_authorization_endpoint: format!("https://{URL_MARK}/device"),
        token_endpoint: format!("https://{URL_MARK}/token"),
        revocation_endpoint: format!("https://{URL_MARK}/revoke"),
        registration_revocation_endpoint: format!("https://{URL_MARK}/revoke-registration"),
    }
}

fn hub_config() -> HubStubConfig {
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

fn ctx() -> DeliveryContext {
    DeliveryContext {
        requirement: AuthorityRequirement {
            required_scope: "scope-a".to_owned(),
            bound_jkt: None,
        },
        decider: ValidityTarget::Session {
            sid: "sid-for-test".to_owned(),
        },
        delegation_ref: "delegation-for-test".to_owned(),
    }
}

fn script(agent_active: bool) -> DeliveryScript {
    DeliveryScript {
        agent: if agent_active {
            AgentCredentialScript::Active {
                scope: "scope-a".to_owned(),
                jkt: None,
                exp: NOW + 100,
            }
        } else {
            AgentCredentialScript::Inactive
        },
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    }
}

#[test]
fn console_stub_starts_connects_and_records_not_real() {
    let comp = ConsoleStubComponent::new(config(), credential_config());
    let handle = comp.handle();
    assert!(!handle.is_running());
    assert!(handle.with(|_| ()).is_none());
    assert!(handle.contract_record().is_none());

    let running = Harness::start(HarnessConfig::new().with(Box::new(comp))).unwrap();
    assert_eq!(running.kinds(), vec![ComponentKind::ConsoleStub]);
    assert!(handle.is_running());

    // 配送の時点の確認の往復。成功は 3 つとも有効のときだけ。
    let mut ports = ScriptedDeliveryPorts::new(script(true));
    let decision = handle
        .with(|s| s.delivery.check(TOKEN, &ctx(), NOW, &mut ports))
        .unwrap()
        .unwrap();
    assert_eq!(decision, DeliveryDecision::Delivered { checked_at: NOW });
    let calls = ports.calls();
    assert_eq!((calls.introspect, calls.decider, calls.hub), (1, 1, 1));

    // 成功にならない向き: ① が無効なら拒否。
    ports.set_script(script(false));
    let decision = handle
        .with(|s| s.delivery.check(TOKEN, &ctx(), NOW, &mut ports))
        .unwrap()
        .unwrap();
    assert_eq!(
        decision,
        DeliveryDecision::Rejected(DeliveryRejection::IntrospectInactive)
    );

    // 実物でない旨の記録。値は載せない。
    let rec = running.composition_record();
    assert_eq!(rec.entries.len(), 1);
    assert!(rec.entries[0].stub);
    let text = rec.to_string();
    assert!(text.contains("実物ではない"));
    assert!(text.contains("A-2026-10-28-9"));
    assert!(!text.contains(URL_MARK));
    let contract = handle.contract_record().unwrap().to_string();
    assert!(contract.contains("実物ではない"));
    assert!(contract.contains("A-2026-10-28-12"));
    assert!(!contract.contains(URL_MARK));

    running.stop().unwrap();
    assert!(!handle.is_running());
    assert!(handle.with(|_| ()).is_none());
    assert!(handle.contract_record().is_none());
}

#[test]
fn hub_and_console_stubs_start_in_order() {
    let hub = HubStubComponent::new(hub_config());
    let hub_handle = hub.handle();
    let console = ConsoleStubComponent::new(config(), credential_config());
    let console_handle = console.handle();

    // 渡す順は逆でも、起動順は HubStub → ConsoleStub。
    let running = Harness::start(
        HarnessConfig::new()
            .with(Box::new(console))
            .with(Box::new(hub)),
    )
    .unwrap();
    assert_eq!(
        running.kinds(),
        vec![ComponentKind::HubStub, ComponentKind::ConsoleStub]
    );
    let rec = running.composition_record();
    assert_eq!(rec.entries.len(), 2);
    assert!(rec.entries.iter().all(|e| e.stub));
    assert!(hub_handle.is_running() && console_handle.is_running());

    running.stop().unwrap();
    assert!(!hub_handle.is_running() && !console_handle.is_running());
}

#[test]
fn double_start_fails_and_debug_hides_values() {
    let mut comp = ConsoleStubComponent::new(config(), credential_config());
    let handle = comp.handle();
    let shown = format!("{comp:?} {handle:?}");
    for secret in [URL_MARK, "console-client.test", TOKEN] {
        assert!(!shown.contains(secret));
    }

    use fandhe_auth_it_harness::Component;
    comp.start().unwrap();
    let err = comp.start().unwrap_err();
    assert_eq!(err.kind(), HarnessErrorKind::StartFailed);
    assert_eq!(err.component(), ComponentKind::ConsoleStub);
    let session = format!("{:?}", handle.with(|s| format!("{s:?}")).unwrap());
    assert!(!session.contains(URL_MARK));

    comp.stop().unwrap();
    comp.stop().unwrap();
    assert!(!handle.is_running());
}
