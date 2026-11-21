//! 登録→認証の E2E の共通の流れと、実行の記録の出力 `RecordSink`（試験コードの中だけ。crate の公開 API にしない）。
//!
//! 記録の形式は `fandhe_auth_e2e_harness::record`（5/6）。出力は英語（`D-2026-11-20-13`）。
//! URL・ポート・セッション ID・authenticator ID・credential ID・署名は出さない（SECR-11・SECR-23）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。
#![allow(dead_code)]

use std::time::Duration;

use fandhe_auth_e2e_harness::{
    AuthenticatorId, AuthenticatorKind, Browser, BrowserVersion, CredentialRef, DriverHandle,
    E2eRunRecord, PasskeyOperation, Protocol, RunResult, Transport, VirtualAuthenticatorOptions,
    register_once,
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
    sink: &mut RecordSink,
) -> (AuthenticatorId, CredentialRef) {
    let aid = handle
        .add_virtual_authenticator(&authenticator_options())
        .expect("authenticator");
    handle.navigate(&origin.origin()).expect("navigate");
    // script timeout は command timeout（既定 30 秒）より短くし、時間切れを WebDriver の script timeout として観測する。
    handle
        .set_script_timeout(Duration::from_secs(20))
        .expect("timeout");
    let result = register_once(handle, &origin.origin(), "localhost");
    sink.record(
        PasskeyOperation::Registration,
        RunResult::from_registration(&result),
    );
    let outcome = result.expect("register");
    assert!(outcome.succeeded(), "error_name={:?}", outcome.error_name());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());
    // docs/harness/e2e.md の登録成功条件: credential がちょうど 1 件で RP ID が localhost。
    assert_one_localhost_credential(handle, &aid);
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

/// 1 つのブラウザのセッションの実行の記録を持ち、`Drop`（panic の巻き戻りを含む）で本文を標準出力へ出し、
/// `target/` の下のファイルに新規で書く。`Drop` の中では panic しない。パスは出さない（SECR-23）。
pub struct RecordSink {
    record: E2eRunRecord,
}

impl RecordSink {
    /// 起動の後に作る。版は `handle` から取る（取れなければ `Unknown`）。
    pub fn new(handle: &DriverHandle, browser: Browser, target: PasskeyOperation) -> Self {
        let version = handle.browser_version().unwrap_or(BrowserVersion::Unknown);
        Self {
            record: E2eRunRecord::new(
                browser,
                version,
                AuthenticatorKind::WebDriverVirtual,
                target,
            ),
        }
    }

    /// 実行 1 件を記録する（検査の前に呼び、失敗した実行も回数に残す）。
    pub fn record(&mut self, op: PasskeyOperation, result: RunResult) {
        let _ = self.record.record(op, result);
    }

    /// 試験が最後まで到達したことを記す。
    pub fn finish(&mut self) {
        self.record.mark_completed();
    }
}

impl Drop for RecordSink {
    fn drop(&mut self) {
        println!("{}", self.record);
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e-records");
        let _ = self.record.write_new(&dir);
    }
}
