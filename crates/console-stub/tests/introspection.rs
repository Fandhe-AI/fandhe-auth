//! TASK-8・#73・#78: introspection の呼び出しの模擬の試験。
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

// ---- #78: 任意項目・形の誤り・境界の網羅 ----

fn check_body(
    body: &str,
    requirement: &AuthorityRequirement,
    now: u64,
) -> Result<CurrentAuthority, IntrospectionFailure> {
    introspector().check(TOKEN, requirement, now, |_, _| resp(200, body))
}

#[test]
fn optional_fields_may_be_absent() {
    // ADLG-15・AGCR-32: scope だけの有効な応答
    let r = check_body(r#"{"active":true,"scope":"agent:test"}"#, &req(false), 1000).unwrap();
    match r {
        CurrentAuthority::Valid(g) => {
            assert_eq!(g.scope, ["agent:test".to_owned()]);
            assert_eq!(g.exp, None);
            assert_eq!(g.cnf_jkt, None);
            assert_eq!(g.client_id, None);
            assert_eq!(g.token_type, None);
        }
        CurrentAuthority::Invalid => panic!("expected valid"),
    }
}

#[test]
fn empty_or_missing_scope_is_invalid() {
    // OPRV-54: 要件の scope を含まない応答は理由なしの無効
    for body in [r#"{"active":true,"scope":""}"#, r#"{"active":true}"#] {
        assert_eq!(
            check_body(body, &req(false), 1000),
            Ok(CurrentAuthority::Invalid),
            "{body}"
        );
    }
}

#[test]
fn wrong_shapes_are_malformed_never_valid() {
    // AVLB-18・AVLB-22 の向き: 形の誤りは有効にならない
    let bodies = [
        r#"{"active":true,"scope":1}"#,
        r#"{"active":true,"scope":"agent:test","exp":"2000"}"#,
        r#"{"active":true,"scope":"agent:test","exp":-1}"#,
        r#"{"active":true,"scope":"agent:test","cnf":"jkt-test"}"#,
        r#"{"active":true,"scope":"agent:test","cnf":{"jkt":1}}"#,
        r#"{"active":true,"scope":"agent:test","client_id":1}"#,
        r#"{"active":true,"scope":"agent:test","token_type":1}"#,
        r#"["active"]"#,
        "null",
        "not json",
        "",
    ];
    for body in bodies {
        assert_eq!(
            check_body(body, &req(false), 1000),
            Err(IntrospectionFailure::MalformedResponse),
            "{body}"
        );
    }
}

#[test]
fn cnf_without_jkt_depends_on_binding_requirement() {
    // AGCR-32・ADLG-15 ①
    let body = r#"{"active":true,"scope":"agent:test","token_type":"DPoP","cnf":{}}"#;
    assert!(matches!(
        check_body(body, &req(false), 1000),
        Ok(CurrentAuthority::Valid(g)) if g.cnf_jkt.is_none()
    ));
    assert_eq!(
        check_body(body, &req(true), 1000),
        Ok(CurrentAuthority::Invalid)
    );
}

#[test]
fn binding_requires_dpop_type_and_matching_jkt() {
    // AGCR-32: token_type が DPoP でないものは jkt が一致しても結合を満たさない
    let bearer =
        r#"{"active":true,"scope":"agent:test","token_type":"Bearer","cnf":{"jkt":"jkt-test"}}"#;
    assert_eq!(
        check_body(bearer, &req(true), 1000),
        Ok(CurrentAuthority::Invalid)
    );
    let no_type = r#"{"active":true,"scope":"agent:test","cnf":{"jkt":"jkt-test"}}"#;
    assert_eq!(
        check_body(no_type, &req(true), 1000),
        Ok(CurrentAuthority::Invalid)
    );
    assert!(matches!(
        check_body(bearer, &req(false), 1000),
        Ok(CurrentAuthority::Valid(_))
    ));
}

#[test]
fn exp_boundary_follows_sign_17() {
    // SIGN-17 の向き: exp > now は有効、exp == now・exp < now は無効
    assert!(matches!(
        check_body(ACTIVE, &req(true), 1999),
        Ok(CurrentAuthority::Valid(_))
    ));
    assert_eq!(
        check_body(ACTIVE, &req(true), 2000),
        Ok(CurrentAuthority::Invalid)
    );
    assert_eq!(
        check_body(ACTIVE, &req(true), 2001),
        Ok(CurrentAuthority::Invalid)
    );
}

#[test]
fn failure_display_is_distinct() {
    let all = [
        IntrospectionFailure::ClientAuthRejected,
        IntrospectionFailure::Unavailable,
        IntrospectionFailure::UnexpectedStatus,
        IntrospectionFailure::MalformedResponse,
        IntrospectionFailure::TransportFailure,
    ];
    // 網羅の match: 値を足したらここが壊れる
    for f in all {
        match f {
            IntrospectionFailure::ClientAuthRejected
            | IntrospectionFailure::Unavailable
            | IntrospectionFailure::UnexpectedStatus
            | IntrospectionFailure::MalformedResponse
            | IntrospectionFailure::TransportFailure => {}
        }
    }
    let texts: Vec<String> = all.iter().map(|f| f.to_string()).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(a.starts_with("introspection: "), "{a}");
        for b in &texts[i + 1..] {
            assert_ne!(a, b);
        }
    }
}
