//! TASK-7 #62: 失効の通知の受け口の模擬が、試験用の鍵で署名した Logout Token で動くことの最小の確認（網羅は #68）。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutRejection, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::VerificationKey;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

const ISS: &str = "https://auth.example.test";
const AUD: &str = "hub-client-test";
const EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";
const NOW: u64 = 1_000;

fn new_key() -> EcdsaKeyPair {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap()
}

fn sign(key: &EcdsaKeyPair, header: &Value, claims: &Value) -> String {
    let enc = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    let input = format!("{}.{}", enc(header), enc(claims));
    let sig = key.sign(&SystemRandom::new(), input.as_bytes()).unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

fn header() -> Value {
    json!({"alg": "ES256", "typ": "logout+jwt", "kid": "k1"})
}

fn claims(jti: &str, sid: &str) -> Value {
    json!({
        "iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_020,
        "jti": jti, "sid": sid, "events": {EVENT: {}}
    })
}

fn receiver(key: &EcdsaKeyPair) -> LogoutTokenReceiver {
    LogoutTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        LogoutExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

/// RVPG-7: 正常な Logout Token を受理して記録する。
#[test]
fn accepts_valid_logout_token_and_records() {
    let key = new_key();
    let mut r = receiver(&key);
    let got = r
        .receive(&sign(&key, &header(), &claims("j1", "sid-1")), NOW)
        .unwrap();
    assert_eq!((got.sid.as_str(), got.jti.as_str()), ("sid-1", "j1"));
    assert_eq!(got.aud, vec![AUD.to_owned()]);
    assert!(!got.duplicate);
    assert_eq!(r.received(), &[got]);
}

/// RVPG-28: 同じ jti の再送は印付きで記録し、適用には数えない。
#[test]
fn marks_resent_jti_as_duplicate() {
    let key = new_key();
    let mut r = receiver(&key);
    let t = sign(&key, &header(), &claims("j1", "sid-1"));
    r.receive(&t, NOW).unwrap();
    assert!(r.receive(&t, NOW).unwrap().duplicate);
    assert_eq!(r.received().len(), 2);
    assert_eq!(r.applied().count(), 1);
}

/// SIGN-1: 別の鍵の署名は拒否し、記録を増やさない。
#[test]
fn rejects_signature_from_other_key() {
    let key = new_key();
    let mut r = receiver(&key);
    let t = sign(&new_key(), &header(), &claims("j1", "s"));
    assert_eq!(r.receive(&t, NOW), Err(LogoutRejection::SignatureMismatch));
    assert!(r.received().is_empty());
}

/// SIGN-9・SIGN-10: typ が logout+jwt 以外（アクセストークンの typ 等）は拒否する。
#[test]
fn rejects_wrong_typ() {
    let key = new_key();
    let mut r = receiver(&key);
    let h = json!({"alg": "ES256", "typ": "at+jwt", "kid": "k1"});
    let t = sign(&key, &h, &claims("j1", "s"));
    assert_eq!(r.receive(&t, NOW), Err(LogoutRejection::HeaderMismatch));
}

/// SIGN-1: alg が ES256 以外（none 含む）は拒否する。
#[test]
fn rejects_wrong_alg() {
    let key = new_key();
    let mut r = receiver(&key);
    let h = json!({"alg": "none", "typ": "logout+jwt", "kid": "k1"});
    let t = sign(&key, &h, &claims("j1", "s"));
    assert_eq!(r.receive(&t, NOW), Err(LogoutRejection::HeaderMismatch));
}

/// RVPG-7: 禁止のクレーム（nonce 等）を持つ通知は拒否する。
#[test]
fn rejects_forbidden_claims() {
    let key = new_key();
    for name in ["nonce", "scope", "at_hash", "client_id"] {
        let mut r = receiver(&key);
        let mut c = claims("j1", "s");
        c[name] = json!("x");
        assert_eq!(
            r.receive(&sign(&key, &header(), &c), NOW),
            Err(LogoutRejection::ForbiddenClaim),
            "{name}"
        );
    }
}

/// RVPG-7: 必須クレームの欠け・events の不一致を拒否する。
#[test]
fn rejects_missing_claim_and_bad_event() {
    let key = new_key();
    let mut r = receiver(&key);
    let mut c = claims("j1", "s");
    c.as_object_mut().unwrap().remove("sid");
    assert_eq!(
        r.receive(&sign(&key, &header(), &c), NOW),
        Err(LogoutRejection::MissingClaim)
    );
    let mut c = claims("j1", "s");
    c["events"] = json!({"http://example.test/other": {}});
    assert_eq!(
        r.receive(&sign(&key, &header(), &c), NOW),
        Err(LogoutRejection::EventMismatch)
    );
    assert!(r.received().is_empty());
}

/// SIGN-17: iss・aud の不一致と exp == now を拒否する。
#[test]
fn rejects_issuer_audience_and_expiry() {
    let key = new_key();
    let mut r = receiver(&key);
    let mut c = claims("j1", "s");
    c["iss"] = json!("https://other.example.test");
    assert_eq!(
        r.receive(&sign(&key, &header(), &c), NOW),
        Err(LogoutRejection::IssuerMismatch)
    );
    let mut c = claims("j1", "s");
    c["aud"] = json!("someone-else");
    assert_eq!(
        r.receive(&sign(&key, &header(), &c), NOW),
        Err(LogoutRejection::AudienceMismatch)
    );
    let c = claims("j1", "s");
    assert_eq!(
        r.receive(&sign(&key, &header(), &c), 1_020),
        Err(LogoutRejection::Expired)
    );
}

/// 壊れた形は Malformed。拒否の文言にトークンの値を含めない（SECR-23〜27）。
#[test]
fn rejects_malformed_without_leaking() {
    let key = new_key();
    let mut r = receiver(&key);
    let e = r.receive("secret-looking.token", NOW).unwrap_err();
    assert_eq!(e, LogoutRejection::Malformed);
    assert!(!e.to_string().contains("secret-looking"));
}
