//! TASK-8・#75・#78: 発行・失効の呼び出しの模擬の試験。
//! 値は試験の中だけの仮の値で、token 等は JWT 形でない試験用の不透明文字列。

use std::time::Duration;

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};
use fandhe_auth_console_stub::credential::{
    ConsoleCredentialCaller, CredentialFailure, CredentialPorts, CredentialScript,
    DeviceAuthorizationRequest, DeviceTokenRequest, ExchangeOutcome, IssuedCredential,
    IssuedTokenType, OAuthErrorCode, RegistrationRevocationRequest, RevokeAnswer,
    ScriptedCredentialPorts, StartAnswer, TokenAnswer, TokenRevocationRequest, TokenTypeHint,
};
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, ConsoleDeliveryChecker, DeliveryContext, DeliveryDecision,
    DeliveryRejection, DeliveryScript, ScriptedDeliveryPorts, ValidityAnswer, ValidityTarget,
};
use fandhe_auth_console_stub::introspection::{AuthorityRequirement, TransportFailure};
use fandhe_auth_console_stub::record::{CallKind, CallOutcome, CallRecord};

const DEVICE_CODE: &str = "opaque-device-code-0001";
const PROOF: &str = "opaque-proof-0002";
const TOKEN: &str = "opaque-token-0003";
const REG_ID: &str = "opaque-registration-0004";

fn caller() -> ConsoleCredentialCaller {
    ConsoleCredentialCaller::new(&AgentCredentialConfig {
        client_id: "agent-test-client".to_owned(),
        device_authorization_endpoint: "https://auth.example.test/device".to_owned(),
        token_endpoint: "https://auth.example.test/token".to_owned(),
        revocation_endpoint: "https://auth.example.test/revoke".to_owned(),
        registration_revocation_endpoint: "https://hub.example.test/revoke".to_owned(),
    })
}

fn issued(token_type: IssuedTokenType, jkt: Option<&str>) -> IssuedCredential {
    IssuedCredential {
        token_type,
        expires_in: 300,
        scope: vec!["agent:run".to_owned()],
        refresh_issued: true,
        cnf_jkt: jkt.map(str::to_owned),
    }
}

fn script() -> CredentialScript {
    CredentialScript {
        start: StartAnswer::Started {
            expires_in: 600,
            interval: 5,
        },
        token: TokenAnswer::Issued(issued(IssuedTokenType::DPoP, Some("jkt-test"))),
        revoke_token: RevokeAnswer::Revoked { status: 200 },
        revoke_registration: RevokeAnswer::Revoked { status: 204 },
    }
}

fn rejected(error: OAuthErrorCode) -> TokenAnswer {
    TokenAnswer::Rejected { status: 400, error }
}

#[test]
fn start_then_exchange_issues_dpop_bound_credential() {
    // DGRT-8・AGCR-1
    let mut c = caller();
    let mut p = ScriptedCredentialPorts::new(script());
    c.start("agent:run", Some(PROOF), &mut p).unwrap();
    let out = c.exchange(DEVICE_CODE, Some(PROOF), &mut p).unwrap();
    assert!(matches!(out, ExchangeOutcome::Issued(_)));
    let kinds: Vec<_> = c.log().records().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [CallKind::DeviceAuthorization, CallKind::DeviceToken]
    );
}

#[test]
fn exchange_pending_slow_down_and_rejections() {
    // DGRT-7・DGRT-36
    let mut c = caller();
    let mut s = script();
    let mut p = ScriptedCredentialPorts::new(s.clone());
    s.token = rejected(OAuthErrorCode::AuthorizationPending);
    p.set_script(s.clone());
    assert_eq!(
        c.exchange(DEVICE_CODE, None, &mut p),
        Ok(ExchangeOutcome::Pending)
    );
    s.token = rejected(OAuthErrorCode::SlowDown);
    p.set_script(s.clone());
    assert_eq!(
        c.exchange(DEVICE_CODE, None, &mut p),
        Ok(ExchangeOutcome::SlowDown)
    );
    for e in [
        OAuthErrorCode::AccessDenied,
        OAuthErrorCode::ExpiredToken,
        OAuthErrorCode::InvalidGrant,
    ] {
        s.token = rejected(e);
        p.set_script(s.clone());
        assert_eq!(
            c.exchange(DEVICE_CODE, None, &mut p),
            Err(CredentialFailure::Rejected {
                status: 400,
                error: Some(e)
            })
        );
    }
}

#[test]
fn bearer_issue_with_proof_is_binding_mismatch() {
    // AGCR-1・AGCR-8
    let mut c = caller();
    let mut s = script();
    s.token = TokenAnswer::Issued(issued(IssuedTokenType::Bearer, None));
    let mut p = ScriptedCredentialPorts::new(s);
    assert_eq!(
        c.exchange(DEVICE_CODE, Some(PROOF), &mut p),
        Err(CredentialFailure::BindingMismatch)
    );
}

#[test]
fn revocations_succeed_only_on_revoked_answer() {
    // OPRV-53・AGCR-43・AGCR-36
    let mut c = caller();
    let mut s = script();
    let mut p = ScriptedCredentialPorts::new(s.clone());
    assert_eq!(
        c.revoke_token(TOKEN, TokenTypeHint::AccessToken, None, &mut p),
        Ok(())
    );
    assert_eq!(c.revoke_registration(REG_ID, &mut p), Ok(()));
    s.revoke_token = RevokeAnswer::Rejected {
        status: 401,
        error: Some(OAuthErrorCode::InvalidClient),
    };
    s.revoke_registration = RevokeAnswer::Rejected {
        status: 404,
        error: None,
    };
    p.set_script(s);
    assert!(
        c.revoke_token(TOKEN, TokenTypeHint::RefreshToken, None, &mut p)
            .is_err()
    );
    assert_eq!(
        c.revoke_registration(REG_ID, &mut p),
        Err(CredentialFailure::Rejected {
            status: 404,
            error: None
        })
    );
    let outcomes: Vec<_> = c.log().records().iter().map(|r| r.outcome).collect();
    assert_eq!(
        outcomes,
        [
            CallOutcome::Responded { status: 200 },
            CallOutcome::Responded { status: 204 },
            CallOutcome::Responded { status: 401 },
            CallOutcome::Responded { status: 404 },
        ]
    );
}

struct Down;

impl CredentialPorts for Down {
    fn device_authorization(
        &mut self,
        _: &DeviceAuthorizationRequest,
        _: Option<&str>,
    ) -> Result<StartAnswer, TransportFailure> {
        Err(TransportFailure)
    }
    fn token(
        &mut self,
        _: &DeviceTokenRequest,
        _: &str,
        _: Option<&str>,
    ) -> Result<TokenAnswer, TransportFailure> {
        Err(TransportFailure)
    }
    fn revoke_token(
        &mut self,
        _: &TokenRevocationRequest,
        _: &str,
        _: Option<&str>,
    ) -> Result<RevokeAnswer, TransportFailure> {
        Err(TransportFailure)
    }
    fn revoke_registration(
        &mut self,
        _: &RegistrationRevocationRequest,
    ) -> Result<RevokeAnswer, TransportFailure> {
        Err(TransportFailure)
    }
}

#[test]
fn transport_failure_is_error_and_recorded() {
    // AVLB-18・AVLB-22 の向き
    let mut c = caller();
    assert_eq!(
        c.exchange(DEVICE_CODE, None, &mut Down),
        Err(CredentialFailure::Transport)
    );
    assert_eq!(
        c.revoke_registration(REG_ID, &mut Down),
        Err(CredentialFailure::Transport)
    );
    assert_eq!(c.log().count(CallKind::DeviceToken), 1);
    assert_eq!(
        c.log().records()[1],
        CallRecord {
            kind: CallKind::RegistrationRevocation,
            outcome: CallOutcome::TransportFailed
        }
    );
}

#[test]
fn outputs_do_not_contain_borrowed_secrets() {
    // SECR-11・SECR-23〜27
    let mut c = caller();
    let mut p = ScriptedCredentialPorts::new(script());
    let _ = c.start("agent:run", Some(PROOF), &mut p);
    let r = c.exchange(DEVICE_CODE, Some(PROOF), &mut p);
    let _ = c.revoke_token(TOKEN, TokenTypeHint::AccessToken, Some(PROOF), &mut p);
    let _ = c.revoke_registration(REG_ID, &mut p);
    let req = RegistrationRevocationRequest {
        endpoint: "https://hub.example.test/revoke".to_owned(),
        registration_id: REG_ID.to_owned(),
    };
    let text = format!(
        "{:?}{:?}{:?}{}{:?}",
        c,
        c.log(),
        r,
        CredentialFailure::BindingMismatch,
        req
    );
    for s in [DEVICE_CODE, PROOF, TOKEN, REG_ID] {
        assert!(!text.contains(s));
    }
}

// ---- #78: start の拒否・送れない、文字列の表、発行 → 失効 → 配送の流れ ----

#[test]
fn start_rejected_is_error_and_recorded() {
    // DGRT-1・AVLB-22 の向き
    let mut c = caller();
    let mut s = script();
    s.start = StartAnswer::Rejected {
        status: 400,
        error: OAuthErrorCode::InvalidClient,
    };
    let mut p = ScriptedCredentialPorts::new(s);
    assert_eq!(
        c.start("agent:run", None, &mut p),
        Err(CredentialFailure::Rejected {
            status: 400,
            error: Some(OAuthErrorCode::InvalidClient)
        })
    );
    assert_eq!(
        c.log().records(),
        [CallRecord {
            kind: CallKind::DeviceAuthorization,
            outcome: CallOutcome::Responded { status: 400 }
        }]
    );
}

#[test]
fn start_transport_failure_is_error_and_recorded() {
    // AVLB-18・AVLB-22 の向き
    let mut c = caller();
    assert_eq!(
        c.start("agent:run", Some(PROOF), &mut Down),
        Err(CredentialFailure::Transport)
    );
    assert_eq!(
        c.log().records(),
        [CallRecord {
            kind: CallKind::DeviceAuthorization,
            outcome: CallOutcome::TransportFailed
        }]
    );
}

#[test]
fn token_type_hint_strings() {
    // RFC 7009 の語彙
    assert_eq!(TokenTypeHint::AccessToken.as_str(), "access_token");
    assert_eq!(TokenTypeHint::RefreshToken.as_str(), "refresh_token");
}

#[test]
fn oauth_error_code_strings_cover_all_values() {
    // 網羅の match: 値を足したらここが壊れる
    let all = [
        OAuthErrorCode::AuthorizationPending,
        OAuthErrorCode::SlowDown,
        OAuthErrorCode::AccessDenied,
        OAuthErrorCode::ExpiredToken,
        OAuthErrorCode::InvalidGrant,
        OAuthErrorCode::InvalidDpopProof,
        OAuthErrorCode::InvalidClient,
        OAuthErrorCode::UnauthorizedClient,
        OAuthErrorCode::InvalidRequest,
        OAuthErrorCode::Other,
    ];
    let expected = |e: OAuthErrorCode| match e {
        OAuthErrorCode::AuthorizationPending => "authorization_pending",
        OAuthErrorCode::SlowDown => "slow_down",
        OAuthErrorCode::AccessDenied => "access_denied",
        OAuthErrorCode::ExpiredToken => "expired_token",
        OAuthErrorCode::InvalidGrant => "invalid_grant",
        OAuthErrorCode::InvalidDpopProof => "invalid_dpop_proof",
        OAuthErrorCode::InvalidClient => "invalid_client",
        OAuthErrorCode::UnauthorizedClient => "unauthorized_client",
        OAuthErrorCode::InvalidRequest => "invalid_request",
        OAuthErrorCode::Other => "other",
    };
    for e in all {
        assert_eq!(e.as_str(), expected(e));
    }
}

#[test]
fn credential_failure_display_has_no_values() {
    // SECR-23〜27
    let all = [
        CredentialFailure::Rejected {
            status: 400,
            error: Some(OAuthErrorCode::InvalidGrant),
        },
        CredentialFailure::BindingMismatch,
        CredentialFailure::Transport,
    ];
    let texts: Vec<String> = all.iter().map(|f| f.to_string()).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(a.starts_with("credential call: "), "{a}");
        assert!(!a.contains("400") && !a.contains("invalid_grant"));
        for b in &texts[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn issue_revoke_then_delivery_rejects() {
    // ADLG-11（PoC-11 手順 3 の (i)）: 発行 → 失効 → 配送の時点の確認で拒否。
    // stub どうしは自動で連動しない。失効を配送の口へ反映するのは試験側（ここ）の責務。
    let mut c = caller();
    let mut p = ScriptedCredentialPorts::new(script());
    c.start("agent:run", Some(PROOF), &mut p).unwrap();
    assert!(matches!(
        c.exchange(DEVICE_CODE, Some(PROOF), &mut p),
        Ok(ExchangeOutcome::Issued(_))
    ));

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
            bound_jkt: Some("jkt-test".to_owned()),
        },
        decider: ValidityTarget::Session {
            sid: "opaque-sid-0005".to_owned(),
        },
        delegation_ref: "delegation-test".to_owned(),
    };
    let mut script_d = DeliveryScript {
        agent: AgentCredentialScript::Active {
            scope: "agent:run".to_owned(),
            jkt: Some("jkt-test".to_owned()),
            exp: 2000,
        },
        decider: ValidityAnswer::Valid,
        hub: ValidityAnswer::Valid,
    };
    let mut dp = ScriptedDeliveryPorts::new(script_d.clone());
    assert_eq!(
        checker.check(TOKEN, &ctx, 1000, &mut dp),
        Ok(DeliveryDecision::Delivered { checked_at: 1000 })
    );

    c.revoke_token(TOKEN, TokenTypeHint::AccessToken, Some(PROOF), &mut p)
        .unwrap();
    script_d.agent = AgentCredentialScript::Inactive;
    dp.set_script(script_d);
    assert_eq!(
        checker.check(TOKEN, &ctx, 1001, &mut dp),
        Ok(DeliveryDecision::Rejected(
            DeliveryRejection::IntrospectInactive
        ))
    );

    let kinds: Vec<_> = c.log().records().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            CallKind::DeviceAuthorization,
            CallKind::DeviceToken,
            CallKind::TokenRevocation
        ]
    );
}
