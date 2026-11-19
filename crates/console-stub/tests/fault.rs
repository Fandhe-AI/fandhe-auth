//! TASK-8・#76: 応答の遅れと拒否の注入の最小の確認（網羅は #78）。値は試験用の仮の値。

use std::time::Duration;

use fandhe_auth_console_stub::config::{
    AgentCredentialConfig, CallWindow, ConsoleStubConfig, CredentialFaultConfig, CredentialRefusal,
    DeliveryFaultConfig, FaultRule, IntrospectionRefusal, ValidityRefusal,
};
use fandhe_auth_console_stub::credential::{
    ConsoleCredentialCaller, CredentialFailure, CredentialScript, IssuedCredential,
    IssuedTokenType, OAuthErrorCode, RevokeAnswer, ScriptedCredentialPorts, StartAnswer,
    TokenAnswer,
};
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, ConsoleDeliveryChecker, DeliveryContext, DeliveryDecision,
    DeliveryFailure, DeliveryScript, ScriptedDeliveryPorts, ValidityAnswer, ValidityTarget,
};
use fandhe_auth_console_stub::fault::{
    FaultInjectingCredentialPorts, FaultInjectingDeliveryPorts, RefusalRecord,
};
use fandhe_auth_console_stub::introspection::{AuthorityRequirement, IntrospectionFailure};
use fandhe_auth_console_stub::record::{CallKind, CallOutcome, RecordingDeliveryPorts};

const TOKEN: &str = "opaque-test-token-0001";
const DEVICE_CODE: &str = "opaque-device-code-0003";
const PROOF: &str = "opaque-proof-0004";

fn checker() -> ConsoleDeliveryChecker {
    ConsoleDeliveryChecker::new(&ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "https://auth.example.test/aud/console".to_owned(),
        client_id: "console-test-client".to_owned(),
        introspection_endpoint: "https://auth.example.test/introspect".to_owned(),
        validity_endpoint: "https://console.example.test/validity".to_owned(),
        delivery_window_max: Duration::from_secs(5),
    })
}

fn ctx() -> DeliveryContext {
    DeliveryContext {
        requirement: AuthorityRequirement {
            required_scope: "agent:run".to_owned(),
            bound_jkt: None,
        },
        decider: ValidityTarget::Session {
            sid: "opaque-sid-0002".to_owned(),
        },
        delegation_ref: "delegation-test".to_owned(),
    }
}

fn scripted() -> ScriptedDeliveryPorts {
    ScriptedDeliveryPorts::new(DeliveryScript {
        agent: AgentCredentialScript::Active {
            scope: "agent:run".to_owned(),
            jkt: None,
            exp: 2000,
        },
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    })
}

fn always<R>(delay: Option<Duration>, refusal: Option<R>) -> Vec<FaultRule<R>> {
    vec![FaultRule {
        calls: CallWindow {
            first: 1,
            last: None,
        },
        delay,
        refusal,
    }]
}

#[test]
fn baseline_passes_through_and_logs_without_injection() {
    let mut p = FaultInjectingDeliveryPorts::new(
        scripted(),
        DeliveryFaultConfig::baseline(),
        |_: Duration| {},
    );
    let d = checker().check(TOKEN, &ctx(), 1000, &mut p);
    assert_eq!(d, Ok(DeliveryDecision::Delivered { checked_at: 1000 }));
    let recs = p.log().records();
    assert_eq!(recs.len(), 3);
    assert!(recs.iter().all(|r| r.injected.is_none()));
}

#[test]
fn introspection_503_fails_but_other_checks_still_run() {
    // ADLG-15・ADLG-16
    let faults = DeliveryFaultConfig {
        introspection: always(None, Some(IntrospectionRefusal::Status(503))),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    let d = checker().check(TOKEN, &ctx(), 1000, &mut p);
    assert_eq!(
        d,
        Err(DeliveryFailure::Introspection(
            IntrospectionFailure::Unavailable
        ))
    );
    let c = p.inner().calls();
    assert_eq!((c.introspect, c.decider, c.hub), (0, 1, 1));
}

#[test]
fn introspection_401_maps_to_client_auth_rejected() {
    let faults = DeliveryFaultConfig {
        introspection: always(None, Some(IntrospectionRefusal::Status(401))),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    let d = checker().check(TOKEN, &ctx(), 1000, &mut p);
    assert_eq!(
        d,
        Err(DeliveryFailure::Introspection(
            IntrospectionFailure::ClientAuthRejected
        ))
    );
}

#[test]
fn hub_unreachable_only_in_window() {
    let faults = DeliveryFaultConfig {
        hub: vec![FaultRule {
            calls: CallWindow {
                first: 2,
                last: Some(2),
            },
            delay: None,
            refusal: Some(ValidityRefusal::Unreachable),
        }],
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    let c = checker();
    assert!(matches!(
        c.check(TOKEN, &ctx(), 1000, &mut p),
        Ok(DeliveryDecision::Delivered { .. })
    ));
    assert_eq!(
        c.check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::HubValidityUnavailable)
    );
    assert!(matches!(
        c.check(TOKEN, &ctx(), 1000, &mut p),
        Ok(DeliveryDecision::Delivered { .. })
    ));
}

#[test]
fn decider_unreachable_fails() {
    let faults = DeliveryFaultConfig {
        decider: always(None, Some(ValidityRefusal::Unreachable)),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    assert_eq!(
        checker().check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::DeciderValidityUnavailable)
    );
}

#[test]
fn delay_only_hands_duration_to_hook_and_keeps_result() {
    let delay = Duration::from_millis(250);
    let faults = DeliveryFaultConfig {
        hub: always::<ValidityRefusal>(Some(delay), None),
        ..DeliveryFaultConfig::baseline()
    };
    let mut seen = Vec::new();
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |d: Duration| seen.push(d));
    let d = checker().check(TOKEN, &ctx(), 1000, &mut p);
    assert_eq!(d, Ok(DeliveryDecision::Delivered { checked_at: 1000 }));
    let injected = p.log().records()[2].injected.expect("injected");
    assert_eq!(injected.delay, Some(delay));
    assert_eq!(injected.refusal, None);
    drop(p);
    assert_eq!(seen, [delay]);
}

#[test]
fn stacked_with_recording_logs_injected_outcome() {
    let faults = DeliveryFaultConfig {
        introspection: always(None, Some(IntrospectionRefusal::Status(503))),
        hub: always(None, Some(ValidityRefusal::Unreachable)),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = RecordingDeliveryPorts::new(FaultInjectingDeliveryPorts::new(
        scripted(),
        faults,
        |_: Duration| {},
    ));
    let _ = checker().check(TOKEN, &ctx(), 1000, &mut p);
    let recs = p.log().records();
    assert_eq!(recs[0].kind, CallKind::Introspection);
    assert_eq!(recs[0].outcome, CallOutcome::Responded { status: 503 });
    assert_eq!(recs[2].outcome, CallOutcome::TransportFailed);
    let inj = p.inner().log().records();
    assert_eq!(
        inj[0].injected.unwrap().refusal,
        Some(RefusalRecord::Status(503))
    );
}

fn cred_caller() -> ConsoleCredentialCaller {
    ConsoleCredentialCaller::new(&AgentCredentialConfig {
        client_id: "agent-test-client".to_owned(),
        device_authorization_endpoint: "https://auth.example.test/device".to_owned(),
        token_endpoint: "https://auth.example.test/token".to_owned(),
        revocation_endpoint: "https://auth.example.test/revoke".to_owned(),
        registration_revocation_endpoint: "https://auth.example.test/reg/revoke".to_owned(),
    })
}

fn cred_script() -> ScriptedCredentialPorts {
    ScriptedCredentialPorts::new(CredentialScript {
        start: StartAnswer::Started {
            expires_in: 600,
            interval: 5,
        },
        token: TokenAnswer::Issued(IssuedCredential {
            token_type: IssuedTokenType::DPoP,
            expires_in: 300,
            scope: vec!["agent:run".to_owned()],
            refresh_issued: false,
            cnf_jkt: Some("jkt-test".to_owned()),
        }),
        revoke_token: RevokeAnswer::Revoked { status: 200 },
        revoke_registration: RevokeAnswer::Revoked { status: 200 },
    })
}

#[test]
fn credential_token_rejected_and_unreachable() {
    let faults = CredentialFaultConfig {
        token: vec![
            FaultRule {
                calls: CallWindow {
                    first: 1,
                    last: Some(1),
                },
                delay: None,
                refusal: Some(CredentialRefusal::Rejected {
                    status: 400,
                    error: OAuthErrorCode::InvalidGrant,
                }),
            },
            FaultRule {
                calls: CallWindow {
                    first: 2,
                    last: Some(2),
                },
                delay: None,
                refusal: Some(CredentialRefusal::Unreachable),
            },
        ],
        ..CredentialFaultConfig::baseline()
    };
    let mut ports = FaultInjectingCredentialPorts::new(cred_script(), faults, |_: Duration| {});
    let mut caller = cred_caller();
    assert_eq!(
        caller.exchange(DEVICE_CODE, Some(PROOF), &mut ports).err(),
        Some(CredentialFailure::Rejected {
            status: 400,
            error: Some(OAuthErrorCode::InvalidGrant)
        })
    );
    assert_eq!(
        caller.exchange(DEVICE_CODE, Some(PROOF), &mut ports).err(),
        Some(CredentialFailure::Transport)
    );
    assert!(
        caller
            .exchange(DEVICE_CODE, Some(PROOF), &mut ports)
            .is_ok()
    );
    assert_eq!(caller.log().count(CallKind::DeviceToken), 3);
    assert_eq!(ports.log().records().len(), 3);
}

#[test]
fn revoke_rejection_is_failure() {
    let faults = CredentialFaultConfig {
        revoke_token: always(
            None,
            Some(CredentialRefusal::Rejected {
                status: 503,
                error: OAuthErrorCode::Other,
            }),
        ),
        ..CredentialFaultConfig::baseline()
    };
    let mut ports = FaultInjectingCredentialPorts::new(cred_script(), faults, |_: Duration| {});
    let mut caller = cred_caller();
    let r = caller.revoke_token(
        TOKEN,
        fandhe_auth_console_stub::credential::TokenTypeHint::AccessToken,
        None,
        &mut ports,
    );
    assert!(matches!(
        r,
        Err(CredentialFailure::Rejected { status: 503, .. })
    ));
}

#[test]
fn debug_output_has_no_secret_values() {
    let mut p = FaultInjectingCredentialPorts::new(
        cred_script(),
        CredentialFaultConfig::baseline(),
        |_: Duration| {},
    );
    let mut caller = cred_caller();
    let _ = caller.exchange(DEVICE_CODE, Some(PROOF), &mut p);
    let s = format!("{p:?}");
    assert!(!s.contains(DEVICE_CODE) && !s.contains(PROOF));

    let mut d = FaultInjectingDeliveryPorts::new(
        scripted(),
        DeliveryFaultConfig::baseline(),
        |_: Duration| {},
    );
    let _ = checker().check(TOKEN, &ctx(), 1000, &mut d);
    let s = format!("{d:?}");
    assert!(!s.contains(TOKEN) && !s.contains("opaque-sid-0002"));
}
