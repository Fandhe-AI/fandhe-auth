//! TASK-7 #61・#68: Hub が `private_key_jwt` の assertion で口を呼ぶ模擬の確認。
//! #68 で `aud` の作り分け・`params()` の並び・`jti()` を足した。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値。
//!
//! 偽の口（`FakePort`）は `fandhe-auth` の口の代わりで、本体の検証部（OCL-22〜28・SIGN-7）の代わりではない。
//! 受け入れ条件の「呼び出しを受けられる」「検証の失敗が拒否される」を、口の拒否を stub が受理として
//! 扱わないことで確かめるための最小の判定だけを持つ。

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::assertion::{
    AssertionClaims, AssertionRequest, AssertionSigningKey, AssertionTarget, Audience, AuthPort,
    CallOutcome, CallRejected, CalledEndpoint, ClientAssertionType, HubAssertionCaller,
    PortResponse,
};
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use ring::signature::ECDSA_P256_SHA256_FIXED;
use ring::signature::UnparsedPublicKey;
use serde_json::{Value, json};

const ISS: &str = "https://auth.example.test";
const CLIENT_ID: &str = "hub-client.test";
const MAX: u64 = 300;
const NOW: u64 = 1_000;
const JWT_BEARER: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

fn config() -> HubStubConfig {
    HubStubConfig {
        client_id: StubValue::test(CLIENT_ID.to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(MAX)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
    }
}

fn caller() -> (HubAssertionCaller, Vec<u8>) {
    let key = AssertionSigningKey::generate("k1").unwrap();
    let public = key.public_key().to_vec();
    let target = AssertionTarget {
        issuer: ISS.to_owned(),
    };
    (HubAssertionCaller::new(&config(), target, key), public)
}

fn decode(part: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(part).unwrap()
}

/// 偽の口。公開鍵・`aud`・`exp − iat`・`client_assertion_type` だけを見て、不備なら 401 `invalid_client`。
struct FakePort {
    public_key: Option<Vec<u8>>,
    received: usize,
}

impl FakePort {
    fn accepting() -> Self {
        Self {
            public_key: None,
            received: 0,
        }
    }
    fn judging(public_key: Vec<u8>) -> Self {
        Self {
            public_key: Some(public_key),
            received: 0,
        }
    }
    fn reject() -> PortResponse {
        PortResponse::Rejected {
            status: 401,
            error: Some("invalid_client".to_owned()),
        }
    }
}

impl AuthPort for FakePort {
    fn call(&mut self, request: &AssertionRequest) -> PortResponse {
        self.received += 1;
        let Some(public) = &self.public_key else {
            return PortResponse::Accepted;
        };
        if request.param("client_assertion_type") != Some(JWT_BEARER) {
            return Self::reject();
        }
        let Some(token) = request.param("client_assertion") else {
            return Self::reject();
        };
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Self::reject();
        }
        let input = format!("{}.{}", parts[0], parts[1]);
        if UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public)
            .verify(input.as_bytes(), &decode(parts[2]))
            .is_err()
        {
            return Self::reject();
        }
        let c: Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        let (iat, exp) = (c["iat"].as_u64().unwrap(), c["exp"].as_u64().unwrap());
        if c["aud"] != json!(ISS) || exp.saturating_sub(iat) > MAX {
            return Self::reject();
        }
        PortResponse::Accepted
    }
}

fn conforming_request(c: &HubAssertionCaller, ep: CalledEndpoint) -> AssertionRequest {
    let a = c.sign(&c.conforming_claims("jti-1", NOW)).unwrap();
    c.request(ep, &a, ClientAssertionType::JwtBearer, true, vec![])
}

/// OCL-21〜25・OCL-27・SIGN-1・SIGN-16: 規則に合う assertion の中身。
#[test]
fn conforming_assertion_has_expected_shape() {
    let (c, public) = caller();
    let a = c.sign(&c.conforming_claims("jti-1", NOW)).unwrap();
    let parts: Vec<&str> = a.expose_compact().split('.').collect();
    assert_eq!(parts.len(), 3);
    let sig = decode(parts[2]);
    assert_eq!(sig.len(), 64, "OCL-23: r||s の 64 バイト");
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
        .unwrap();
    let h: Value = serde_json::from_slice(&decode(parts[0])).unwrap();
    assert_eq!(h, json!({"alg": "ES256", "kid": "k1"}));
    let p: Value = serde_json::from_slice(&decode(parts[1])).unwrap();
    assert_eq!(p["iss"], CLIENT_ID);
    assert_eq!(p["sub"], CLIENT_ID);
    assert_eq!(p["aud"], ISS);
    assert_eq!(p["exp"].as_u64().unwrap() - p["iat"].as_u64().unwrap(), MAX);
    assert_eq!(p["jti"], "jti-1");
}

/// OCL-21・OCL-24・OCL-30: 要求のパラメータと呼ぶ口の種類。
#[test]
fn request_carries_form_params_for_each_endpoint() {
    let (c, _) = caller();
    for ep in [
        CalledEndpoint::Token,
        CalledEndpoint::Introspection,
        CalledEndpoint::Revocation,
        CalledEndpoint::ChangeFeed,
        CalledEndpoint::AuditQuery,
    ] {
        let r = conforming_request(&c, ep);
        assert_eq!(r.endpoint(), ep);
        assert_eq!(r.param("client_assertion_type"), Some(JWT_BEARER));
        assert!(r.param("client_assertion").is_some());
        assert_eq!(r.param("client_id"), Some(CLIENT_ID));
    }
}

/// 受け入れ条件 1: 口が受理すると `Ok`。記録は受理 1 件。
#[test]
fn accepted_call_is_recorded() {
    let (mut c, _) = caller();
    let mut port = FakePort::accepting();
    let r = conforming_request(&c, CalledEndpoint::Token);
    assert_eq!(c.call(&mut port, &r), Ok(()));
    assert_eq!(port.received, 1);
    assert_eq!(c.records().len(), 1);
    assert_eq!(c.records()[0].outcome, CallOutcome::Accepted);
    assert_eq!(c.records()[0].jti.as_deref(), Some("jti-1"));
}

/// 受け入れ条件 2・OCL-21・OCL-25: 検証の失敗は拒否として返り、受理に数えない。
#[test]
fn rejected_calls_are_not_counted_as_accepted() {
    let (mut c, public) = caller();
    let mut port = FakePort::judging(public);

    // 合う assertion は通る（偽の口の判定が全拒否でないことの確認）。
    let ok = conforming_request(&c, CalledEndpoint::Introspection);
    assert_eq!(c.call(&mut port, &ok), Ok(()));

    // 別の鍵で署名した assertion。
    let (other, _) = caller();
    let a = other.sign(&other.conforming_claims("jti-2", NOW)).unwrap();
    let bad_sig = c.request(
        CalledEndpoint::Revocation,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![],
    );
    // exp − iat が上限 + 1 秒。
    let mut claims: AssertionClaims = c.conforming_claims("jti-3", NOW);
    claims.exp = Some(NOW + MAX + 1);
    let a = c.sign(&claims).unwrap();
    let too_long = c.request(
        CalledEndpoint::ChangeFeed,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![],
    );
    // client_assertion_type が欠けた・別の値。
    let a = c.sign(&c.conforming_claims("jti-4", NOW)).unwrap();
    let no_type = c.request(
        CalledEndpoint::AuditQuery,
        &a,
        ClientAssertionType::Omitted,
        true,
        vec![],
    );
    let other_type = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::Other("urn:example:test".to_owned()),
        true,
        vec![],
    );

    for r in [&bad_sig, &too_long, &no_type, &other_type] {
        assert_eq!(c.call(&mut port, r), Err(CallRejected { status: 401 }));
    }
    let accepted = c
        .records()
        .iter()
        .filter(|r| r.outcome == CallOutcome::Accepted)
        .count();
    assert_eq!(accepted, 1);
    assert_eq!(c.records().len(), 5);
    assert!(
        c.records()[1..]
            .iter()
            .all(|r| r.outcome == CallOutcome::Rejected { status: 401 })
    );
}

/// SECR-23〜27: assertion の文字列とその各部が `Debug`・`Display` に出ない。
#[test]
fn assertion_never_appears_in_debug_or_display() {
    let (mut c, _) = caller();
    let a = c.sign(&c.conforming_claims("jti-1", NOW)).unwrap();
    let compact = a.expose_compact().to_owned();
    let r = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![],
    );
    let mut port = FakePort::judging(vec![0; 65]); // 公開鍵が合わず拒否される。
    let err = c.call(&mut port, &r).unwrap_err();

    let key = AssertionSigningKey::generate("k1").unwrap();
    let outputs = [
        format!("{key:?}"),
        format!("{a:?}"),
        format!("{r:?}"),
        format!("{:?}", c.records()),
        format!("{err:?}"),
        format!("{err}"),
        format!("{c:?}"),
    ];
    for out in &outputs {
        assert!(!out.contains(&compact));
        for part in compact.split('.') {
            assert!(!out.contains(part));
        }
    }
}

/// OCL-23: P-256 以外の ECDSA 鍵（P-384）は鍵の読み込みで拒否され、ES256 のヘッダのまま別の署名が出ない。
#[test]
fn non_p256_key_is_rejected() {
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair};
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &SystemRandom::new())
            .unwrap();
    assert!(AssertionSigningKey::from_pkcs8("k1", pkcs8.as_ref()).is_err());
}

/// SECR-11・SECR-23〜27: `extra` の token 等の値も `Debug` に出ない。
#[test]
fn extra_values_never_appear_in_request_debug() {
    let (c, _) = caller();
    let a = c.sign(&c.conforming_claims("jti-1", NOW)).unwrap();
    let r = c.request(
        CalledEndpoint::Revocation,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![("token".to_owned(), "extra-secret-value".to_owned())],
    );
    let out = format!("{r:?}");
    assert!(!out.contains("extra-secret-value"));
    assert_eq!(r.param("token"), Some("extra-secret-value"));
}

/// OCL-22: `aud` は `Single` なら文字列、`List` なら配列で署名され、`nbf` を載せたときだけ出る。
#[test]
fn audience_forms_and_optional_claims_are_signed_as_given() {
    let (c, _) = caller();
    let payload = |claims: &AssertionClaims| -> Value {
        let a = c.sign(claims).unwrap();
        let parts: Vec<&str> = a.expose_compact().split('.').collect();
        serde_json::from_slice(&decode(parts[1])).unwrap()
    };

    let mut claims = c.conforming_claims("jti-a", NOW);
    assert_eq!(claims.aud, Audience::Single(ISS.to_owned()));
    let p = payload(&claims);
    assert_eq!(p["aud"], json!(ISS));
    assert!(p.get("nbf").is_none());

    claims.aud = Audience::List(vec![
        ISS.to_owned(),
        "https://other.example.test".to_owned(),
    ]);
    claims.nbf = Some(NOW - 1);
    let p = payload(&claims);
    assert_eq!(p["aud"], json!([ISS, "https://other.example.test"]));
    assert_eq!(p["nbf"], json!(NOW - 1));

    // OCL-25・OCL-27: iat・exp・jti の欠けは欄ごと出ない。
    claims.iat = None;
    claims.exp = None;
    claims.jti = None;
    let p = payload(&claims);
    for name in ["iat", "exp", "jti"] {
        assert!(p.get(name).is_none(), "{name}");
    }
}

/// OCL-30: `params()` は名前の並びと順序を保つ。`client_id` を含めない形・`extra` の追加も作れる。
#[test]
fn params_keep_order_and_names() {
    let (c, _) = caller();
    let a = c.sign(&c.conforming_claims("jti-p", NOW)).unwrap();
    let with = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![("grant_type".to_owned(), "x".to_owned())],
    );
    let names: Vec<&str> = with.params().map(|(k, _)| k).collect();
    assert_eq!(
        names,
        [
            "client_assertion_type",
            "client_assertion",
            "client_id",
            "grant_type"
        ]
    );
    let without = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::Omitted,
        false,
        vec![],
    );
    let names: Vec<&str> = without.params().map(|(k, _)| k).collect();
    assert_eq!(names, ["client_assertion"]);
    assert_eq!(without.param("client_id"), None);
}

/// OCL-27: 要求の `jti()` は assertion の `jti` の写しで、`jti` の無い assertion では `None`。
#[test]
fn request_jti_mirrors_assertion_jti() {
    let (c, _) = caller();
    let a = c.sign(&c.conforming_claims("jti-q", NOW)).unwrap();
    let r = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![],
    );
    assert_eq!(r.jti(), Some("jti-q"));
    let mut claims = c.conforming_claims("jti-r", NOW);
    claims.jti = None;
    let a = c.sign(&claims).unwrap();
    let r = c.request(
        CalledEndpoint::Token,
        &a,
        ClientAssertionType::JwtBearer,
        true,
        vec![],
    );
    assert_eq!(r.jti(), None);
}
