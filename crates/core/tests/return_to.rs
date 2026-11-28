//! `validate_return_to` の公開 API の試験（UISF-6・CSRF-33・CSRF-34）。

use fandhe_auth_core::return_to::{ReturnTo, ReturnToDecision, validate_return_to};

fn dflt(p: &str) -> ReturnTo {
    ReturnTo::default_destination(p).expect("default must be valid")
}

// CSRF-34: 型を満たす値は入力と完全に一致して通る。
#[test]
fn valid_paths_pass_through_byte_for_byte() {
    let d = dflt("/");
    for p in [
        "/",
        "/home?tab=1",
        "/a/b%20c",
        "/sessions#top",
        "/%2F%2Fevil.example",
        "/x?u=https://evil.example/",
        "/a//b",
        "/passkeys",
        "/sessions",
        "/home",
    ] {
        match validate_return_to(Some(p), &d) {
            ReturnToDecision::Requested(t) => assert_eq!(t.as_str(), p),
            other => panic!("expected Requested for {p:?}, got {other:?}"),
        }
    }
}

// UISF-6・CSRF-33: 型を満たさない値と値の無い場合は既定に落ちる。
#[test]
fn invalid_or_missing_falls_back_to_default() {
    let cases: [Option<&str>; 14] = [
        None,
        Some(""),
        Some("https://evil.example/"),
        Some("//evil.example"),
        Some("/\\evil.example"),
        Some("javascript:alert(1)"),
        Some("/ok\r\nSet-Cookie: x=1"),
        Some("/a\tb"),
        Some("/a\u{0}b"),
        Some("/a\u{7f}b"),
        Some("/a\u{85}b"),
        Some(" /x"),
        Some("/a\\b"),
        Some("x"),
    ];
    for dp in ["/", "/home"] {
        let d = dflt(dp);
        for c in cases {
            let r = validate_return_to(c, &d);
            assert!(matches!(r, ReturnToDecision::Default(_)), "{c:?}");
            assert_eq!(r.target(), &d, "{c:?}");
        }
    }
}

// 既定そのものが型を満たさない設定は作れない（fail-closed）。
#[test]
fn invalid_default_is_rejected() {
    assert!(ReturnTo::default_destination("//evil.example").is_none());
    assert!(ReturnTo::default_destination("https://evil.example/").is_none());
    assert!(ReturnTo::default_destination("").is_none());
}
