//! Chrome の仮想 authenticator で passkey の登録 1 件を通す E2E（`#[ignore]`。`make e2e-chrome` で流す。CI 外）。
//!
//! 前提: chromedriver と Chrome が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。

mod support;

use std::time::Duration;

use fandhe_auth_e2e_harness::{
    Browser, ChromeDriverComponent, ChromeDriverConfig, E2eConfig, E2eHarness, PasskeyOperation,
    Protocol, Transport, VirtualAuthenticatorOptions, register_once,
};
use support::TestOrigin;

#[test]
#[ignore = "chromedriver と Chrome が要る。make e2e-chrome で流す"]
fn registers_one_passkey_with_chrome_virtual_authenticator() {
    let origin = TestOrigin::start();
    let chrome = ChromeDriverComponent::new(ChromeDriverConfig::new("chromedriver"));
    let handle = chrome.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Registration, Browser::Chrome).with(Box::new(chrome)),
    )
    .expect("start");

    // 試験の値。PoC の値（QP-1）であり要件の値ではない。
    let aid = handle
        .add_virtual_authenticator(&VirtualAuthenticatorOptions {
            protocol: Protocol::Ctap2,
            transport: Transport::Internal,
            has_resident_key: true,
            has_user_verification: true,
            is_user_verified: true,
        })
        .expect("authenticator");
    handle.navigate(&origin.origin()).expect("navigate");
    handle
        .set_script_timeout(Duration::from_secs(30))
        .expect("timeout");

    let outcome = register_once(&handle, &origin.origin(), "localhost").expect("register");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());

    let creds = handle.credentials(&aid).expect("credentials");
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].rp_id, "localhost");

    running.stop().expect("stop");
}
