//! TASK-8・#76・#78: 応答の遅れと拒否の注入の試験。値は試験用の仮の値。

use std::time::Duration;

use fandhe_auth_console_stub::config::{
    AgentCredentialConfig, CallWindow, ConsoleStubConfig, CredentialFaultConfig, CredentialRefusal,
    DeliveryFaultConfig, FaultRule, IntrospectionRefusal, ValidityRefusal,
};
use fandhe_auth_console_stub::credential::{
    ConsoleCredentialCaller, CredentialFailure, CredentialScript, ExchangeOutcome,
    IssuedCredential, IssuedTokenType, OAuthErrorCode, RevokeAnswer, ScriptedCredentialPorts,
    StartAnswer, TokenAnswer,
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

#[test]
fn call_window_first_zero_behaves_like_first_one() {
    let zero = CallWindow {
        first: 0,
        last: Some(1),
    };
    let one = CallWindow {
        first: 1,
        last: Some(1),
    };
    for n in 0..=2 {
        assert_eq!(zero.contains(n), one.contains(n), "n = {n}");
    }
    assert!(!zero.contains(0));
    assert!(zero.contains(1));
}

// ---- #78: 規則なし・遅れのみ・拒否・窓・切り替え・into_inner の網羅 ----

const REG_ID: &str = "opaque-registration-0005";
const DELAY: Duration = Duration::from_millis(250);

fn rejected(status: u16, error: OAuthErrorCode) -> CredentialRefusal {
    CredentialRefusal::Rejected { status, error }
}

#[test]
fn start_without_rule_passes_through_and_logs_none() {
    // 基準線: 注入なしは素通し。記録は全呼び出しで 1 件
    let mut p = FaultInjectingCredentialPorts::new(
        cred_script(),
        CredentialFaultConfig::baseline(),
        |_: Duration| {},
    );
    let mut caller = cred_caller();
    let s = caller.start("agent:run", None, &mut p).unwrap();
    assert_eq!((s.expires_in, s.interval), (600, 5));
    let r = p.log().records();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].kind, CallKind::DeviceAuthorization);
    assert_eq!(r[0].call_index, 1);
    assert!(r[0].injected.is_none());
}

#[test]
fn start_rejected_and_unreachable_are_failures() {
    // AVLB-18・AVLB-22: 注入で成功は作れない
    let faults = CredentialFaultConfig {
        device_authorization: vec![
            FaultRule {
                calls: CallWindow {
                    first: 1,
                    last: Some(1),
                },
                delay: None,
                refusal: Some(rejected(400, OAuthErrorCode::InvalidClient)),
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
    let mut p = FaultInjectingCredentialPorts::new(cred_script(), faults, |_: Duration| {});
    let mut c = cred_caller();
    assert_eq!(
        c.start("agent:run", None, &mut p),
        Err(CredentialFailure::Rejected {
            status: 400,
            error: Some(OAuthErrorCode::InvalidClient)
        })
    );
    assert_eq!(
        c.start("agent:run", None, &mut p),
        Err(CredentialFailure::Transport)
    );
    assert!(c.start("agent:run", None, &mut p).is_ok());
    let idx: Vec<_> = p.log().records().iter().map(|r| r.call_index).collect();
    assert_eq!(idx, [1, 2, 3]);
    assert_eq!(
        p.log().records()[0].injected.unwrap().refusal,
        Some(RefusalRecord::Status(400))
    );
    assert_eq!(
        p.log().records()[1].injected.unwrap().refusal,
        Some(RefusalRecord::Unreachable)
    );
}

#[test]
fn credential_delay_only_keeps_results_on_every_port() {
    // 遅れのみは結果を変えず、フックへ値を渡す（THRT-29・TASK-30: 値は仮）
    let faults = CredentialFaultConfig {
        device_authorization: always::<CredentialRefusal>(Some(DELAY), None),
        token: always::<CredentialRefusal>(Some(DELAY), None),
        revoke_token: always::<CredentialRefusal>(Some(DELAY), None),
        revoke_registration: always::<CredentialRefusal>(Some(DELAY), None),
    };
    let mut seen = Vec::new();
    let mut p =
        FaultInjectingCredentialPorts::new(cred_script(), faults, |d: Duration| seen.push(d));
    let mut c = cred_caller();
    assert!(c.start("agent:run", None, &mut p).is_ok());
    assert!(matches!(
        c.exchange(DEVICE_CODE, Some(PROOF), &mut p),
        Ok(ExchangeOutcome::Issued(_))
    ));
    assert!(
        c.revoke_token(
            TOKEN,
            fandhe_auth_console_stub::credential::TokenTypeHint::AccessToken,
            None,
            &mut p
        )
        .is_ok()
    );
    assert!(c.revoke_registration(REG_ID, &mut p).is_ok());
    for r in p.log().records() {
        let inj = r.injected.expect("injected");
        assert_eq!((inj.delay, inj.refusal), (Some(DELAY), None));
    }
    drop(p);
    assert_eq!(seen, [DELAY; 4]);
}

#[test]
fn revoke_registration_passes_through_rejects_and_unreachable() {
    // AGCR-35・AGCR-36: 失効の拒否・送れないは Err
    let mut p = FaultInjectingCredentialPorts::new(
        cred_script(),
        CredentialFaultConfig::baseline(),
        |_: Duration| {},
    );
    let mut c = cred_caller();
    assert_eq!(c.revoke_registration(REG_ID, &mut p), Ok(()));

    p.set_faults(CredentialFaultConfig {
        revoke_registration: always(None, Some(rejected(503, OAuthErrorCode::Other))),
        ..CredentialFaultConfig::baseline()
    });
    assert_eq!(
        c.revoke_registration(REG_ID, &mut p),
        Err(CredentialFailure::Rejected {
            status: 503,
            error: Some(OAuthErrorCode::Other)
        })
    );

    p.set_faults(CredentialFaultConfig {
        revoke_registration: always(Some(DELAY), Some(CredentialRefusal::Unreachable)),
        ..CredentialFaultConfig::baseline()
    });
    assert_eq!(
        c.revoke_registration(REG_ID, &mut p),
        Err(CredentialFailure::Transport)
    );
    // 回数は set_faults を跨いで通算
    let idx: Vec<_> = p.log().records().iter().map(|r| r.call_index).collect();
    assert_eq!(idx, [1, 2, 3]);
    assert_eq!(p.log().records()[2].kind, CallKind::RegistrationRevocation);
}

#[test]
fn revoke_token_window_and_unreachable() {
    // 窓の外は素通し、窓の中は拒否、Unreachable は送れない
    let faults = CredentialFaultConfig {
        revoke_token: vec![
            FaultRule {
                calls: CallWindow {
                    first: 2,
                    last: Some(2),
                },
                delay: None,
                refusal: Some(rejected(401, OAuthErrorCode::InvalidClient)),
            },
            FaultRule {
                calls: CallWindow {
                    first: 3,
                    last: Some(3),
                },
                delay: None,
                refusal: Some(CredentialRefusal::Unreachable),
            },
        ],
        ..CredentialFaultConfig::baseline()
    };
    let mut p = FaultInjectingCredentialPorts::new(cred_script(), faults, |_: Duration| {});
    let mut c = cred_caller();
    let hint = fandhe_auth_console_stub::credential::TokenTypeHint::AccessToken;
    assert_eq!(c.revoke_token(TOKEN, hint, None, &mut p), Ok(()));
    assert!(matches!(
        c.revoke_token(TOKEN, hint, None, &mut p),
        Err(CredentialFailure::Rejected { status: 401, .. })
    ));
    assert_eq!(
        c.revoke_token(TOKEN, hint, None, &mut p),
        Err(CredentialFailure::Transport)
    );
    assert_eq!(c.revoke_token(TOKEN, hint, None, &mut p), Ok(()));
}

#[test]
fn token_delay_only_passes_delay_and_keeps_issued() {
    let faults = CredentialFaultConfig {
        token: always::<CredentialRefusal>(Some(DELAY), None),
        ..CredentialFaultConfig::baseline()
    };
    let mut seen = Vec::new();
    let mut p =
        FaultInjectingCredentialPorts::new(cred_script(), faults, |d: Duration| seen.push(d));
    let mut c = cred_caller();
    assert!(matches!(
        c.exchange(DEVICE_CODE, Some(PROOF), &mut p),
        Ok(ExchangeOutcome::Issued(_))
    ));
    drop(p);
    assert_eq!(seen, [DELAY]);
}

#[test]
fn first_matching_rule_wins() {
    // 規則は先頭の当たった 1 件だけ
    let faults = CredentialFaultConfig {
        token: vec![
            FaultRule {
                calls: CallWindow {
                    first: 1,
                    last: None,
                },
                delay: None,
                refusal: Some(rejected(400, OAuthErrorCode::AccessDenied)),
            },
            FaultRule {
                calls: CallWindow {
                    first: 1,
                    last: None,
                },
                delay: None,
                refusal: Some(CredentialRefusal::Unreachable),
            },
        ],
        ..CredentialFaultConfig::baseline()
    };
    let mut p = FaultInjectingCredentialPorts::new(cred_script(), faults, |_: Duration| {});
    let mut c = cred_caller();
    assert_eq!(
        c.exchange(DEVICE_CODE, None, &mut p),
        Err(CredentialFailure::Rejected {
            status: 400,
            error: Some(OAuthErrorCode::AccessDenied)
        })
    );
}

#[test]
fn credential_set_faults_and_into_inner() {
    let mut p = FaultInjectingCredentialPorts::new(
        cred_script(),
        CredentialFaultConfig {
            token: always(None, Some(CredentialRefusal::Unreachable)),
            ..CredentialFaultConfig::baseline()
        },
        |_: Duration| {},
    );
    let mut c = cred_caller();
    assert_eq!(
        c.exchange(DEVICE_CODE, None, &mut p),
        Err(CredentialFailure::Transport)
    );
    p.set_faults(CredentialFaultConfig::baseline());
    assert!(c.exchange(DEVICE_CODE, None, &mut p).is_ok());
    assert_eq!(p.log().records()[1].call_index, 2);
    assert!(p.log().records()[1].injected.is_none());
    // 内側の口が取り出せ、注入を通さずに直接使える
    let mut inner = p.into_inner();
    let mut c2 = cred_caller();
    assert!(c2.exchange(DEVICE_CODE, None, &mut inner).is_ok());
}

#[test]
fn introspection_unreachable_fails_but_other_checks_still_run() {
    // ADLG-15・ADLG-16・AVLB-22
    let faults = DeliveryFaultConfig {
        introspection: always(None, Some(IntrospectionRefusal::Unreachable)),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    assert_eq!(
        checker().check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::Introspection(
            IntrospectionFailure::TransportFailure
        ))
    );
    let c = p.inner().calls();
    assert_eq!((c.introspect, c.decider, c.hub), (0, 1, 1));
    assert_eq!(
        p.log().records()[0].injected.unwrap().refusal,
        Some(RefusalRecord::Unreachable)
    );
}

#[test]
fn introspection_status_200_never_succeeds() {
    // AVLB-18・AVLB-22: Status(200) は本文が空で MalformedResponse
    let faults = DeliveryFaultConfig {
        introspection: always(None, Some(IntrospectionRefusal::Status(200))),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    assert_eq!(
        checker().check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::Introspection(
            IntrospectionFailure::MalformedResponse
        ))
    );
}

#[test]
fn introspection_and_decider_delay_only_keep_delivered() {
    let faults = DeliveryFaultConfig {
        introspection: always::<IntrospectionRefusal>(Some(DELAY), None),
        decider: always::<ValidityRefusal>(Some(DELAY * 2), None),
        ..DeliveryFaultConfig::baseline()
    };
    let mut seen = Vec::new();
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |d: Duration| seen.push(d));
    assert_eq!(
        checker().check(TOKEN, &ctx(), 1000, &mut p),
        Ok(DeliveryDecision::Delivered { checked_at: 1000 })
    );
    let c = p.inner().calls();
    assert_eq!((c.introspect, c.decider, c.hub), (1, 1, 1));
    drop(p);
    assert_eq!(seen, [DELAY, DELAY * 2]);
}

#[test]
fn delay_with_refusal_skips_inner_port() {
    // 遅れ + 拒否: フックは呼ばれ、内側の口は呼ばれない
    let faults = DeliveryFaultConfig {
        hub: always(Some(DELAY), Some(ValidityRefusal::Unreachable)),
        ..DeliveryFaultConfig::baseline()
    };
    let mut seen = Vec::new();
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |d: Duration| seen.push(d));
    assert_eq!(
        checker().check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::HubValidityUnavailable)
    );
    assert_eq!(p.inner().calls().hub, 0);
    drop(p);
    assert_eq!(seen, [DELAY]);
}

#[test]
fn delivery_set_faults_and_into_inner() {
    let faults = DeliveryFaultConfig {
        decider: always(None, Some(ValidityRefusal::Unreachable)),
        ..DeliveryFaultConfig::baseline()
    };
    let mut p = FaultInjectingDeliveryPorts::new(scripted(), faults, |_: Duration| {});
    let c = checker();
    assert_eq!(
        c.check(TOKEN, &ctx(), 1000, &mut p),
        Err(DeliveryFailure::DeciderValidityUnavailable)
    );
    p.set_faults(DeliveryFaultConfig::baseline());
    assert!(matches!(
        c.check(TOKEN, &ctx(), 1000, &mut p),
        Ok(DeliveryDecision::Delivered { .. })
    ));
    let idx: Vec<_> = p
        .log()
        .records()
        .iter()
        .filter(|r| r.kind == CallKind::DeciderValidity)
        .map(|r| r.call_index)
        .collect();
    assert_eq!(idx, [1, 2]);
    let inner = p.into_inner();
    let n = inner.calls();
    // 1 回目は ② が送れず、② だけ内側に届かない
    assert_eq!((n.introspect, n.decider, n.hub), (2, 1, 2));
}
