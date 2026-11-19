//! TASK-8・#74・#78: 配送の時点の確認の模擬の試験。
//! 値は試験の中だけの仮の値で、token は JWT 形でない試験用の不透明文字列。

use std::time::Duration;

use fandhe_auth_console_stub::config::ConsoleStubConfig;
use fandhe_auth_console_stub::delivery::{
    AgentCredentialScript, ConsoleDeliveryChecker, DeliveryContext, DeliveryDecision,
    DeliveryFailure, DeliveryPorts, DeliveryRejection, DeliveryScript, HubValidityRequest,
    ScriptedDeliveryPorts, ValidityAnswer, ValidityRequest, ValidityTarget,
};
use fandhe_auth_console_stub::introspection::{
    AuthorityRequirement, IntrospectionFailure, IntrospectionHttpResponse, IntrospectionRequest,
    TransportFailure,
};

use ValidityAnswer::{Invalid, Valid};

const TOKEN: &str = "opaque-test-token-0001";
const NOW: u64 = 1000;

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
            required_scope: "agent:test".to_owned(),
            bound_jkt: Some("jkt-test".to_owned()),
        },
        decider: ValidityTarget::Session {
            sid: "sid-test".to_owned(),
        },
        delegation_ref: "delegation-test".to_owned(),
    }
}

fn script(agent_ok: bool, decider: ValidityAnswer, hub: ValidityAnswer) -> DeliveryScript {
    DeliveryScript {
        agent: if agent_ok {
            AgentCredentialScript::Active {
                scope: "agent:test".to_owned(),
                jkt: Some("jkt-test".to_owned()),
                exp: 2000,
            }
        } else {
            AgentCredentialScript::Inactive
        },
        decider,
        hub,
    }
}

#[test]
fn all_valid_delivers() {
    let mut p = ScriptedDeliveryPorts::new(script(true, Valid, Valid));
    let d = checker().check(TOKEN, &ctx(), NOW, &mut p);
    assert_eq!(d, Ok(DeliveryDecision::Delivered { checked_at: NOW }));
}

#[test]
fn each_invalid_rejects_by_config_only() {
    let cases = [
        (
            script(false, Valid, Valid),
            DeliveryRejection::IntrospectInactive,
            "introspect_inactive",
        ),
        (
            script(true, Invalid, Valid),
            DeliveryRejection::DeciderSessionInactive,
            "decider_session_inactive",
        ),
        (
            script(true, Valid, Invalid),
            DeliveryRejection::HubInvalid,
            "hub_invalid",
        ),
    ];
    for (s, rej, name) in cases {
        let mut p = ScriptedDeliveryPorts::new(s);
        let d = checker().check(TOKEN, &ctx(), NOW, &mut p);
        assert_eq!(d, Ok(DeliveryDecision::Rejected(rej)));
        assert_eq!(rej.as_str(), name);
    }
}

#[test]
fn no_cache_between_deliveries_and_all_ports_called() {
    let c = checker();
    let mut p = ScriptedDeliveryPorts::new(script(true, Valid, Valid));
    assert!(matches!(
        c.check(TOKEN, &ctx(), NOW, &mut p),
        Ok(DeliveryDecision::Delivered { .. })
    ));
    p.set_script(script(false, Invalid, Invalid));
    assert_eq!(
        c.check(TOKEN, &ctx(), NOW, &mut p),
        Ok(DeliveryDecision::Rejected(
            DeliveryRejection::IntrospectInactive
        ))
    );
    let n = p.calls();
    assert_eq!((n.introspect, n.decider, n.hub), (2, 2, 2));
}

struct Failing {
    fail_introspect: bool,
    fail_decider: bool,
    fail_hub: bool,
    last_validity: Option<ValidityRequest>,
    last_hub: Option<HubValidityRequest>,
}

impl DeliveryPorts for Failing {
    fn introspect(
        &mut self,
        _r: &IntrospectionRequest,
        _t: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure> {
        if self.fail_introspect {
            return Err(TransportFailure);
        }
        Ok(IntrospectionHttpResponse {
            status: 200,
            body: br#"{"active":true,"scope":"agent:test","token_type":"DPoP","cnf":{"jkt":"jkt-test"},"exp":2000}"#.to_vec(),
        })
    }
    fn query_validity(&mut self, r: &ValidityRequest) -> Result<ValidityAnswer, TransportFailure> {
        self.last_validity = Some(r.clone());
        if self.fail_decider {
            Err(TransportFailure)
        } else {
            Ok(Valid)
        }
    }
    fn query_hub(&mut self, r: &HubValidityRequest) -> Result<ValidityAnswer, TransportFailure> {
        self.last_hub = Some(r.clone());
        if self.fail_hub {
            Err(TransportFailure)
        } else {
            Ok(Valid)
        }
    }
}

fn failing(i: bool, d: bool, h: bool) -> Failing {
    Failing {
        fail_introspect: i,
        fail_decider: d,
        fail_hub: h,
        last_validity: None,
        last_hub: None,
    }
}

#[test]
fn port_errors_never_deliver() {
    let c = checker();
    assert!(matches!(
        c.check(TOKEN, &ctx(), NOW, &mut failing(true, false, false)),
        Err(DeliveryFailure::Introspection(_))
    ));
    assert_eq!(
        c.check(TOKEN, &ctx(), NOW, &mut failing(false, true, false)),
        Err(DeliveryFailure::DeciderValidityUnavailable)
    );
    assert_eq!(
        c.check(TOKEN, &ctx(), NOW, &mut failing(false, false, true)),
        Err(DeliveryFailure::HubValidityUnavailable)
    );
    // 比較: 全て成功する口なら成功する。
    assert!(matches!(
        c.check(TOKEN, &ctx(), NOW, &mut failing(false, false, false)),
        Ok(DeliveryDecision::Delivered { .. })
    ));
}

#[test]
fn requests_carry_config_and_context() {
    let c = checker();
    let mut p = failing(false, false, false);
    let _ = c.check(TOKEN, &ctx(), NOW, &mut p);
    let v = p.last_validity.clone().expect("validity called");
    assert_eq!(v.endpoint, "https://console.example.test/validity");
    assert_eq!(v.client_id, "console-test-client");
    assert_eq!(
        v.target,
        ValidityTarget::Session {
            sid: "sid-test".to_owned()
        }
    );
    assert_eq!(
        p.last_hub.clone().expect("hub called").delegation_ref,
        "delegation-test"
    );

    let mut ctx2 = ctx();
    ctx2.decider = ValidityTarget::Registration {
        registration_id: "reg-test".to_owned(),
    };
    let _ = c.check(TOKEN, &ctx2, NOW, &mut p);
    assert_eq!(
        p.last_validity.expect("validity called").target,
        ValidityTarget::Registration {
            registration_id: "reg-test".to_owned()
        }
    );
}

#[test]
fn token_not_in_debug_or_display() {
    let c = checker();
    let mut p = ScriptedDeliveryPorts::new(script(true, Valid, Valid));
    let d = c.check(TOKEN, &ctx(), NOW, &mut p);
    let e = DeliveryFailure::Introspection(IntrospectionFailure::Unavailable);
    let out = format!("{c:?}{p:?}{d:?}{e:?}{e}{:?}", ctx());
    assert!(!out.contains(TOKEN));
}

// ---- #78: 文字列の表と Debug の網羅 ----

#[test]
fn delivery_failure_display_is_distinct() {
    let all = [
        DeliveryFailure::Introspection(IntrospectionFailure::Unavailable),
        DeliveryFailure::DeciderValidityUnavailable,
        DeliveryFailure::HubValidityUnavailable,
    ];
    let texts: Vec<String> = all.iter().map(|f| f.to_string()).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(a.starts_with("delivery check: "), "{a}");
        for b in &texts[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn rejection_vocabulary_is_fixed() {
    // ADLG-11・ADLG-12・ADLG-31: PoC-11 の語彙
    assert_eq!(
        DeliveryRejection::IntrospectInactive.as_str(),
        "introspect_inactive"
    );
    assert_eq!(
        DeliveryRejection::DeciderSessionInactive.as_str(),
        "decider_session_inactive"
    );
    assert_eq!(DeliveryRejection::HubInvalid.as_str(), "hub_invalid");
}

#[test]
fn validity_debug_hides_identifiers() {
    // SECR-23〜27: sid・登録 ID は種別だけを出す
    let reg = ValidityTarget::Registration {
        registration_id: "reg-secret-test".to_owned(),
    };
    let sess = ValidityTarget::Session {
        sid: "sid-secret-test".to_owned(),
    };
    let req = ValidityRequest {
        endpoint: "https://console.example.test/validity".to_owned(),
        client_id: "console-test-client".to_owned(),
        target: reg.clone(),
    };
    let text = format!("{reg:?}{sess:?}{req:?}");
    assert!(text.contains("Registration") && text.contains("Session"));
    assert!(!text.contains("reg-secret-test") && !text.contains("sid-secret-test"));
}
