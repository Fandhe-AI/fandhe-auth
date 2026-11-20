//! chromedriver 構成要素と操作の入口の試験（公開 API だけ）。

use fandhe_auth_e2e_harness::chrome::new_session_request;
use fandhe_auth_e2e_harness::{
    AuthenticatorId, ChromeDriverComponent, ChromeDriverConfig, E2eComponent, E2eErrorKind,
    Protocol, Transport, VirtualAuthenticatorOptions,
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
fn new_session_request_has_only_headless_flag() {
    // DPOL-30: 検証を外す意味の flag を足さない。
    let v = new_session_request();
    let args = &v["capabilities"]["alwaysMatch"]["goog:chromeOptions"]["args"];
    assert_eq!(args, &serde_json::json!(["--headless=new"]));
}

#[test]
fn ids_with_path_characters_are_rejected() {
    for bad in ["", "a/b", "a b", "../x", "a?b", "a\r\nb"] {
        assert!(AuthenticatorId::new(bad).is_err(), "{bad:?}");
    }
    assert!(AuthenticatorId::new("ok-1").is_ok());
}

#[test]
fn start_with_missing_driver_program_fails_and_leaves_nothing() {
    let handle;
    let mut c = {
        let c = ChromeDriverComponent::new(ChromeDriverConfig::new("/nonexistent/e2e-driver"));
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
    let c = ChromeDriverComponent::new(ChromeDriverConfig::new("x"));
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
