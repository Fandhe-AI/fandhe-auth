//! TASK-8・#75: 配送の時点の確認の口の記録の最小の確認（網羅は #78）。

use std::time::Duration;

use fandhe_auth_console_stub::config::ConsoleStubConfig;
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, ConsoleDeliveryChecker, DeliveryContext, DeliveryScript,
    ScriptedDeliveryPorts, ValidityAnswer, ValidityTarget,
};
use fandhe_auth_console_stub::introspection::AuthorityRequirement;
use fandhe_auth_console_stub::record::{CallKind, CallOutcome, RecordingDeliveryPorts};

const TOKEN: &str = "opaque-test-token-0001";
const SID: &str = "opaque-sid-0002";

fn run(agent: AgentCredentialScript) -> (RecordingDeliveryPorts<ScriptedDeliveryPorts>, String) {
    let checker = ConsoleDeliveryChecker::new(&ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "https://auth.example.test/aud/console".to_owned(),
        client_id: "console-test-client".to_owned(),
        introspection_endpoint: "https://auth.example.test/introspect".to_owned(),
        validity_endpoint: "https://console.example.test/validity".to_owned(),
        delivery_window_max: Duration::from_secs(5),
    });
    let mut ports = RecordingDeliveryPorts::new(ScriptedDeliveryPorts::new(DeliveryScript {
        agent,
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    }));
    let ctx = DeliveryContext {
        requirement: AuthorityRequirement {
            required_scope: "agent:run".to_owned(),
            bound_jkt: None,
        },
        decider: ValidityTarget::Session {
            sid: SID.to_owned(),
        },
        delegation_ref: "delegation-test".to_owned(),
    };
    let d = checker.check(TOKEN, &ctx, 1000, &mut ports);
    (ports, format!("{d:?}"))
}

#[test]
fn records_three_checks_in_order() {
    // ADLG-15
    let (p, _) = run(AgentCredentialScript::Active {
        scope: "agent:run".to_owned(),
        jkt: None,
        exp: 2000,
    });
    let kinds: Vec<_> = p.log().records().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            CallKind::Introspection,
            CallKind::DeciderValidity,
            CallKind::HubValidity
        ]
    );
    assert_eq!(p.inner().calls().introspect, 1);
}

#[test]
fn inactive_agent_still_records_other_checks() {
    // ADLG-16
    let (p, text) = run(AgentCredentialScript::Inactive);
    assert_eq!(p.log().records().len(), 3);
    assert_eq!(
        p.log().records()[0].outcome,
        CallOutcome::Responded { status: 200 }
    );
    let all = format!("{:?}{text}", p.log());
    assert!(!all.contains(TOKEN) && !all.contains(SID));
}
