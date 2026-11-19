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
