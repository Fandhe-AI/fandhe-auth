//! Chrome の仮想 authenticator で、登録の後の passkey の認証を通す E2E（`#[ignore]`。`make e2e-chrome` で流す。CI 外）。
//!
//! 前提: chromedriver と Chrome が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。
//! 失敗の場合は、仮想 authenticator の UV を失敗にして `userVerification: required` で認証し、`NotAllowedError` を確かめる。
//! unix 限定（構成要素が `cfg(unix)`）。非 unix では 0 件になり、`make e2e-chrome` が失敗にする。
#![cfg(unix)]

mod support;

use fandhe_auth_e2e_harness::{
    Browser, ChromeDriverComponent, ChromeDriverConfig, E2eConfig, E2eHarness, PasskeyOperation,
    WebAuthnErrorName, authenticate_once,
};
use support::TestOrigin;
use support::passkey::{Verdict, assert_one_localhost_credential, print_result, register};

#[test]
#[ignore = "chromedriver と Chrome が要る。make e2e-chrome で流す"]
fn authenticates_after_registration_with_chrome_virtual_authenticator() {
    let origin = TestOrigin::start();
    let driver = ChromeDriverComponent::new(ChromeDriverConfig::new("chromedriver"));
    let handle = driver.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Authentication, Browser::Chrome).with(Box::new(driver)),
    )
    .expect("start");

    let (aid, cred) = register(&handle, &origin, Browser::Chrome);
    let outcome =
        authenticate_once(&handle, &origin.origin(), "localhost", &cred).expect("authenticate");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());

    assert_one_localhost_credential(&handle, &aid);
    print_result(
        &handle,
        PasskeyOperation::Authentication,
        Browser::Chrome,
        &Verdict::Succeeded,
    );

    running.stop().expect("stop");
}

#[test]
#[ignore = "chromedriver と Chrome が要る。make e2e-chrome で流す"]
fn authentication_fails_when_user_verification_fails_with_chrome() {
    let origin = TestOrigin::start();
    let driver = ChromeDriverComponent::new(ChromeDriverConfig::new("chromedriver"));
    let handle = driver.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Authentication, Browser::Chrome).with(Box::new(driver)),
    )
    .expect("start");

    let (aid, cred) = register(&handle, &origin, Browser::Chrome);
    // 登録の後に UV を失敗にする。判定は authenticator 側で完結し、ネットワークに出ない。
    handle.set_user_verified(&aid, false).expect("set uv");
    let outcome =
        authenticate_once(&handle, &origin.origin(), "localhost", &cred).expect("authenticate");
    assert!(!outcome.succeeded());
    assert_eq!(outcome.error_name(), Some(WebAuthnErrorName::NotAllowed));
    assert_one_localhost_credential(&handle, &aid);
    print_result(
        &handle,
        PasskeyOperation::Authentication,
        Browser::Chrome,
        &Verdict::Rejected(WebAuthnErrorName::NotAllowed),
    );

    running.stop().expect("stop");
}
