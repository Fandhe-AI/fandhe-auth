//! geckodriver の構成要素（`GeckoDriverComponent`）の試験。geckodriver・Firefox なしで流せる範囲だけ。
//!
//! unix 限定（構成要素が `cfg(unix)`）。結果は起動・停止の形の確認で、E2E の合格に数えない（ACPT-7）。
#![cfg(unix)]

use fandhe_auth_e2e_harness::firefox::new_session_request;
use fandhe_auth_e2e_harness::{
    E2eComponent, E2eErrorKind, GeckoDriverComponent, GeckoDriverConfig, Protocol, Transport,
    VirtualAuthenticatorOptions,
};

fn opts() -> VirtualAuthenticatorOptions {
    // 試験の値。PoC の値（QP-1）であり要件の値ではない。
    VirtualAuthenticatorOptions {
        protocol: Protocol::Ctap2,
        transport: Transport::Internal,
        has_resident_key: true,
        has_user_verification: true,
        is_user_verified: true,
    }
}

#[test]
fn new_session_request_has_only_headless_arg_and_two_prefs() {
    // DPOL-30: 検証を外す意味の引数・pref を足さない。pref は PoC-3 の値（要件の値ではない。人の確認待ち）。
    let v = new_session_request();
    let opts = &v["capabilities"]["alwaysMatch"]["moz:firefoxOptions"];
    assert_eq!(v["capabilities"]["alwaysMatch"]["browserName"], "firefox");
    assert_eq!(opts["args"], serde_json::json!(["-headless"]));
    assert_eq!(
        opts["prefs"],
        serde_json::json!({
            "security.webauth.webauthn_enable_softtoken": true,
            "security.webauth.webauthn_enable_usbtoken": false
        })
    );
    let text = v.to_string().to_ascii_lowercase();
    for word in [
        "disable",
        "insecure",
        "skip_",
        "bypass",
        "no_csrf",
        "allow_all",
        "unchecked",
    ] {
        assert!(!text.contains(word), "{word}");
    }
}

#[test]
fn start_with_missing_driver_program_fails_and_leaves_nothing() {
    let handle;
    let mut c = {
        let c = GeckoDriverComponent::new(GeckoDriverConfig::new("/nonexistent/e2e-driver"));
        handle = c.handle();
        c
    };
    let e = c.start().unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::StartFailed);
    assert!(!handle.is_running());
    assert!(c.stop().is_ok());
}

#[test]
fn handle_is_not_running_before_start() {
    let c = GeckoDriverComponent::new(GeckoDriverConfig::new("x"));
    let h = c.handle();
    assert_eq!(
        h.navigate("http://localhost/").unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
    assert_eq!(
        h.browser_version().unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
    assert_eq!(
        h.add_virtual_authenticator(&opts()).unwrap_err().kind(),
        E2eErrorKind::NotRunning
    );
}
