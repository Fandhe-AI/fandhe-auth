//! TASK-7 #60: 本人トークンの提示の模擬が、試験用の鍵で署名したトークンで動くことの最小の確認（網羅は #68）。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationExpectations, PresentationRejection, VerificationKey,
};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

const ISS: &str = "https://auth.example.test";
const AUD: &str = "https://hub.example.test";
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
    json!({"alg": "ES256", "typ": "at+jwt", "kid": "k1"})
}

fn claims() -> Value {
    json!({
        "iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_900,
        "jti": "jti-1", "sid": "sid-1", "client_id": "client-1", "scope": "openid",
        "https://auth.example.test/claims/auth_event_id": "evt-1"
    })
}

fn receiver(key: &EcdsaKeyPair) -> IdentityTokenReceiver {
    IdentityTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        PresentationExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

/// CLM-1・CLM-17: 試験用の鍵で署名した正常な本人トークンを受理して記録する。
#[test]
fn accepts_valid_token_and_records_claims() {
    let key = new_key();
    let mut r = receiver(&key);
    let got = r.present(&sign(&key, &header(), &claims()), NOW).unwrap();
    assert_eq!(got.iss, ISS);
    assert_eq!(got.sub, "user-1");
    assert_eq!(got.aud, vec![AUD.to_owned()]);
    assert_eq!((got.iat, got.exp), (900, 1_900));
    assert_eq!(got.jti, "jti-1");
    assert_eq!(got.sid, "sid-1");
    assert_eq!(got.client_id, "client-1");
    assert_eq!(got.scope, "openid");
    assert_eq!(got.auth_event_id.as_deref(), Some("evt-1"));
    assert_eq!(r.received(), &[got]);
}

/// SIGN-1: 別の鍵の署名は拒否し、記録を増やさない。
#[test]
fn rejects_signature_from_other_key() {
    let key = new_key();
    let mut r = receiver(&key);
    let t = sign(&new_key(), &header(), &claims());
    assert_eq!(
        r.present(&t, NOW),
        Err(PresentationRejection::SignatureMismatch)
    );
    assert!(r.received().is_empty());
}

/// SIGN-9・SIGN-10: typ の取り違えを拒否する。
#[test]
fn rejects_wrong_typ() {
    let key = new_key();
    let mut r = receiver(&key);
    let h = json!({"alg": "ES256", "typ": "JWT", "kid": "k1"});
    assert_eq!(
        r.present(&sign(&key, &h, &claims()), NOW),
        Err(PresentationRejection::HeaderMismatch)
    );
}

/// SIGN-17: exp == now は拒否する。
#[test]
fn rejects_exp_equal_to_now() {
    let key = new_key();
    let mut r = receiver(&key);
    let t = sign(&key, &header(), &claims());
    assert_eq!(r.present(&t, 1_900), Err(PresentationRejection::Expired));
}

/// CLM-1: iss・aud の不一致を拒否する。
#[test]
fn rejects_issuer_and_audience_mismatch() {
    let key = new_key();
    let mut r = receiver(&key);
    let mut c = claims();
    c["iss"] = json!("https://other.example.test");
    assert_eq!(
        r.present(&sign(&key, &header(), &c), NOW),
        Err(PresentationRejection::IssuerMismatch)
    );
    let mut c = claims();
    c["aud"] = json!("https://other.example.test");
    assert_eq!(
        r.present(&sign(&key, &header(), &c), NOW),
        Err(PresentationRejection::AudienceMismatch)
    );
}

/// SECR-23〜27: 拒否の表示にトークンの文字列もその各部も出ない。
#[test]
fn rejection_output_does_not_contain_token() {
    let key = new_key();
    let mut r = receiver(&key);
    let t = sign(&new_key(), &header(), &claims());
    let e = r.present(&t, NOW).unwrap_err();
    let out = format!("{e} {e:?}");
    assert!(!out.contains(&t));
    assert!(t.split('.').all(|part| !out.contains(part)));
}
