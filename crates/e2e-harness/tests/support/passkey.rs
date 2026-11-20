//! 登録→認証の E2E の共通の流れと、暫定の結果行（試験コードの中だけ。crate の公開 API にしない）。
//!
//! 結果行の正式な形式・実行の回数の記録は 5/6（#105）で決める。ここは暫定の 1 行で、英語（`D-2026-11-20-13`）。
//! URL・ポート・セッション ID・authenticator ID・credential ID・署名は出さない（SECR-11・SECR-23）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。
#![allow(dead_code)]

use std::time::Duration;

use fandhe_auth_e2e_harness::{
    AuthenticatorId, Browser, CredentialRef, DriverHandle, PasskeyOperation, Protocol, Transport,
    VirtualAuthenticatorOptions, WebAuthnErrorName, register_once,
};

use super::TestOrigin;

/// 試験の値。PoC の値（QP-1）であり要件の値ではない。
pub fn authenticator_options() -> VirtualAuthenticatorOptions {
    VirtualAuthenticatorOptions {
        protocol: Protocol::Ctap2,
        transport: Transport::Internal,
        has_resident_key: true,
        has_user_verification: true,
        is_user_verified: true,
    }
}

/// 仮想 authenticator を足して試験用 origin へ移動し、登録に成功して credential ID を返す。
pub fn register(
    handle: &DriverHandle,
    origin: &TestOrigin,
    browser: Browser,
) -> (AuthenticatorId, CredentialRef) {
    let aid = handle
        .add_virtual_authenticator(&authenticator_options())
        .expect("authenticator");
    handle.navigate(&origin.origin()).expect("navigate");
    // script timeout は command timeout（既定 30 秒）より短くし、時間切れを WebDriver の script timeout として観測する。
    handle
        .set_script_timeout(Duration::from_secs(20))
        .expect("timeout");
    let outcome = register_once(handle, &origin.origin(), "localhost").expect("register");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());
    // docs/harness/e2e.md の登録成功条件: credential がちょうど 1 件で RP ID が localhost。検証の後に成功行を出す。
    assert_one_localhost_credential(handle, &aid);
    print_result(
        handle,
        PasskeyOperation::Registration,
        browser,
        &Verdict::Succeeded,
    );
    let cred = outcome.credential().expect("credential").clone();
    (aid, cred)
}

/// 仮想 authenticator の credential がちょうど 1 件で、RP ID が `localhost` であることを確かめる。
pub fn assert_one_localhost_credential(handle: &DriverHandle, aid: &AuthenticatorId) {
    let creds = handle.credentials(aid).expect("credentials");
    assert!(creds.len() == 1, "credential count mismatch");
    // SECR-23: ドライバ応答の生値を失敗メッセージに出さない。真偽だけを検査する。
    assert!(creds[0].rp_id == "localhost", "credential rp_id mismatch");
}

pub enum Verdict {
    Succeeded,
    Rejected(WebAuthnErrorName),
}

/// 暫定の結果行を標準出力へ出す。ブラウザの版はドライバが返した文字列を文字種と長さで絞る。
pub fn print_result(
    handle: &DriverHandle,
    operation: PasskeyOperation,
    browser: Browser,
    verdict: &Verdict,
) {
    println!(
        "{}",
        result_line(
            operation,
            browser,
            &handle.browser_version().unwrap_or_default(),
            verdict
        )
    );
}

pub fn result_line(
    operation: PasskeyOperation,
    browser: Browser,
    version: &str,
    verdict: &Verdict,
) -> String {
    // 版番号の形式（数字と `.` だけで、先頭は数字で `.` を含む）に絞る。UUID 形式のセッション ID 等は通さない（SECR-23）。
    let version = if version.len() <= 32
        && version.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && version.contains('.')
        && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        version
    } else {
        "unknown"
    };
    let result = match verdict {
        Verdict::Succeeded => "succeeded".to_owned(),
        Verdict::Rejected(n) => format!("rejected:{n:?}"),
    };
    let device = "authenticator=WebDriverVirtual evidence=Virtual";
    let flags = "counts_as_real_device=false counts_as_safari=false";
    format!(
        "e2e-result: operation={operation:?} browser={browser:?} browser_version={version} {device} {flags} result={result}"
    )
}
