//! TASK-7 #66・#68: 遅れと拒否の注入の確認。
//! #68 で提示の口の素通し・遅れと拒否の順序・`CallWindow` の境界・規則の選び方・記録の照合を足した。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値で、秒数も任意の小さな値。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::assertion::AssertionSigningKey;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::fault::{
    CallWindow, FaultRule, HubCallKind, HubFaultConfig, HubFaultInjector, InjectedRefusal,
    JwksRefusal, Refusal, RefusalRecord,
};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, HeartbeatReceiver, Liveness};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationExpectations, PresentationRejection, VerificationKey,
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
        client_id: StubValue::test("hub-client.test".to_owned()),
        public_host: StubValue::test("hub.example.test".to_owned()),
        jwks_uri: StubValue::test("https://hub.example.test/jwks".to_owned()),
        logout_receiver_uri: StubValue::test("https://hub.example.test/logout".to_owned()),
        jwks_max_age: StubValue::test(Duration::from_secs(1)),
        jwks_fetch_min_interval: StubValue::test(Duration::from_secs(2)),
        jwks_cache_max: StubValue::test(Duration::from_secs(3)),
        jwks_stale_max: StubValue::test(Duration::from_secs(6)),
        assertion_max_lifetime: StubValue::test(Duration::from_secs(300)),
        feed_fetch_min_interval: StubValue::test(Duration::from_secs(5)),
        heartbeat_loss_threshold: StubValue::test(Duration::from_secs(10)),
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

fn presentation_receiver(key: &EcdsaKeyPair) -> IdentityTokenReceiver {
    IdentityTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        PresentationExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

fn identity_token(key: &EcdsaKeyPair, typ: &str) -> String {
    let enc = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    let header = json!({"alg": "ES256", "typ": typ, "kid": "k1"});
    let claims = json!({
        "iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_900,
        "jti": "jti-1", "sid": "sid-1", "client_id": "client-1", "scope": "openid"
    });
    let input = format!("{}.{}", enc(&header), enc(&claims));
    let sig = key.sign(&SystemRandom::new(), input.as_bytes()).unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

/// AVLB-22: 提示の口は素通しで、受理は内側が行い、内側の拒否は注入の失敗にならずそのまま返る。
#[test]
fn presentation_passes_through_and_inner_rejection_is_returned_as_is() {
    let key = new_key();
    let mut rcv = presentation_receiver(&key);
    let mut inj = HubFaultInjector::new(HubFaultConfig::baseline(), noop());
    let ok = inj
        .present(&mut rcv, &identity_token(&key, "at+jwt"), NOW)
        .unwrap();
    assert!(ok.is_ok());
    assert_eq!(rcv.received().len(), 1);
    let bad = inj
        .present(&mut rcv, &identity_token(&key, "JWT"), NOW)
        .unwrap();
    assert_eq!(bad, Err(PresentationRejection::HeaderMismatch));
    assert_eq!(rcv.received().len(), 1);
    let kinds: Vec<_> = inj.log().records().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [HubCallKind::Presentation, HubCallKind::Presentation]
    );
}

/// 提示の口の遅れだけの規則は受理したうえで遅れを渡し、遅れと拒否の規則は内側を呼ばない。
#[test]
fn presentation_delay_only_and_delay_with_refusal() {
    let key = new_key();
    let mut rcv = presentation_receiver(&key);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let d1 = Duration::from_millis(3);
    let d2 = Duration::from_millis(5);
    let mut faults = HubFaultConfig::baseline();
    faults
        .presentation
        .push(rule::<Refusal>(1, Some(1), Some(d1), None));
    faults
        .presentation
        .push(rule(2, Some(2), Some(d2), Some(Refusal::Unreachable)));
    let mut inj = HubFaultInjector::new(faults, move |x| sink.borrow_mut().push(x));
    let t = identity_token(&key, "at+jwt");
    assert!(inj.present(&mut rcv, &t, NOW).unwrap().is_ok());
    assert_eq!(*seen.borrow(), vec![d1]);
    assert_eq!(
        inj.present(&mut rcv, &t, NOW).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert_eq!(*seen.borrow(), vec![d1, d2]);
    // 拒否の回は内側の記録を増やさない。
    assert_eq!(rcv.received().len(), 1);
}

/// 遅れと拒否の両方を持つ規則では、`Wait` が拒否の返却より先に呼ばれる。
#[test]
fn wait_is_called_before_refusal_is_returned() {
    let order = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&order);
    let mut faults = HubFaultConfig::baseline();
    faults.heartbeat.push(rule(
        1,
        None,
        Some(Duration::from_millis(2)),
        Some(Refusal::Status(502)),
    ));
    let mut inj = HubFaultInjector::new(faults, move |_| sink.borrow_mut().push("wait"));
    let mut hbr = HeartbeatReceiver::new(&config());
    let r = inj.receive_heartbeat(&mut hbr, &hb(), NOW);
    order.borrow_mut().push("returned");
    assert_eq!(r, Err(InjectedRefusal::Status(502)));
    assert_eq!(*order.borrow(), vec!["wait", "returned"]);
    assert!(hbr.records().is_empty());
}

/// `CallWindow::contains`: 両端を含み、`last: None` は以降すべて、`first == 0` は 1 と同じ。
#[test]
fn call_window_boundaries() {
    let w = CallWindow {
        first: 2,
        last: Some(4),
    };
    let got: Vec<bool> = (0..=6).map(|n| w.contains(n)).collect();
    assert_eq!(got, [false, false, true, true, true, false, false]);
    let open = CallWindow {
        first: 3,
        last: None,
    };
    assert!(!open.contains(2));
    assert!(open.contains(3));
    assert!(open.contains(u32::MAX));
    let zero = CallWindow {
        first: 0,
        last: Some(1),
    };
    assert!(!zero.contains(0));
    assert!(zero.contains(1));
    assert!(!zero.contains(2));
}

/// `first == 0` の規則は 1 回目から当たる（1 と同じ扱い）。
#[test]
fn rule_with_first_zero_applies_from_first_call() {
    let mut faults = HubFaultConfig::baseline();
    faults
        .heartbeat
        .push(rule(0, Some(1), None, Some(Refusal::Status(500))));
    let mut inj = HubFaultInjector::new(faults, noop());
    let mut hbr = HeartbeatReceiver::new(&config());
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap_err(),
        InjectedRefusal::Status(500)
    );
    assert!(inj.receive_heartbeat(&mut hbr, &hb(), NOW).is_ok());
}

/// 規則は先頭から見て最初に当たった 1 件だけを使い、後ろの規則の遅れ・拒否を足さない。
#[test]
fn only_first_matching_rule_is_used() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let first_delay = Duration::from_millis(1);
    let mut faults = HubFaultConfig::baseline();
    faults
        .heartbeat
        .push(rule::<Refusal>(1, None, Some(first_delay), None));
    faults.heartbeat.push(rule(
        1,
        None,
        Some(Duration::from_millis(9)),
        Some(Refusal::Status(500)),
    ));
    let mut inj = HubFaultInjector::new(faults, move |x| sink.borrow_mut().push(x));
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(*seen.borrow(), vec![first_delay]);
    assert_eq!(hbr.records().len(), 1);
    let fault = inj.log().records()[0].injected.unwrap();
    assert_eq!(fault.delay, Some(first_delay));
    assert_eq!(fault.refusal, None);
}

/// 記録は口ごとの通算の回数を持ち、種別・拒否の種類を `Presentation`・`Unreachable`・`Status` まで写す。
#[test]
fn log_records_kind_index_and_refusal_for_every_door() {
    let key = new_key();
    let mut faults = HubFaultConfig::baseline();
    faults
        .presentation
        .push(rule(1, Some(1), None, Some(Refusal::Unreachable)));
    faults
        .logout
        .push(rule(1, Some(1), None, Some(Refusal::Status(429))));
    faults
        .heartbeat
        .push(rule(1, Some(1), None, Some(Refusal::Status(503))));
    faults
        .jwks
        .push(rule(1, Some(1), None, Some(JwksRefusal::Status(500))));
    let mut inj = HubFaultInjector::new(faults, noop());
    let mut rcv = presentation_receiver(&key);
    let mut logout = logout_receiver(&key);
    let mut hbr = HeartbeatReceiver::new(&config());
    let mut jwks = HubJwksEndpoint::new(&config());
    let t = identity_token(&key, "at+jwt");
    let lt = logout_token(&key, "j1");
    assert!(inj.present(&mut rcv, &t, NOW).is_err());
    assert!(inj.receive_logout(&mut logout, &lt, NOW + 1).is_err());
    assert!(inj.receive_heartbeat(&mut hbr, &hb(), NOW + 2).is_err());
    assert!(inj.serve_jwks(&mut jwks, NOW + 3).is_err());
    // 2 回目は規則の外で、口ごとに通算が 2 になる。
    assert!(inj.present(&mut rcv, &t, NOW + 4).unwrap().is_ok());
    let got: Vec<_> = inj
        .log()
        .records()
        .iter()
        .map(|r| {
            (
                r.kind,
                r.call_index,
                r.at,
                r.injected.and_then(|f| f.refusal),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                HubCallKind::Presentation,
                1,
                NOW,
                Some(RefusalRecord::Unreachable)
            ),
            (
                HubCallKind::Logout,
                1,
                NOW + 1,
                Some(RefusalRecord::Status(429))
            ),
            (
                HubCallKind::Heartbeat,
                1,
                NOW + 2,
                Some(RefusalRecord::Status(503))
            ),
            (
                HubCallKind::Jwks,
                1,
                NOW + 3,
                Some(RefusalRecord::Status(500))
            ),
            (HubCallKind::Presentation, 2, NOW + 4, None),
        ]
    );
}
