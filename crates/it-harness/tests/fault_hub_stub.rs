//! TASK-13 (5/8): Hub の stub の遅れと拒否の注入の試験。Docker・ネットワーク・鍵の生成なし。
//! 値は予約 TLD `.test` の仮の値で、遅れの数値も任意の小さな値（THRT-29・A-2026-10-28-1）。
//! 鍵が無いので、提示・失効の通知は壊れた token で「内側に届いたか」を確かめる。
//! 記録は事実の出力で合否の判定ではない。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
//! 根拠: AVLB-18・AVLB-22・RVPG-28・OCL-36・Hub AUTHC-21・SECR-23。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::config::{HubStubConfig, StubValue};
use fandhe_auth_hub_stub::fault::{InjectedRefusal, InjectionLog};
use fandhe_auth_hub_stub::heartbeat::{DbState, Heartbeat, HeartbeatReceiver, Liveness};
use fandhe_auth_hub_stub::jwks::HubJwksEndpoint;
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutRejection, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::{
    IdentityTokenReceiver, PresentationExpectations, PresentationRejection, VerificationKey,
};
use fandhe_auth_it_harness::fault::{
    FaultKind, FaultRecordError, HubStubFaultError, HubStubFaultInjector, HubStubPort,
    HubStubRefusalKind,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{
    FaultRecord, FaultTimeline, Harness, HarnessConfig, HubStubComponent,
};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

const NOW: u64 = 1_000;
const ISS: &str = "https://auth.example.test";
const AUD: &str = "hub-client.test";
const EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// 漏えい検査用の値。実行ごとに乱数から作り、コミットしない（secret-values.md）。
fn canary() -> String {
    let mut b = [0_u8; 16];
    SystemRandom::new().fill(&mut b).unwrap();
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("canary-{hex}.test")
}

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

fn logout_token(key: &EcdsaKeyPair) -> String {
    sign(
        key,
        &json!({"alg": "ES256", "typ": "logout+jwt", "kid": "k1"}),
        &json!({"iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_020,
                "jti": "j1", "sid": "sid-1", "events": {EVENT: {}}}),
    )
}

fn identity_token(key: &EcdsaKeyPair) -> String {
    sign(
        key,
        &json!({"alg": "ES256", "typ": "at+jwt", "kid": "k1"}),
        &json!({"iss": ISS, "sub": "user-1", "aud": AUD, "iat": 900, "exp": 1_900,
                "jti": "jti-1", "sid": "sid-1", "client_id": "client-1", "scope": "openid"}),
    )
}

fn keyed_logout(key: &EcdsaKeyPair) -> LogoutTokenReceiver {
    LogoutTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        LogoutExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

fn keyed_presenter(key: &EcdsaKeyPair) -> IdentityTokenReceiver {
    IdentityTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        PresentationExpectations {
            issuer: ISS.to_owned(),
            audience: AUD.to_owned(),
        },
    )
}

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

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn timeline() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), t0()).unwrap()
}

fn finish(tl: FaultTimeline) -> FaultRecord {
    tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap()
}

type Waits = Rc<RefCell<Vec<Duration>>>;

/// 注入器は 1 つの実行（`tl`）に結ぶ。
fn injector(tl: &FaultTimeline) -> (HubStubFaultInjector<impl FnMut(Duration) + use<>>, Waits) {
    let waits: Waits = Rc::default();
    let w = Rc::clone(&waits);
    (
        HubStubFaultInjector::new(move |d: Duration| w.borrow_mut().push(d), tl),
        waits,
    )
}

fn presenter() -> IdentityTokenReceiver {
    IdentityTokenReceiver::new(
        Vec::new(),
        PresentationExpectations {
            issuer: "https://auth.example.test".to_owned(),
            audience: "hub-client.test".to_owned(),
        },
    )
}

fn logout_rx() -> LogoutTokenReceiver {
    LogoutTokenReceiver::new(
        Vec::new(),
        LogoutExpectations {
            issuer: "https://auth.example.test".to_owned(),
            audience: "hub-client.test".to_owned(),
        },
    )
}

fn hb() -> Heartbeat {
    Heartbeat {
        instance: "a.test".to_owned(),
        db: Some(DbState::Ok),
        pending: Some(0),
    }
}

fn refusal(port: HubStubPort, r: HubStubRefusalKind) -> FaultKind {
    FaultKind::hub_stub_refusal(port, r).unwrap()
}

fn delay(port: HubStubPort, ms: u64) -> FaultKind {
    FaultKind::hub_stub_delay(port, Duration::from_millis(ms)).unwrap()
}

fn injected_flags(log: &InjectionLog) -> Vec<bool> {
    log.records().iter().map(|r| r.injected.is_some()).collect()
}

/// OCL-36: JWKS の拒否は状態もキャッシュも変えず、解除の後は 200 で事前に載せた鍵が含まれる。
#[test]
fn jwks_status_refusal_then_release() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut jwks = HubJwksEndpoint::new(&config());
    let key = new_key();
    jwks.publish_es256("k-expected", key.public_key().as_ref())
        .unwrap();
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    inj.apply(&mut tl, k).unwrap();
    let v = jwks.current_version();
    let n = jwks.records().len();
    assert_eq!(
        inj.serve_jwks(&mut jwks, NOW).unwrap_err(),
        InjectedRefusal::Status(503)
    );
    assert_eq!(jwks.current_version(), v);
    assert_eq!(jwks.records().len(), n);
    inj.release(&mut tl, k).unwrap();
    assert_eq!(inj.active_count(), 0);
    let ok = inj.serve_jwks(&mut jwks, NOW + 1).unwrap();
    assert_eq!(ok.status, 200);
    assert_eq!(ok.version, v);
    let body: Value = serde_json::from_str(&ok.body).unwrap();
    let kids: Vec<&str> = body["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["kid"].as_str().unwrap())
        .collect();
    assert_eq!(kids, ["k-expected"]);
    assert_eq!(jwks.records().len(), n + 1);
    assert_eq!(injected_flags(inj.call_log()), [true, false]);
}

/// 壊れた JWKS は HTTP 200 のまま本文が構文エラーになる。注入前は同じ口の本文が解釈できる。
#[test]
fn jwks_malformed_body_is_200_but_unparsable() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut jwks = HubJwksEndpoint::new(&config());
    let key = new_key();
    jwks.publish_es256("k1", key.public_key().as_ref()).unwrap();
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::MalformedBody);
    inj.apply(&mut tl, k).unwrap();
    let r = inj.serve_jwks(&mut jwks, NOW).unwrap();
    assert_eq!(r.status, 200);
    assert!(serde_json::from_str::<Value>(&r.body).is_err());
    inj.release(&mut tl, k).unwrap();
    let ok = inj.serve_jwks(&mut jwks, NOW + 1).unwrap();
    assert!(serde_json::from_str::<Value>(&ok.body).is_ok());
}

/// Hub AUTHC-21: 受理した後に拒否を続けて閾値をまたぐと途絶になり、解除の後の再受理で生存に戻る。
#[test]
fn heartbeat_refusal_makes_loss_and_release_recovers() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut hbr = HeartbeatReceiver::new(&config());
    // 注入前: 正常な生存確認を受理する。
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(hbr.status("a.test", NOW), Liveness::Alive);
    let k = refusal(HubStubPort::Heartbeat, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, k).unwrap();
    // 閾値（10 秒）の内側では拒否していても生存。
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW + 5).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert_eq!(hbr.status("a.test", NOW + 5), Liveness::Alive);
    // 閾値をまたぐまで拒否を続けると途絶。
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW + 11)
            .unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert_eq!(hbr.records().len(), 1);
    assert_eq!(hbr.status("a.test", NOW + 11), Liveness::Lost);
    inj.release(&mut tl, k).unwrap();
    inj.receive_heartbeat(&mut hbr, &hb(), NOW + 12).unwrap();
    assert_eq!(hbr.records().len(), 2);
    assert_eq!(hbr.status("a.test", NOW + 13), Liveness::Alive);
    assert_eq!(hbr.status("a.test", NOW + 12 + 100), Liveness::Lost);
}

/// RVPG-28: 内側で受理できる token を拒否の期間に渡しても受信側の状態は変わらず、
/// 解除の後に同じ token を初回（再送でない）として受理できる。
#[test]
fn logout_and_presentation_refusal_do_not_reach_inner() {
    let key = new_key();
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut lo = keyed_logout(&key);
    let mut pr = keyed_presenter(&key);
    let lt = logout_token(&key);
    let it = identity_token(&key);
    let kl = refusal(HubStubPort::Logout, HubStubRefusalKind::Status(500));
    let kp = refusal(HubStubPort::Presentation, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, kl).unwrap();
    inj.apply(&mut tl, kp).unwrap();
    assert_eq!(
        inj.receive_logout(&mut lo, &lt, NOW).unwrap_err(),
        InjectedRefusal::Status(500)
    );
    assert_eq!(
        inj.present(&mut pr, &it, NOW).unwrap_err(),
        InjectedRefusal::Unreachable
    );
    assert!(lo.received().is_empty());
    assert!(pr.received().is_empty());
    inj.release(&mut tl, kl).unwrap();
    inj.release(&mut tl, kp).unwrap();
    let got = inj.receive_logout(&mut lo, &lt, NOW + 1).unwrap().unwrap();
    assert!(!got.duplicate);
    assert_eq!(lo.received().len(), 1);
    assert_eq!(lo.applied().count(), 1);
    assert!(inj.present(&mut pr, &it, NOW + 1).unwrap().is_ok());
    assert_eq!(pr.received().len(), 1);
    // 内側が拒否する token は、注入がなくても拒否のまま（壊れた token は内側に届いて Malformed）。
    assert_eq!(
        inj.receive_logout(&mut lo, "x.y.z", NOW + 2).unwrap(),
        Err(LogoutRejection::Malformed)
    );
    assert_eq!(
        inj.present(&mut pr, "x.y.z", NOW + 2).unwrap(),
        Err(PresentationRejection::Malformed)
    );
}

#[test]
fn delay_reaches_wait_on_all_ports_and_passes_result_through() {
    let mut tl = timeline();
    let (mut inj, waits) = injector(&tl);
    let mut lo = logout_rx();
    let mut pr = presenter();
    let mut hbr = HeartbeatReceiver::new(&config());
    let mut jwks = HubJwksEndpoint::new(&config());
    let c = canary();
    for p in [
        HubStubPort::Presentation,
        HubStubPort::Logout,
        HubStubPort::Heartbeat,
        HubStubPort::Jwks,
    ] {
        inj.apply(&mut tl, delay(p, 7)).unwrap();
    }
    assert!(inj.present(&mut pr, &c, NOW).unwrap().is_err());
    assert!(inj.receive_logout(&mut lo, &c, NOW).unwrap().is_err());
    inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap();
    assert_eq!(inj.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    assert_eq!(*waits.borrow(), vec![Duration::from_millis(7); 4]);
    assert_eq!(hbr.status("a.test", NOW), Liveness::Alive);
}

/// 同じ口の遅れと拒否は 1 つの規則に合成される。片方が黙って落ちない。
#[test]
fn delay_and_refusal_on_same_port_are_combined() {
    let mut tl = timeline();
    let (mut inj, waits) = injector(&tl);
    let mut hbr = HeartbeatReceiver::new(&config());
    inj.apply(&mut tl, delay(HubStubPort::Heartbeat, 5))
        .unwrap();
    inj.apply(
        &mut tl,
        refusal(HubStubPort::Heartbeat, HubStubRefusalKind::Status(502)),
    )
    .unwrap();
    assert_eq!(inj.active_count(), 2);
    assert_eq!(
        inj.receive_heartbeat(&mut hbr, &hb(), NOW).unwrap_err(),
        InjectedRefusal::Status(502)
    );
    assert_eq!(*waits.borrow(), vec![Duration::from_millis(5)]);
    assert!(hbr.records().is_empty());
}

/// 基準線では記録が拒否し、注入もしない。
#[test]
fn baseline_rejects_apply_and_injects_nothing() {
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let (mut inj, waits) = injector(&tl);
    let mut jwks = HubJwksEndpoint::new(&config());
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    assert_eq!(
        inj.apply(&mut tl, k).unwrap_err(),
        HubStubFaultError::Record(FaultRecordError::EventsInBaseline)
    );
    assert_eq!(inj.active_count(), 0);
    assert_eq!(inj.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    assert_eq!(injected_flags(inj.call_log()), [false]);
    assert!(waits.borrow().is_empty());
    let text = finish(tl).to_string();
    assert!(text.contains("run_mode: baseline"));
    assert!(text.contains("injection_count: 0"));
}

#[test]
fn invalid_params_are_rejected_without_recording() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let bad = [
        FaultKind::DbBlock,
        FaultKind::HubStubRefusal {
            port: HubStubPort::Logout,
            refusal: HubStubRefusalKind::MalformedBody,
        },
        FaultKind::HubStubDelay {
            port: HubStubPort::Jwks,
            delay_ms: 0,
        },
        FaultKind::HubStubRefusal {
            port: HubStubPort::Jwks,
            refusal: HubStubRefusalKind::Status(99),
        },
    ];
    for k in bad {
        assert_eq!(
            inj.apply(&mut tl, k).unwrap_err(),
            HubStubFaultError::InvalidParam
        );
    }
    assert_eq!(inj.active_count(), 0);
    assert_eq!(
        tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
            .unwrap_err(),
        FaultRecordError::NoInjectionInFaultRun
    );
}

#[test]
fn state_errors_do_not_record() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let k = delay(HubStubPort::Jwks, 5);
    assert_eq!(
        inj.release(&mut tl, k).unwrap_err(),
        HubStubFaultError::NotActive
    );
    inj.apply(&mut tl, k).unwrap();
    assert_eq!(
        inj.apply(&mut tl, k).unwrap_err(),
        HubStubFaultError::AlreadyActive
    );
    assert_eq!(
        inj.release(&mut tl, delay(HubStubPort::Jwks, 6))
            .unwrap_err(),
        HubStubFaultError::NotActive
    );
    inj.release(&mut tl, k).unwrap();
    assert_eq!(finish(tl).events().len(), 2);
}

#[test]
fn record_lines_and_no_token_leak() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut lo = logout_rx();
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    let kl = refusal(HubStubPort::Logout, HubStubRefusalKind::Unreachable);
    inj.apply(&mut tl, k).unwrap();
    inj.apply(&mut tl, kl).unwrap();
    let c = canary();
    let _ = inj.receive_logout(&mut lo, &c, NOW);
    let text = finish(tl).to_string();
    assert!(text.contains("hub_stub_refusal target=hub_stub"));
    assert!(text.contains("port=jwks refusal=status_503"));
    assert!(text.contains("A-2026-10-28-9"));
    assert!(!text.contains(&c));
    assert!(!format!("{inj:?}").contains(&c));
    assert!(!format!("{:?}", inj.call_log()).contains(&c));
    for e in [
        HubStubFaultError::InvalidParam,
        HubStubFaultError::AlreadyActive,
        HubStubFaultError::NotActive,
    ] {
        assert!(!e.to_string().contains(&c));
    }
}

/// ハーネスで起動した stub のセッションの口へ注入できる。
#[test]
fn injects_into_harness_started_stub() {
    let comp = HubStubComponent::new(config());
    let handle = comp.handle();
    let _running = Harness::start(HarnessConfig::new().with(Box::new(comp))).unwrap();
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    inj.apply(
        &mut tl,
        refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503)),
    )
    .unwrap();
    let r = handle.with(|s| inj.serve_jwks(&mut s.jwks, NOW)).unwrap();
    assert_eq!(r.unwrap_err(), InjectedRefusal::Status(503));
}

/// 注入器は 1 つの実行に結ぶ。別の実行の記録には使えず、finish は注入器を消費して規則を持ち越さない。
#[test]
fn injector_is_bound_to_a_single_run() {
    let mut tl1 = timeline();
    let (mut inj, _) = injector(&tl1);
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    inj.apply(&mut tl1, k).unwrap();
    // 別の実行（基準線を含む）の記録では、開始も解除も終了もできず、状態も記録も変わらない。
    let mut tl2 = timeline();
    let mut base = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    for other in [&mut tl2, &mut base] {
        assert_eq!(
            inj.apply(other, delay(HubStubPort::Jwks, 3)).unwrap_err(),
            HubStubFaultError::RunMismatch
        );
        assert_eq!(
            inj.release(other, k).unwrap_err(),
            HubStubFaultError::RunMismatch
        );
    }
    assert_eq!(inj.active_count(), 1);
    let (inj2, _) = injector(&tl2);
    assert_eq!(
        inj2.finish(base, UtcTimestamp::from_unix_seconds(1_700_000_060))
            .unwrap_err(),
        HubStubFaultError::RunMismatch
    );
    // 解除せずに終える。注入器は消費され、解除しなかった分は記録に数えられる。
    let rec = inj
        .finish(tl1, UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap();
    assert_eq!(rec.unreleased_count(), 1);
    // 基準線には新しい注入器を作る。以前の規則は効かない。
    let base = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    let (mut fresh, _) = injector(&base);
    let mut jwks = HubJwksEndpoint::new(&config());
    assert_eq!(fresh.serve_jwks(&mut jwks, NOW).unwrap().status, 200);
    assert_eq!(injected_flags(fresh.call_log()), [false]);
}

/// 同じ口へ同じ障害を二度適用・解除しても、事象の呼び出し位置でどの期間の呼び出しかを照合できる。
#[test]
fn event_log_positions_separate_periods_of_the_same_fault() {
    let mut tl = timeline();
    let (mut inj, _) = injector(&tl);
    let mut jwks = HubJwksEndpoint::new(&config());
    let k = refusal(HubStubPort::Jwks, HubStubRefusalKind::Status(503));
    inj.apply(&mut tl, k).unwrap();
    assert!(inj.serve_jwks(&mut jwks, NOW).is_err());
    inj.release(&mut tl, k).unwrap();
    assert!(inj.serve_jwks(&mut jwks, NOW + 1).is_ok());
    inj.apply(&mut tl, k).unwrap();
    assert!(inj.serve_jwks(&mut jwks, NOW + 2).is_err());
    inj.release(&mut tl, k).unwrap();
    let flags = injected_flags(inj.call_log());
    let rec = inj
        .finish(tl, UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap();
    let pos: Vec<Option<usize>> = rec.events().iter().map(|e| e.log_pos).collect();
    assert_eq!(pos, [Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(flags, [true, false, true]);
    // 期間 [applied, released) の呼び出しは、すべて注入あり、期間の外はすべて注入なし。
    let ev = rec.events();
    for (i, injected) in flags.iter().enumerate() {
        let inside = ev
            .chunks(2)
            .any(|w| w[0].log_pos.unwrap() <= i && i < w[1].log_pos.unwrap());
        assert_eq!(inside, *injected);
    }
    assert!(rec.to_string().contains("log_pos=3"));
}
