//! Chrome の仮想 authenticator で passkey の登録 1 件を通す E2E（`#[ignore]`。`make e2e-chrome` で流す。CI 外）。
//!
//! 前提: chromedriver と Chrome が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。
//! unix 限定（構成要素が `cfg(unix)`）。非 unix では 0 件になり、`make e2e-chrome` が失敗にする。
#![cfg(unix)]

mod support;

use std::time::Duration;

use fandhe_auth_e2e_harness::{
    Browser, ChromeDriverComponent, ChromeDriverConfig, E2eConfig, E2eHarness, PasskeyOperation,
    Protocol, Transport, VirtualAuthenticatorOptions, register_once,
};
use support::TestOrigin;
use support::passkey::RecordSink;

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

    let mut sink = RecordSink::new(&handle, Browser::Chrome, PasskeyOperation::Registration);
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
    // script timeout は command timeout（既定 30 秒）より短くし、時間切れを WebDriver の
    // script timeout として観測できるようにする。
    handle
        .set_script_timeout(Duration::from_secs(20))
        .expect("timeout");

    let result = register_once(&handle, &origin.origin(), "localhost");
    // credential の取得と検査は 1 回だけ行い、記録と合否の両方に同じ結果を使う。
    let (run, check_ok) = support::passkey::registration_run_result(&handle, &aid, &result);
    sink.record(PasskeyOperation::Registration, run)
        .expect("record");
    let outcome = result.expect("register");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());

    assert!(check_ok, "credential check failed");
    running.stop().expect("stop");
    sink.finish().expect("record");
}
