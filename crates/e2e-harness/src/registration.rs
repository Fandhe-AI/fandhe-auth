//! passkey の登録 1 件の手順（試験用ページで `navigator.credentials.create()` を呼ぶ）。
//!
//! 役割: 仮想 authenticator を足したブラウザで登録の操作を 1 回通し、ページ内で比べた真偽だけを返す。
//! サーバー側の登録応答の検証（`webauthn-rs`）は RP 側の実体（`TASK-716`・PKEY の実装タスク）待ちで、ここでは扱わない。
//! PKEY の振る舞いを満たしたとは主張しない。結果は〔仮想〕（ACPT-7・ACPT-8）。
//!
//! 安全: 実行するスクリプトは定数で、外部入力を連結しない（A03）。challenge・user.id はブラウザの
//! `crypto.getRandomValues` で作る。credential の生の値・公開鍵・attestation の中身は返さない（SECR-11）。

use serde_json::{Value, json};

use crate::browser::{AuthenticatorKind, Evidence};
use crate::chrome::ChromeHandle;
use crate::component::E2eComponentKind;
use crate::harness::{E2eError, E2eErrorKind};

/// 登録のスクリプト。options は PKEY-4 の実測の形（UV required・attestation none・ES256/RS256）に合わせるが、
/// PKEY-4 を満たした主張はしない。`arguments[0]` が RP ID、`arguments[1]` が期待する origin。
const REGISTER_SCRIPT: &str = r#"
const done = arguments[arguments.length - 1];
const rpId = arguments[0];
const expectedOrigin = arguments[1];
(async () => {
    try {
        const rnd = (n) => crypto.getRandomValues(new Uint8Array(n));
        const b64u = (b) => btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
        const challenge = rnd(32);
        const cred = await navigator.credentials.create({ publicKey: {
            rp: { id: rpId, name: 'e2e' },
            user: { id: rnd(16), name: 'e2e-user', displayName: 'e2e-user' },
            challenge,
            pubKeyCredParams: [{ type: 'public-key', alg: -7 }, { type: 'public-key', alg: -257 }],
            authenticatorSelection: { residentKey: 'discouraged', userVerification: 'required' },
            attestation: 'none',
            timeout: 60000
        }});
        const cd = JSON.parse(new TextDecoder().decode(cred.response.clientDataJSON));
        done({
            ok: true,
            typeOk: cred.type === 'public-key',
            attestationLen: cred.response.attestationObject.byteLength,
            clientDataTypeOk: cd.type === 'webauthn.create',
            originOk: cd.origin === expectedOrigin && location.origin === expectedOrigin,
            challengeOk: cd.challenge === b64u(challenge)
        });
    } catch (e) {
        done({ ok: false, errorName: String((e && e.name) || 'Error') });
    }
})();
"#;

/// 登録 1 件の結果。真偽と長さだけで、credential の値を持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationOutcome {
    created: bool,
    type_ok: bool,
    attestation_len: u64,
    client_data_type_ok: bool,
    origin_ok: bool,
    challenge_ok: bool,
    error_name: Option<String>,
}

impl RegistrationOutcome {
    /// ページ内の比較がすべて成り立ち、attestation が空でないか。
    pub fn succeeded(&self) -> bool {
        self.created
            && self.type_ok
            && self.attestation_len > 0
            && self.client_data_type_ok
            && self.origin_ok
            && self.challenge_ok
    }

    /// `create()` が失敗したときの `DOMException.name`。
    pub fn error_name(&self) -> Option<&str> {
        self.error_name.as_deref()
    }

    /// 証拠の種類。常に〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。
    pub fn evidence(&self) -> Evidence {
        AuthenticatorKind::WebDriverVirtual.evidence()
    }

    fn from_value(v: &Value) -> Option<Self> {
        let b = |k: &str| v.get(k).and_then(Value::as_bool);
        if b("ok")? {
            Some(Self {
                created: true,
                type_ok: b("typeOk")?,
                attestation_len: v.get("attestationLen")?.as_u64()?,
                client_data_type_ok: b("clientDataTypeOk")?,
                origin_ok: b("originOk")?,
                challenge_ok: b("challengeOk")?,
                error_name: None,
            })
        } else {
            Some(Self {
                created: false,
                type_ok: false,
                attestation_len: 0,
                client_data_type_ok: false,
                origin_ok: false,
                challenge_ok: false,
                error_name: Some(v.get("errorName")?.as_str()?.chars().take(64).collect()),
            })
        }
    }
}

/// 登録を 1 回通す。呼ぶ前に `origin` へ移動済みで、仮想 authenticator を足してあること。
pub fn register_once(
    handle: &ChromeHandle,
    origin: &str,
    rp_id: &str,
) -> Result<RegistrationOutcome, E2eError> {
    let v = handle.execute_async(REGISTER_SCRIPT, &[json!(rp_id), json!(origin)])?;
    RegistrationOutcome::from_value(&v).ok_or(E2eError::new(
        E2eErrorKind::CommandFailed,
        E2eComponentKind::BrowserDriver,
    ))
}
