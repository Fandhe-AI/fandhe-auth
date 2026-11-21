//! Firefox の仮想 authenticator で passkey の登録 1 件を通す E2E（`#[ignore]`。`make e2e-firefox` で流す。CI 外）。
//!
//! 前提: geckodriver と Firefox が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。
//! unix 限定（構成要素が `cfg(unix)`）。非 unix では 0 件になり、`make e2e-firefox` が失敗にする。
#![cfg(unix)]

mod support;

use std::time::Duration;

use fandhe_auth_e2e_harness::{
    Browser, E2eConfig, E2eHarness, GeckoDriverComponent, GeckoDriverConfig, PasskeyOperation,
    Protocol, RunResult, Transport, VirtualAuthenticatorOptions, register_once,
};
use support::TestOrigin;
use support::passkey::RecordSink;

#[test]
#[ignore = "geckodriver と Firefox が要る。make e2e-firefox で流す"]
fn registers_one_passkey_with_firefox_virtual_authenticator() {
    let origin = TestOrigin::start();
    let gecko = GeckoDriverComponent::new(GeckoDriverConfig::new("geckodriver"));
    let handle = gecko.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Registration, Browser::Firefox).with(Box::new(gecko)),
    )
    .expect("start");

    let mut sink = RecordSink::new(&handle, Browser::Firefox, PasskeyOperation::Registration);
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
    sink.record(
        PasskeyOperation::Registration,
        RunResult::from_registration(&result),
    );
    let outcome = result.expect("register");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());

    let creds = handle.credentials(&aid).expect("credentials");
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].rp_id, "localhost");
    sink.finish();

    running.stop().expect("stop");
}
