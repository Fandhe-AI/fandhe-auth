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
use support::passkey::{RecordSink, authentication_run_result, register};

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

    let mut sink = RecordSink::new(&handle, Browser::Firefox, PasskeyOperation::Authentication);
    let (aid, cred) = register(&handle, &origin, &mut sink);
    let result = authenticate_once(&handle, &origin.origin(), "localhost", &cred);
    // 認証後の credential の検査も記録へ反映する（WebAuthn の結果と検査の結果を 1 件にまとめる）。
    let (run, check_ok) = authentication_run_result(&handle, &aid, &result);
    sink.record(PasskeyOperation::Authentication, run)
        .expect("record");
    let outcome = result.expect("authenticate");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());
    assert!(check_ok, "credential check failed");
    running.stop().expect("stop");
    sink.finish().expect("record");
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

    let mut sink = RecordSink::new(&handle, Browser::Firefox, PasskeyOperation::Authentication);
    let (aid, cred) = register(&handle, &origin, &mut sink);
    // 対照: 同じセッション・credential・UV 成功の状態で認証が通ることを先に確かめる。
    // NotAllowedError は期限切れ等でも返るため、これは UV の状態の変更後に NotAllowedError を観測する試験で、原因が UV であることまでは断定しない。
    let control_result = authenticate_once(&handle, &origin.origin(), "localhost", &cred);
    let (control_run, control_check_ok) = authentication_run_result(&handle, &aid, &control_result);
    sink.record(PasskeyOperation::Authentication, control_run)
        .expect("record");
    let control = control_result.expect("control");
    assert!(control.succeeded(), "error_name={:?}", control.error_name());
    assert!(control_check_ok, "credential check failed");
    // 登録の後に UV を失敗にする。判定は authenticator 側で完結し、ネットワークに出ない。
    handle.set_user_verified(&aid, false).expect("set uv");
    let result = authenticate_once(&handle, &origin.origin(), "localhost", &cred);
    let (run, check_ok) = authentication_run_result(&handle, &aid, &result);
    sink.record(PasskeyOperation::Authentication, run)
        .expect("record");
    let outcome = result.expect("authenticate");
    assert!(!outcome.succeeded());
    // HarnessTimeout とは別の名前で、対照が成功した同一条件から UV だけを変えた後の結果としてだけ読み、原因の判別とは読まない。
    assert_eq!(outcome.error_name(), Some(WebAuthnErrorName::NotAllowed));
    assert!(check_ok, "credential check failed");
    running.stop().expect("stop");
    sink.finish().expect("record");
}
