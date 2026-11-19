//! TASK-8・#73: introspection の呼び出しの模擬の最小の確認（網羅は #78）。
//! 値は試験の中だけの仮の値で、token は JWT 形でない試験用の不透明文字列。

use std::cell::Cell;
use std::time::Duration;

use fandhe_auth_console_stub::config::ConsoleStubConfig;
use fandhe_auth_console_stub::introspection::{
    AuthorityRequirement, ConsoleIntrospector, CurrentAuthority, IntrospectionFailure,
    IntrospectionHttpResponse, TransportFailure,
};

const TOKEN: &str = "opaque-test-token-0001";

fn introspector() -> ConsoleIntrospector {
    ConsoleIntrospector::new(&ConsoleStubConfig {
        issuer: "https://auth.example.test".to_owned(),
        audience: "https://auth.example.test/aud/console".to_owned(),
        client_id: "console-test-client".to_owned(),
        introspection_endpoint: "https://auth.example.test/introspect".to_owned(),
        validity_endpoint: "https://console.example.test/validity".to_owned(),
        delivery_window_max: Duration::from_secs(5),
    })
}

fn req(bound: bool) -> AuthorityRequirement {
    AuthorityRequirement {
        required_scope: "agent:test".to_owned(),
        bound_jkt: bound.then(|| "jkt-test".to_owned()),
    }
}

fn resp(status: u16, body: &str) -> Result<IntrospectionHttpResponse, TransportFailure> {
    Ok(IntrospectionHttpResponse {
        status,
        body: body.as_bytes().to_vec(),
    })
}

const ACTIVE: &str = r#"{"active":true,"scope":"a agent:test","token_type":"DPoP","cnf":{"jkt":"jkt-test"},"exp":2000,"client_id":"console-test-client","https://auth.example.test/claims/read_marker":"x"}"#;

#[test]
fn active_with_binding_is_valid() {
    // ADLG-15・AGCR-32・OPRV-72
    let r = introspector().check(TOKEN, &req(true), 1000, |_, _| resp(200, ACTIVE));
    match r.unwrap() {
        CurrentAuthority::Valid(g) => {
            assert_eq!(g.cnf_jkt.as_deref(), Some("jkt-test"));
            assert_eq!(g.exp, Some(2000));
            assert!(g.scope.contains(&"agent:test".to_owned()));
        }
        CurrentAuthority::Invalid => panic!("expected valid"),
    }
}

#[test]
fn inactive_is_invalid() {
    // AGCR-32・OPRV-55
    let r = introspector().check(TOKEN, &req(false), 1000, |_, _| {
        resp(200, r#"{"active":false}"#)
    });
    assert_eq!(r, Ok(CurrentAuthority::Invalid));
}

#[test]
fn result_changes_and_is_not_cached() {
    // ADLG-15: 配送ごとに最初から確かめる
    let i = introspector();
    let calls = Cell::new(0);
    let first = i.check(TOKEN, &req(true), 1000, |_, _| {
        calls.set(calls.get() + 1);
        resp(200, ACTIVE)
    });
    let second = i.check(TOKEN, &req(true), 1000, |_, _| {
        calls.set(calls.get() + 1);
        resp(200, r#"{"active":false}"#)
    });
    assert!(matches!(first, Ok(CurrentAuthority::Valid(_))));
    assert_eq!(second, Ok(CurrentAuthority::Invalid));
    assert_eq!(calls.get(), 2);
}

#[test]
fn unmet_requirements_are_invalid() {
    let i = introspector();
    let wrong_scope = AuthorityRequirement {
        required_scope: "other".to_owned(),
        bound_jkt: None,
    };
    assert_eq!(
        i.check(TOKEN, &wrong_scope, 1000, |_, _| resp(200, ACTIVE)),
        Ok(CurrentAuthority::Invalid)
    );
    let wrong_jkt = AuthorityRequirement {
        required_scope: "agent:test".to_owned(),
        bound_jkt: Some("other".to_owned()),
    };
    assert_eq!(
        i.check(TOKEN, &wrong_jkt, 1000, |_, _| resp(200, ACTIVE)),
        Ok(CurrentAuthority::Invalid)
    );
    // SIGN-17 の向き: exp == now
    assert_eq!(
        i.check(TOKEN, &req(true), 2000, |_, _| resp(200, ACTIVE)),
        Ok(CurrentAuthority::Invalid)
    );
}

#[test]
fn failures_never_become_valid() {
    // AVLB-18・AVLB-22・OPRV-54
    let i = introspector();
    let check = |r: Result<IntrospectionHttpResponse, TransportFailure>| {
        i.check(TOKEN, &req(false), 1000, move |_, _| r)
    };
    assert_eq!(check(resp(503, "")), Err(IntrospectionFailure::Unavailable));
    assert_eq!(
        check(resp(401, "")),
        Err(IntrospectionFailure::ClientAuthRejected)
    );
    assert_eq!(
        check(resp(500, "")),
        Err(IntrospectionFailure::UnexpectedStatus)
    );
    assert_eq!(
        check(resp(200, r#"{"scope":"agent:test"}"#)),
        Err(IntrospectionFailure::MalformedResponse)
    );
    assert_eq!(
        check(resp(200, r#"{"active":"true"}"#)),
        Err(IntrospectionFailure::MalformedResponse)
    );
    assert_eq!(
        check(Err(TransportFailure)),
        Err(IntrospectionFailure::TransportFailure)
    );
}

#[test]
fn scope_is_split_on_ascii_space_only() {
    // RFC 6749 §3.3・RFC 7662 §2.2: 区切りは SP のみ。タブ・改行・Unicode 空白・空トークンは不正
    let i = introspector();
    let cases = [
        "other\\tagent:test",
        "a\\nagent:test",
        "a\u{3000}agent:test",
        "a  agent:test",
        " a",
        "a ",
    ];
    for scope in cases {
        let body = format!(r#"{{"active":true,"scope":"{scope}"}}"#);
        assert_eq!(
            i.check(TOKEN, &req(false), 1000, |_, _| resp(200, &body)),
            Err(IntrospectionFailure::MalformedResponse),
            "{scope:?}"
        );
    }
}

#[test]
fn request_follows_config_and_token_is_passed_by_argument() {
    let seen = Cell::new(false);
    let _ = introspector().check(TOKEN, &req(false), 1000, |r, t| {
        assert_eq!(r.endpoint, "https://auth.example.test/introspect");
        assert_eq!(r.client_id, "console-test-client");
        assert_eq!(r.token_type_hint, "access_token");
        assert_eq!(t, TOKEN);
        seen.set(true);
        resp(200, r#"{"active":false}"#)
    });
    assert!(seen.get());
}

#[test]
fn outputs_do_not_contain_token() {
    // SECR-23〜27
    let i = introspector();
    let dumps = [
        format!("{:?}", i.request()),
        format!("{:?}", i),
        format!(
            "{:?}",
            i.check(TOKEN, &req(true), 1000, |_, _| resp(200, ACTIVE))
        ),
        format!("{:?}", resp(200, TOKEN)),
        format!("{}", IntrospectionFailure::Unavailable),
        format!("{:?}", IntrospectionFailure::MalformedResponse),
    ];
    for d in dumps {
        assert!(!d.contains(TOKEN), "{d}");
    }
}
