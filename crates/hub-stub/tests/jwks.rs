//! TASK-7 #65: JWKS の取得の模擬の最小の確認（網羅は #68）。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値で、秒数も任意の小さな値。

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::HubStubConfig;
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use serde_json::Value;

const T0: u64 = 1_000;

fn config(max_age: u64, cache_max: u64) -> HubStubConfig {
    HubStubConfig {
        client_id: "hub-client.test".to_owned(),
        public_host: "hub.example.test".to_owned(),
        jwks_uri: "https://hub.example.test/jwks".to_owned(),
        logout_receiver_uri: "https://hub.example.test/logout".to_owned(),
        jwks_max_age: Duration::from_secs(max_age),
        jwks_fetch_min_interval: Duration::from_secs(2),
        jwks_cache_max: Duration::from_secs(cache_max),
        jwks_stale_max: Duration::from_secs(6),
        assertion_max_lifetime: Duration::from_secs(300),
        feed_fetch_min_interval: Duration::from_secs(5),
        heartbeat_loss_threshold: Duration::from_secs(10),
    }
}

fn keys_of(body: &str) -> Vec<Value> {
    let v: Value = serde_json::from_str(body).unwrap();
    v["keys"].as_array().unwrap().clone()
}

#[test]
fn jwks_has_only_public_fields_and_matches_key() {
    let key = AssertionSigningKey::generate("k1").unwrap();
    let mut ep = HubJwksEndpoint::new(&config(1, 0));
    ep.publish(&key);
    let r = ep.serve(T0);
    assert_eq!(r.status, 200);
    let keys = keys_of(&r.body);
    assert_eq!(keys.len(), 1);
    let obj = keys[0].as_object().unwrap();
    let mut names: Vec<&str> = obj.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["alg", "crv", "kid", "kty", "use", "x", "y"]);
    assert_eq!(obj["alg"], "ES256");
    assert_eq!(obj["kid"], "k1");
    let x = URL_SAFE_NO_PAD.decode(obj["x"].as_str().unwrap()).unwrap();
    let y = URL_SAFE_NO_PAD.decode(obj["y"].as_str().unwrap()).unwrap();
    let mut point = vec![0x04];
    point.extend(x);
    point.extend(y);
    assert_eq!(point, key.public_key());
}

#[test]
fn max_age_comes_from_config() {
    for n in [7_u64, 11] {
        let mut ep = HubJwksEndpoint::new(&config(n, 0));
        let r = ep.serve(T0);
        assert_eq!(r.max_age, n);
        assert_eq!(r.cache_control(), format!("max-age={n}"));
    }
}

#[test]
fn cache_max_bounds_how_long_old_version_is_served() {
    for cmax in [3_u64, 7] {
        let old = AssertionSigningKey::generate("old").unwrap();
        let new = AssertionSigningKey::generate("new").unwrap();
        let mut ep = HubJwksEndpoint::new(&config(1, cmax));
        ep.publish(&old);
        let first = ep.serve(T0);
        assert!(!first.from_cache);
        ep.retire("old");
        ep.publish(&new);
        // ちょうど C_max まではキャッシュ（古い版）を返す。
        let at_limit = ep.serve(T0 + cmax);
        assert!(at_limit.from_cache);
        assert_eq!(at_limit.version, first.version);
        assert_eq!(at_limit.age, cmax);
        // 1 秒超えると取り直して新しい版になる。
        let past = ep.serve(T0 + cmax + 1);
        assert!(!past.from_cache);
        assert_eq!(past.version, ep.current_version());
        assert_eq!(keys_of(&past.body)[0]["kid"], "new");
    }
}

#[test]
fn zero_cache_max_serves_origin_immediately() {
    let a = AssertionSigningKey::generate("a").unwrap();
    let b = AssertionSigningKey::generate("b").unwrap();
    let mut ep = HubJwksEndpoint::new(&config(1, 0));
    ep.publish(&a);
    ep.serve(T0);
    ep.publish(&b);
    let r = ep.serve(T0);
    assert!(!r.from_cache);
    assert_eq!(keys_of(&r.body).len(), 2);
}

#[test]
fn clock_rollback_refetches_origin() {
    let a = AssertionSigningKey::generate("a").unwrap();
    let mut ep = HubJwksEndpoint::new(&config(1, 100));
    ep.publish(&a);
    ep.serve(T0);
    ep.retire("a");
    let r = ep.serve(T0 - 1);
    assert!(!r.from_cache);
    assert!(keys_of(&r.body).is_empty());
}

#[test]
fn duplicate_kid_allowed_and_retire_empties() {
    let a = AssertionSigningKey::generate("dup").unwrap();
    let b = AssertionSigningKey::generate("dup").unwrap();
    let mut ep = HubJwksEndpoint::new(&config(1, 0));
    ep.publish(&a);
    ep.publish(&b);
    assert_eq!(keys_of(&ep.serve(T0).body).len(), 2);
    assert!(ep.retire("dup"));
    assert!(!ep.retire("dup"));
    assert!(keys_of(&ep.serve(T0).body).is_empty());
}

#[test]
fn invalid_points_are_rejected() {
    let mut ep = HubJwksEndpoint::new(&config(1, 0));
    assert!(ep.publish_es256("k", &[0x04; 64]).is_err());
    let mut bad_prefix = [0_u8; 65];
    bad_prefix[0] = 0x02;
    assert!(ep.publish_es256("k", &bad_prefix).is_err());
    assert_eq!(ep.current_version(), 0);
}

// ---- Hub が fandhe-auth の JWKS を取得する模擬（AuthJwksFetcher） ----

use std::collections::VecDeque;

use fandhe_auth_hub_stub::jwks::{
    AuthJwksFetcher, AuthJwksPort, AuthJwksResponse, JwksFetchError, JwksFetchOutcome,
    JwksFetchTrigger,
};
use fandhe_auth_hub_stub::presentation::VerificationKey;

fn config_b(min_interval: u64, stale_max: u64) -> HubStubConfig {
    let mut c = config(1, 0);
    c.jwks_fetch_min_interval = Duration::from_secs(min_interval);
    c.jwks_stale_max = Duration::from_secs(stale_max);
    c
}

/// 決めた応答を順に返し、口に届いた回数を数える偽の口。
struct FakeJwksPort {
    responses: VecDeque<AuthJwksResponse>,
    calls: usize,
}

impl FakeJwksPort {
    fn new(responses: Vec<AuthJwksResponse>) -> Self {
        Self {
            responses: responses.into(),
            calls: 0,
        }
    }
}

impl AuthJwksPort for FakeJwksPort {
    fn fetch_jwks(&mut self) -> AuthJwksResponse {
        self.calls += 1;
        self.responses.pop_front().unwrap()
    }
}

fn ok(body: String) -> AuthJwksResponse {
    AuthJwksResponse::Ok {
        body,
        max_age: Some(9),
    }
}

/// HubJwksEndpoint を origin として使い、本文を組み立てる。
fn body_of(keys: &[&AssertionSigningKey]) -> String {
    let mut ep = HubJwksEndpoint::new(&config(1, 0));
    for k in keys {
        ep.publish(k);
    }
    ep.serve(T0).body
}

fn expected(k: &AssertionSigningKey) -> VerificationKey {
    VerificationKey::es256(k.kid(), k.public_key())
}

#[test]
fn fetch_success_returns_keys_and_respects_min_interval() {
    let k = AssertionSigningKey::generate("k1").unwrap();
    let mut f = AuthJwksFetcher::new(&config_b(4, 100));
    let mut port = FakeJwksPort::new(vec![ok(body_of(&[&k])), ok(body_of(&[&k]))]);
    let s = f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
    assert_eq!((s.keys, s.skipped), (1, 0));
    assert_eq!(f.keys(T0), vec![expected(&k)]);
    assert_eq!(f.last_max_age(), Some(9));
    // 下限より前は口に届かない。
    assert_eq!(
        f.fetch(&mut port, JwksFetchTrigger::UnknownKid, T0 + 3),
        Err(JwksFetchError::TooSoon)
    );
    assert_eq!(port.calls, 1);
    // ちょうど下限なら取得する。
    assert!(
        f.fetch(&mut port, JwksFetchTrigger::Periodic, T0 + 4)
            .is_ok()
    );
    assert_eq!(port.calls, 2);
    // 時計の巻き戻りは TooSoon。
    assert_eq!(
        f.fetch(&mut port, JwksFetchTrigger::Periodic, T0),
        Err(JwksFetchError::TooSoon)
    );
    assert_eq!(f.records()[1].outcome, JwksFetchOutcome::TooSoon);
}

#[test]
fn bad_bodies_keep_previous_key_set() {
    let k = AssertionSigningKey::generate("k1").unwrap();
    let dup_a = AssertionSigningKey::generate("dup").unwrap();
    let dup_b = AssertionSigningKey::generate("dup").unwrap();
    let mut f = AuthJwksFetcher::new(&config_b(1, 100));
    let mut port = FakeJwksPort::new(vec![
        ok(body_of(&[&k])),
        ok(body_of(&[&dup_a, &dup_b])),
        ok("{not json".to_owned()),
        ok(r#"{"keys":[]}"#.to_owned()),
        AuthJwksResponse::Rejected { status: 503 },
    ]);
    f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
    let errs = [
        JwksFetchError::DuplicateKid,
        JwksFetchError::Malformed,
        JwksFetchError::NoUsableKey,
        JwksFetchError::Rejected { status: 503 },
    ];
    for (i, want) in errs.iter().enumerate() {
        let now = T0 + 1 + i as u64;
        assert_eq!(
            f.fetch(&mut port, JwksFetchTrigger::Periodic, now),
            Err(*want)
        );
        assert_eq!(f.keys(now), vec![expected(&k)]);
    }
    assert_eq!(f.last_success_at(), Some(T0));
    let outcomes: Vec<_> = f.records().iter().map(|r| r.outcome).collect();
    assert_eq!(outcomes[1], JwksFetchOutcome::DuplicateKid);
    assert_eq!(outcomes[2], JwksFetchOutcome::Malformed);
    assert_eq!(outcomes[3], JwksFetchOutcome::NoUsableKey);
    assert_eq!(outcomes[4], JwksFetchOutcome::Rejected { status: 503 });
}

#[test]
fn non_es256_keys_are_skipped_not_failed() {
    let k = AssertionSigningKey::generate("k1").unwrap();
    let mut v: Value = serde_json::from_str(&body_of(&[&k])).unwrap();
    v["keys"].as_array_mut().unwrap().push(serde_json::json!({
        "kty": "RSA", "kid": "r1", "alg": "RS256", "n": "AQAB", "e": "AQAB"
    }));
    let mut f = AuthJwksFetcher::new(&config_b(1, 100));
    let mut port = FakeJwksPort::new(vec![ok(v.to_string())]);
    let s = f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
    assert_eq!((s.keys, s.skipped), (1, 1));
}

#[test]
fn keys_are_dropped_after_stale_max() {
    for u in [3_u64, 8] {
        let k = AssertionSigningKey::generate("k1").unwrap();
        let mut f = AuthJwksFetcher::new(&config_b(1, u));
        assert!(f.keys(T0).is_empty());
        let mut port = FakeJwksPort::new(vec![ok(body_of(&[&k]))]);
        f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
        assert_eq!(f.keys(T0 + u).len(), 1);
        assert!(f.keys(T0 + u + 1).is_empty());
        assert!(f.keys(T0 - 1).is_empty());
    }
}

#[test]
fn keys_with_non_verify_use_or_key_ops_are_skipped() {
    let k = AssertionSigningKey::generate("k1").unwrap();
    let base: Value = serde_json::from_str(&body_of(&[&k])).unwrap();
    let template = base["keys"][0].clone();
    let cases = [
        ("use", serde_json::json!("enc")),
        ("key_ops", serde_json::json!(["sign"])),
    ];
    for (name, val) in cases {
        let mut bad = template.clone();
        bad["kid"] = serde_json::json!("bad");
        bad[name] = val;
        let mut v = base.clone();
        v["keys"].as_array_mut().unwrap().push(bad);
        let mut f = AuthJwksFetcher::new(&config_b(1, 100));
        let mut port = FakeJwksPort::new(vec![ok(v.to_string())]);
        let s = f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
        assert_eq!((s.keys, s.skipped), (1, 1), "{name}");
        assert_eq!(f.keys(T0), vec![expected(&k)]);
    }
    // use=sig・key_ops に verify を含む鍵は受ける
    let mut ok_key = template.clone();
    ok_key["use"] = serde_json::json!("sig");
    ok_key["key_ops"] = serde_json::json!(["verify"]);
    let mut v = base.clone();
    v["keys"] = serde_json::json!([ok_key]);
    let mut f = AuthJwksFetcher::new(&config_b(1, 100));
    let mut port = FakeJwksPort::new(vec![ok(v.to_string())]);
    let s = f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
    assert_eq!((s.keys, s.skipped), (1, 0));
}

#[test]
fn sub_second_stale_max_is_not_rounded_up() {
    let k = AssertionSigningKey::generate("k1").unwrap();
    let mut c = config_b(1, 0);
    c.jwks_stale_max = Duration::from_millis(500);
    let mut f = AuthJwksFetcher::new(&c);
    let mut port = FakeJwksPort::new(vec![ok(body_of(&[&k]))]);
    f.fetch(&mut port, JwksFetchTrigger::Periodic, T0).unwrap();
    assert_eq!(f.keys(T0).len(), 1);
    assert!(f.keys(T0 + 1).is_empty());
}
