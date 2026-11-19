//! TASK-8・#75・#78: 配送の時点の確認の口の記録の試験。

use std::time::Duration;

use fandhe_auth_console_stub::config::ConsoleStubConfig;
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, ConsoleDeliveryChecker, DeliveryContext, DeliveryFailure, DeliveryPorts,
    DeliveryScript, HubValidityRequest, ScriptedDeliveryPorts, ValidityAnswer, ValidityRequest,
    ValidityTarget,
};
use fandhe_auth_console_stub::introspection::{
    AuthorityRequirement, IntrospectionFailure, IntrospectionHttpResponse, IntrospectionRequest,
    TransportFailure,
};
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

// ---- #78: 送れない・into_inner・件数・無効の記録 ----

struct IntrospectDown(ScriptedDeliveryPorts);

impl DeliveryPorts for IntrospectDown {
    fn introspect(
        &mut self,
        _: &IntrospectionRequest,
        _: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure> {
        Err(TransportFailure)
    }
    fn query_validity(&mut self, r: &ValidityRequest) -> Result<ValidityAnswer, TransportFailure> {
        self.0.query_validity(r)
    }
    fn query_hub(&mut self, r: &HubValidityRequest) -> Result<ValidityAnswer, TransportFailure> {
        self.0.query_hub(r)
    }
}

fn valid_script() -> DeliveryScript {
    DeliveryScript {
        agent: AgentCredentialScript::Active {
            scope: "agent:run".to_owned(),
            jkt: None,
            exp: 2000,
        },
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    }
}

#[test]
fn introspection_transport_failure_is_recorded_and_others_still_run() {
    // ADLG-15・ADLG-16・AVLB-18: 送れない呼び出しも 1 件残す
    let checker = ConsoleDeliveryChecker::new(&ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "https://auth.example.test/aud/console".to_owned(),
        client_id: "console-test-client".to_owned(),
        introspection_endpoint: "https://auth.example.test/introspect".to_owned(),
        validity_endpoint: "https://console.example.test/validity".to_owned(),
        delivery_window_max: Duration::from_secs(5),
    });
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
    let mut p =
        RecordingDeliveryPorts::new(IntrospectDown(ScriptedDeliveryPorts::new(valid_script())));
    let d = checker.check(TOKEN, &ctx, 1000, &mut p);
    assert_eq!(
        d,
        Err(DeliveryFailure::Introspection(
            IntrospectionFailure::TransportFailure
        ))
    );
    let outcomes: Vec<_> = p.log().records().iter().map(|r| r.outcome).collect();
    assert_eq!(
        outcomes,
        [
            CallOutcome::TransportFailed,
            CallOutcome::Answered(ValidityAnswer::Valid),
            CallOutcome::Answered(ValidityAnswer::Valid),
        ]
    );
}

#[test]
fn invalid_answers_are_recorded_for_decider_and_hub() {
    let mut p = RecordingDeliveryPorts::new(ScriptedDeliveryPorts::new(DeliveryScript {
        decider: ValidityAnswer::Invalid,
        hub: ValidityAnswer::Invalid,
        ..valid_script()
    }));
    let _ = p.query_validity(&ValidityRequest {
        endpoint: "https://console.example.test/validity".to_owned(),
        client_id: "console-test-client".to_owned(),
        target: ValidityTarget::Session {
            sid: SID.to_owned(),
        },
    });
    let _ = p.query_hub(&HubValidityRequest {
        delegation_ref: "delegation-test".to_owned(),
    });
    let outcomes: Vec<_> = p.log().records().iter().map(|r| r.outcome).collect();
    assert_eq!(
        outcomes,
        [
            CallOutcome::Answered(ValidityAnswer::Invalid),
            CallOutcome::Answered(ValidityAnswer::Invalid),
        ]
    );
}

#[test]
fn into_inner_returns_wrapped_ports_and_count_is_per_kind() {
    let (p, _) = run(AgentCredentialScript::Inactive);
    assert_eq!(p.log().count(CallKind::Introspection), 1);
    assert_eq!(p.log().count(CallKind::DeciderValidity), 1);
    assert_eq!(p.log().count(CallKind::HubValidity), 1);
    assert_eq!(p.log().count(CallKind::DeviceToken), 0);
    let inner = p.into_inner();
    let c = inner.calls();
    assert_eq!((c.introspect, c.decider, c.hub), (1, 1, 1));
}

#[test]
fn call_kind_strings_are_distinct() {
    let kinds = [
        CallKind::DeviceAuthorization,
        CallKind::DeviceToken,
        CallKind::TokenRevocation,
        CallKind::RegistrationRevocation,
        CallKind::Introspection,
        CallKind::DeciderValidity,
        CallKind::HubValidity,
    ];
    let mut names: Vec<_> = kinds.iter().map(CallKind::as_str).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), kinds.len());
}
