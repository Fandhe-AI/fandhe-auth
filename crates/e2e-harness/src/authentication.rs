//! passkey の認証 1 件の手順（試験用ページで `navigator.credentials.get()` を呼ぶ）。
//!
//! 役割: [`crate::registration`] で登録した credential を `allowCredentials` に入れて認証の操作を 1 回通し、
//! ページ内で比べた真偽・長さ・失敗の名前だけを返す。サーバー側の認証応答の検証（`webauthn-rs`）は RP 側の実体
//! （`TASK-716`・PKEY の実装タスク）待ちで、ここでは扱わない。PKEY の振る舞いを満たしたとは主張しない。
//! 結果は〔仮想〕（ACPT-7・ACPT-8）。呼び出し元は E2E の試験だけ。
//!
//! 安全: 実行するスクリプトは定数で、値は WebDriver の `args`（JSON）で渡す（A03）。challenge はブラウザの
//! `crypto.getRandomValues` で作り、RP ID のハッシュの比較もページ内の `crypto.subtle` で行う（暗号を自前で実装しない）。
//! 署名・`authenticatorData` の中身・`userHandle` は返さない（SECR-11）。

use serde_json::{Value, json};

use crate::browser::{AuthenticatorKind, Evidence};
use crate::component::E2eComponentKind;
use crate::driver_process::DriverHandle;
use crate::harness::{E2eError, E2eErrorKind};
use crate::webauthn_error::WebAuthnErrorName;
use crate::webdriver::CredentialRef;

/// 認証のスクリプト。`arguments[0]` が RP ID、`[1]` が期待する origin、`[2]` が credential ID（base64url）。
/// 10 秒の期限はページ内の `AbortController` で明示的に設ける（`publicKey.timeout` はブラウザが調整し得るヒントで、
/// 打ち切りの保証にならない）。期限切れは `HarnessTimeout`（`NotAllowed` とは別の名前）で返し、試験側の script timeout
/// （20 秒）より先に結果として観測する。値はハーネスの安全上の値で、要件の値ではない。
const AUTHENTICATE_SCRIPT: &str = r#"
const done = arguments[arguments.length - 1];
const rpId = arguments[0];
const expectedOrigin = arguments[1];
const credB64u = arguments[2];
(async () => {
    let timedOut = false;
    const ctl = new AbortController();
    const timer = setTimeout(() => { timedOut = true; ctl.abort(); }, 10000);
    try {
        if (location.origin !== expectedOrigin) {
            clearTimeout(timer);
            done({ ok: false, errorName: 'OriginMismatch' });
            return;
        }
        const rnd = (n) => crypto.getRandomValues(new Uint8Array(n));
        const b64u = (b) => btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
        const unb64u = (s) => {
            const p = s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4);
            return Uint8Array.from(atob(p), (c) => c.charCodeAt(0));
        };
        const challenge = rnd(32);
        const cred = await navigator.credentials.get({ signal: ctl.signal, publicKey: {
            challenge,
            rpId,
            allowCredentials: [{ type: 'public-key', id: unb64u(credB64u) }],
            userVerification: 'required',
            timeout: 10000
        }});
        clearTimeout(timer);
        const cd = JSON.parse(new TextDecoder().decode(cred.response.clientDataJSON));
        const ad = new Uint8Array(cred.response.authenticatorData);
        const sig = new Uint8Array(cred.response.signature);
        const rpHash = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(rpId)));
        let hashOk = ad.length >= 37;
        for (let i = 0; hashOk && i < 32; i++) { if (ad[i] !== rpHash[i]) { hashOk = false; } }
        done({
            ok: true,
            typeOk: cred.type === 'public-key',
            idOk: b64u(new Uint8Array(cred.rawId)) === credB64u,
            clientDataTypeOk: cd.type === 'webauthn.get',
            originOk: cd.origin === expectedOrigin && location.origin === expectedOrigin,
            challengeOk: cd.challenge === b64u(challenge),
            rpIdHashOk: hashOk,
            upOk: ad.length >= 37 && (ad[32] & 0x01) !== 0,
            uvOk: ad.length >= 37 && (ad[32] & 0x04) !== 0,
            authenticatorDataLen: ad.length,
            signatureLen: sig.length
        });
    } catch (e) {
        clearTimeout(timer);
        done({ ok: false, errorName: timedOut ? 'HarnessTimeout' : String((e && e.name) || 'Error') });
    }
})();
"#;

/// 認証 1 件の結果。真偽・長さ・失敗の名前だけで、署名・authenticator data の値を持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticationOutcome {
    asserted: bool,
    checks: [bool; 8],
    authenticator_data_len: u64,
    signature_len: u64,
    error_name: Option<WebAuthnErrorName>,
}

impl AuthenticationOutcome {
    /// ページ内の比較がすべて成り立ち、UP・UV が立ち、長さが条件（37 以上・1 以上）を満たすか。
    pub fn succeeded(&self) -> bool {
        self.asserted
            && self.checks.iter().all(|c| *c)
            && self.authenticator_data_len >= 37
            && self.signature_len >= 1
    }

    /// `get()` が失敗したときの `DOMException.name`（既知の名前の列挙。未知は `Other`）。
    pub fn error_name(&self) -> Option<WebAuthnErrorName> {
        self.error_name
    }

    /// 証拠の種類。常に〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。
    pub fn evidence(&self) -> Evidence {
        AuthenticatorKind::WebDriverVirtual.evidence()
    }

    pub(crate) fn from_value(v: &Value) -> Option<Self> {
        let b = |k: &str| v.get(k).and_then(Value::as_bool);
        if b("ok")? {
            Some(Self {
                asserted: true,
                checks: [
                    b("typeOk")?,
                    b("idOk")?,
                    b("clientDataTypeOk")?,
                    b("originOk")?,
                    b("challengeOk")?,
                    b("rpIdHashOk")?,
                    b("upOk")?,
                    b("uvOk")?,
                ],
                authenticator_data_len: v.get("authenticatorDataLen")?.as_u64()?,
                signature_len: v.get("signatureLen")?.as_u64()?,
                error_name: None,
            })
        } else {
            Some(Self {
                asserted: false,
                checks: [false; 8],
                authenticator_data_len: 0,
                signature_len: 0,
                error_name: Some(WebAuthnErrorName::from_dom_name(
                    v.get("errorName")?.as_str()?,
                )),
            })
        }
    }
}

/// `http://<host>[:<port>]` の形の origin からホストを取り出す。形が違えば `None`。
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix("http://")?;
    let host = rest.split_once(':').map_or(rest, |(h, _)| h);
    (!host.is_empty() && !host.contains(['/', '@', '?', '#'])).then_some(host)
}

/// 認証を 1 回通す。呼ぶ前に `origin` へ移動済みで、`credential` を持つ仮想 authenticator があること。
pub fn authenticate_once(
    handle: &DriverHandle,
    origin: &str,
    rp_id: &str,
    credential: &CredentialRef,
) -> Result<AuthenticationOutcome, E2eError> {
    // A10: RP ID は試験用 origin のホストと完全一致に限る（無関係な RP ID は Related Origin Requests で外へ通信し得る）。
    if origin_host(origin) != Some(rp_id) {
        return Err(E2eError::new(
            E2eErrorKind::CommandFailed,
            E2eComponentKind::BrowserDriver,
        ));
    }
    let v = handle.execute_async(
        AUTHENTICATE_SCRIPT,
        &[json!(rp_id), json!(origin), json!(credential.as_str())],
    )?;
    AuthenticationOutcome::from_value(&v).ok_or(E2eError::new(
        E2eErrorKind::CommandFailed,
        E2eComponentKind::BrowserDriver,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success() -> Value {
        json!({
            "ok": true, "typeOk": true, "idOk": true, "clientDataTypeOk": true,
            "originOk": true, "challengeOk": true, "rpIdHashOk": true, "upOk": true, "uvOk": true,
            "authenticatorDataLen": 37, "signatureLen": 70
        })
    }

    #[test]
    fn unknown_error_name_is_not_kept_or_printed() {
        // SECR-23: ブラウザが返す文字列をそのまま持たない・出力しない。
        let o = AuthenticationOutcome::from_value(
            &json!({ "ok": false, "errorName": "leaky-value-123" }),
        )
        .unwrap();
        let shown = format!("{o:?} {:?}", o.error_name());
        assert!(!shown.contains("leaky-value-123"), "{shown}");
        assert_eq!(o.error_name(), Some(WebAuthnErrorName::Other));
        assert!(!o.succeeded());
    }

    #[test]
    fn malformed_responses_are_none() {
        for v in [
            json!({}),
            json!({ "ok": "yes" }),
            json!({ "ok": false }),
            json!({ "ok": false, "errorName": 3 }),
            json!({ "ok": true, "typeOk": true }),
            {
                let mut s = success();
                s["signatureLen"] = json!("70");
                s
            },
        ] {
            assert!(AuthenticationOutcome::from_value(&v).is_none(), "{v}");
        }
    }

    #[test]
    fn any_false_check_or_short_length_is_not_success() {
        assert!(
            AuthenticationOutcome::from_value(&success())
                .unwrap()
                .succeeded()
        );
        for k in [
            "typeOk",
            "idOk",
            "clientDataTypeOk",
            "originOk",
            "challengeOk",
            "rpIdHashOk",
            "upOk",
            "uvOk",
        ] {
            let mut v = success();
            v[k] = json!(false);
            assert!(
                !AuthenticationOutcome::from_value(&v).unwrap().succeeded(),
                "{k}"
            );
        }
        for (k, n) in [("authenticatorDataLen", 36), ("signatureLen", 0)] {
            let mut v = success();
            v[k] = json!(n);
            assert!(
                !AuthenticationOutcome::from_value(&v).unwrap().succeeded(),
                "{k}"
            );
        }
    }

    #[test]
    fn origin_host_extracts_host_only_for_plain_origins() {
        assert_eq!(origin_host("http://localhost:8080"), Some("localhost"));
        assert_eq!(origin_host("http://localhost"), Some("localhost"));
        for bad in [
            "",
            "localhost",
            "https://localhost",
            "http://",
            "http://:80",
            "http://a@b:1",
            "http://a/b",
        ] {
            assert_eq!(origin_host(bad), None, "{bad}");
        }
    }

    #[test]
    fn credential_ref_validates_and_redacts() {
        for bad in [
            "",
            "a b",
            "a/b",
            "a=b",
            "日本",
            "Ab-_9",
            "AB",
            &"a".repeat(1367),
        ] {
            assert!(CredentialRef::new(bad).is_err(), "{bad:?}");
        }
        let c = CredentialRef::new("Ab-_0w").unwrap();
        let shown = format!("{c:?} {c:#?}");
        assert!(!shown.contains("Ab-_0w"), "{shown}");
        assert!(shown.contains("<redacted>"));
    }
}
