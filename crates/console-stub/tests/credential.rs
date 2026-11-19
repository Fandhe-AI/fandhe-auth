//! TASK-8・#75: 発行・失効の呼び出しの模擬の最小の確認（網羅は #78）。
//! 値は試験の中だけの仮の値で、token 等は JWT 形でない試験用の不透明文字列。

use fandhe_auth_console_stub::config::AgentCredentialConfig;
use fandhe_auth_console_stub::credential::{
    ConsoleCredentialCaller, CredentialFailure, CredentialPorts, CredentialScript,
    DeviceAuthorizationRequest, DeviceTokenRequest, ExchangeOutcome, IssuedCredential,
    IssuedTokenType, OAuthErrorCode, RegistrationRevocationRequest, RevokeAnswer,
    ScriptedCredentialPorts, StartAnswer, TokenAnswer, TokenRevocationRequest, TokenTypeHint,
};
use fandhe_auth_console_stub::introspection::TransportFailure;
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
