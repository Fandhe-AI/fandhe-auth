//! TASK-7 #66: 遅れと拒否の注入の最小の確認（網羅は #68）。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値で、秒数も任意の小さな値。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::HubStubConfig;
use fandhe_auth_hub_stub::fault::{
    CallWindow, FaultRule, HubCallKind, HubFaultConfig, HubFaultInjector, InjectedRefusal,
    JwksRefusal, Refusal, RefusalRecord,
};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, HeartbeatReceiver, Liveness};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationExpectations, VerificationKey,
};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

const ISS: &str = "https://auth.example.test";
const AUD: &str = "hub-client-test";
const EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";
const NOW: u64 = 1_000;
const TOKEN_MARK: &str = "marker-instance.test";

fn config() -> HubStubConfig {
    HubStubConfig {
        client_id: "hub-client.test".to_owned(),
        public_host: "hub.example.test".to_owned(),
        jwks_uri: "https://hub.example.test/jwks".to_owned(),
        logout_receiver_uri: "https://hub.example.test/logout".to_owned(),
        jwks_max_age: Duration::from_secs(1),
        jwks_fetch_min_interval: Duration::from_secs(2),
        jwks_cache_max: Duration::from_secs(3),
        jwks_stale_max: Duration::from_secs(6),
        assertion_max_lifetime: Duration::from_secs(300),
        feed_fetch_min_interval: Duration::from_secs(5),
        heartbeat_loss_threshold: Duration::from_secs(10),
    }
}

fn new_key() -> EcdsaKeyPair {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap()
}

fn logout_token(key: &EcdsaKeyPair, jti: &str) -> String {
    let enc = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    let header = json!({"alg": "ES256", "typ": "logout+jwt", "kid": "k1"});
    let claims = json!({
        "iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_020,
        "jti": jti, "sid": "sid-1", "events": {EVENT: {}}
    });
    let input = format!("{}.{}", enc(&header), enc(&claims));
    let sig = key.sign(&SystemRandom::new(), input.as_bytes()).unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

fn logout_receiver(key: &EcdsaKeyPair) -> LogoutTokenReceiver {
    LogoutTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        LogoutExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

fn rule<R>(
    first: u32,
    last: Option<u32>,
    delay: Option<Duration>,
    refusal: Option<R>,
) -> FaultRule<R> {
    FaultRule {
        calls: CallWindow { first, last },
        delay,
        refusal,
    }
}

fn hb() -> Heartbeat {
    Heartbeat {
        instance: TOKEN_MARK.to_owned(),
        db: Some(DbState::Ok),
        pending: Some(0),
    }
}

fn noop() -> impl FnMut(Duration) {
    |_| {}
}

/// 基準線は 4 つの口が素通しで、記録が呼んだ順に注入なしで積まれる。
#[test]
fn baseline_passes_through_and_logs_in_order() {
    let key = new_key();
    let mut inj = HubFaultInjector::new(HubFaultConfig::baseline(), noop());
    let mut logout = logout_receiver(&key);
    let mut hbr = HeartbeatReceiver::new(&config());
    let mut jwks = HubJwksEndpoint::new(&config());
    assert!(
        inj.receive_logout(&mut logout, &logout_token(&key, "j1"), NOW)
            .unwrap()
            .is_ok()
    );
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(inj.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    let kinds: Vec<_> = inj
        .log()
        .records()
        .iter()
        .map(|r| (r.kind, r.call_index, r.at))
        .collect();
    assert_eq!(
        kinds,
        [
            (HubCallKind::Logout, 1, NOW),
            (HubCallKind::Heartbeat, 1, NOW),
            (HubCallKind::Jwks, 1, NOW)
        ]
    );
    assert!(inj.log().records().iter().all(|r| r.injected.is_none()));
}

/// RVPG-28: 拒否した通知は既受信にならず、再送が重複扱いにならずに受理される。
#[test]
fn refused_logout_is_not_recorded_and_resend_is_accepted() {
    let key = new_key();
    let mut faults = HubFaultConfig::baseline();
    faults
        .logout
        .push(rule(1, Some(1), None, Some(Refusal::Status(503))));
    let mut inj = HubFaultInjector::new(faults, noop());
    let mut logout = logout_receiver(&key);
    let t = logout_token(&key, "j1");
    assert_eq!(
        inj.receive_logout(&mut logout, &t, NOW).unwrap_err(),
        InjectedRefusal::Status(503)
    );
    assert!(logout.received().is_empty());
    let got = inj.receive_logout(&mut logout, &t, NOW).unwrap().unwrap();
    assert!(!got.duplicate);
}

/// 遅れだけの規則では受理されたうえで、フックに同じ遅れが渡る。
#[test]
fn delay_only_rule_accepts_and_hands_delay_to_hook() {
    let key = new_key();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let d = Duration::from_millis(7);
    let mut faults = HubFaultConfig::baseline();
    faults.logout.push(rule::<Refusal>(1, None, Some(d), None));
    let mut inj = HubFaultInjector::new(faults, move |x| sink.borrow_mut().push(x));
    let mut logout = logout_receiver(&key);
    assert!(
        inj.receive_logout(&mut logout, &logout_token(&key, "j1"), NOW)
            .unwrap()
            .is_ok()
    );
    assert_eq!(*seen.borrow(), vec![d]);
    assert_eq!(logout.received().len(), 1);
}

/// Hub AUTHC-21: 拒否した生存確認は最終受信時刻を進めず、途絶が作れる。
#[test]
fn refused_heartbeat_does_not_advance_liveness() {
    let mut faults = HubFaultConfig::baseline();
    faults
        .heartbeat
        .push(rule(2, None, None, Some(Refusal::Unreachable)));
    let mut inj = HubFaultInjector::new(faults, noop());
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW + 8).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert_eq!(hbr.records().len(), 1);
    assert_eq!(hbr.status(TOKEN_MARK, NOW + 11), Liveness::Lost);
}

/// OCL-36: JWKS の各拒否で内側の記録と版が変わらず、壊れた本文は解析できない。
#[test]
fn jwks_refusals_leave_inner_untouched() {
    let mut jwks = HubJwksEndpoint::new(&config());
    jwks.publish(&AssertionSigningKey::generate("k1").unwrap());
    let mut faults = HubFaultConfig::baseline();
    faults
        .jwks
        .push(rule(1, Some(1), None, Some(JwksRefusal::Unreachable)));
    faults
        .jwks
        .push(rule(2, Some(2), None, Some(JwksRefusal::Status(503))));
    faults
        .jwks
        .push(rule(3, Some(3), None, Some(JwksRefusal::MalformedBody)));
    let mut inj = HubFaultInjector::new(faults, noop());
    let v = jwks.current_version();
    assert_eq!(
        inj.serve_jwks(&mut jwks, NOW).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert_eq!(
        inj.serve_jwks(&mut jwks, NOW).unwrap_err(),
        InjectedRefusal::Status(503)
    );
    let bad = inj.serve_jwks(&mut jwks, NOW).unwrap();
    assert!(serde_json::from_str::<Value>(&bad.body).is_err());
    assert!(jwks.records().is_empty());
    assert_eq!(jwks.current_version(), v);
    // 4 回目は規則の外で素通し。
    assert!(!inj.serve_jwks(&mut jwks, NOW).unwrap().from_cache);
    assert_eq!(jwks.records().len(), 1);
    assert_eq!(
        inj.log().records()[2].injected.unwrap().refusal,
        Some(RefusalRecord::MalformedBody)
    );
}

/// AVLB-18: `Status(200)` でも成功を作らない。
#[test]
fn status_200_is_still_a_refusal() {
    let mut faults = HubFaultConfig::baseline();
    faults
        .presentation
        .push(rule(1, None, None, Some(Refusal::Status(200))));
    let mut inj = HubFaultInjector::new(faults, noop());
    let key = new_key();
    let mut rcv = IdentityTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        PresentationExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    );
    assert_eq!(
        inj.present(&mut rcv, "x.y.z", NOW).unwrap_err(),
        InjectedRefusal::Status(200)
    );
    assert!(rcv.received().is_empty());
}

/// 回数は通算で、`set_faults` の後も戻らない。
#[test]
fn call_count_is_cumulative_across_set_faults() {
    let mut inj = HubFaultInjector::new(HubFaultConfig::baseline(), noop());
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    let mut faults = HubFaultConfig::baseline();
    faults
        .heartbeat
        .push(rule(2, Some(2), None, Some(Refusal::Status(500))));
    inj.set_faults(faults);
    assert!(inj.receive_heartbeat(&mut hbr, &hb(), NOW).is_err());
    assert!(inj.receive_heartbeat(&mut hbr, &hb(), NOW).is_ok());
}

/// SECR-23〜27: 注入器の Debug と記録に token・instance が現れない。
#[test]
fn debug_and_log_do_not_contain_values() {
    let key = new_key();
    let t = logout_token(&key, "j1");
    let mut inj = HubFaultInjector::new(HubFaultConfig::baseline(), noop());
    let mut logout = logout_receiver(&key);
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.receive_logout(&mut logout, &t, NOW).unwrap().unwrap();
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    let text = format!("{inj:?} {:?}", inj.log());
    assert!(!text.contains(&t));
    assert!(!text.contains(TOKEN_MARK));
}
