//! Firefox の仮想 authenticator で、登録の後の passkey の認証を通す E2E（`#[ignore]`。`make e2e-firefox` で流す。CI 外）。
//!
//! 前提: geckodriver と Firefox が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。
//! 失敗の場合は、仮想 authenticator の UV を失敗にして `userVerification: required` で認証し、`NotAllowedError` を確かめる。
//! unix 限定（構成要素が `cfg(unix)`）。非 unix では 0 件になり、`make e2e-firefox` が失敗にする。
#![cfg(unix)]

mod support;

use fandhe_auth_e2e_harness::{
    Browser, E2eConfig, E2eHarness, GeckoDriverComponent, GeckoDriverConfig, PasskeyOperation,
    WebAuthnErrorName, authenticate_once,
};
use support::TestOrigin;
use support::passkey::{Verdict, print_result, register};

#[test]
#[ignore = "geckodriver と Firefox が要る。make e2e-firefox で流す"]
fn authenticates_after_registration_with_firefox_virtual_authenticator() {
    let origin = TestOrigin::start();
    let driver = GeckoDriverComponent::new(GeckoDriverConfig::new("geckodriver"));
    let handle = driver.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Authentication, Browser::Firefox).with(Box::new(driver)),
    )
    .expect("start");

    let (aid, cred) = register(&handle, &origin, Browser::Firefox);
    let outcome =
        authenticate_once(&handle, &origin.origin(), "localhost", &cred).expect("authenticate");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());
    print_result(
        &handle,
        PasskeyOperation::Authentication,
        Browser::Firefox,
        &Verdict::Succeeded,
    );

    let creds = handle.credentials(&aid).expect("credentials");
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].rp_id, "localhost");

    running.stop().expect("stop");
}

#[test]
#[ignore = "geckodriver と Firefox が要る。make e2e-firefox で流す"]
fn authentication_fails_when_user_verification_fails_with_firefox() {
    let origin = TestOrigin::start();
    let driver = GeckoDriverComponent::new(GeckoDriverConfig::new("geckodriver"));
    let handle = driver.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Authentication, Browser::Firefox).with(Box::new(driver)),
    )
    .expect("start");

    let (aid, cred) = register(&handle, &origin, Browser::Firefox);
    // 登録の後に UV を失敗にする。判定は authenticator 側で完結し、ネットワークに出ない。
    handle.set_user_verified(&aid, false).expect("set uv");
    let outcome =
        authenticate_once(&handle, &origin.origin(), "localhost", &cred).expect("authenticate");
    assert!(!outcome.succeeded());
    assert_eq!(outcome.error_name(), Some(WebAuthnErrorName::NotAllowed));
    print_result(
        &handle,
        PasskeyOperation::Authentication,
        Browser::Firefox,
        &Verdict::Rejected(WebAuthnErrorName::NotAllowed),
    );

    running.stop().expect("stop");
}
