//! TASK-7 #63: 変更のフィードの取得の模擬の最小の確認（網羅は #68）。
//! 鍵は実行時に生成し、コミットしない。値は予約 TLD `.test` の仮の値。
//!
//! 偽の口（`FakePort`）は `fandhe-auth` の口の代わりで、本体の検証部の代わりではない。
//! 受けた要求を記録し、決めた応答を返すだけ。

use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fandhe_auth_hub_stub::assertion::{
    AssertionRequest, AssertionSigningKey, AssertionTarget, CalledEndpoint, HubAssertionCaller,
};
use fandhe_auth_hub_stub::config::HubStubConfig;
use fandhe_auth_hub_stub::feed::{
    ChangeFeedFetcher, ChangeFeedPort, FeedEntryKind, FeedResponse, FetchError, FetchOutcome,
    applied_jtis_across,
};
use fandhe_auth_hub_stub::logout::{LogoutExpectations, LogoutTokenReceiver};
use fandhe_auth_hub_stub::presentation::VerificationKey;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

const ISS: &str = "https://auth.example.test";
const CLIENT_ID: &str = "hub-client.test";
const JWT_BEARER: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";
const EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";
const MIN: u64 = 5;
const T0: u64 = 1_000;

fn config() -> HubStubConfig {
    HubStubConfig {
        client_id: CLIENT_ID.to_owned(),
        public_host: "hub.example.test".to_owned(),
        jwks_uri: "https://hub.example.test/jwks".to_owned(),
        logout_receiver_uri: "https://hub.example.test/logout".to_owned(),
        jwks_max_age: Duration::from_secs(1),
        jwks_fetch_min_interval: Duration::from_secs(2),
        jwks_cache_max: Duration::from_secs(3),
        jwks_stale_max: Duration::from_secs(6),
        assertion_max_lifetime: Duration::from_secs(300),
        feed_fetch_min_interval: Duration::from_secs(MIN),
        heartbeat_loss_threshold: Duration::from_secs(10),
    }
}

fn caller() -> HubAssertionCaller {
    let key = AssertionSigningKey::generate("k1").unwrap();
    let target = AssertionTarget {
        issuer: ISS.to_owned(),
    };
    HubAssertionCaller::new(&config(), target, key)
}

/// 受けた要求の記録と、決めた応答を順に返す偽の口。
struct FakePort {
    responses: VecDeque<FeedResponse>,
    received: usize,
    endpoints: Vec<CalledEndpoint>,
    assertion_types: Vec<Option<String>>,
    has_assertion: Vec<bool>,
    cursors: Vec<Option<String>>,
}

impl FakePort {
    fn new(responses: Vec<FeedResponse>) -> Self {
        Self {
            responses: responses.into(),
            received: 0,
            endpoints: Vec::new(),
            assertion_types: Vec::new(),
            has_assertion: Vec::new(),
            cursors: Vec::new(),
        }
    }
}

impl ChangeFeedPort for FakePort {
    fn fetch(&mut self, request: &AssertionRequest) -> FeedResponse {
        self.received += 1;
        self.endpoints.push(request.endpoint());
        self.assertion_types
            .push(request.param("client_assertion_type").map(str::to_owned));
        self.has_assertion
            .push(request.param("client_assertion").is_some());
        self.cursors
            .push(request.param("cursor").map(str::to_owned));
        self.responses.pop_front().expect("unexpected fetch")
    }
}

fn items(items: Vec<Value>, cursor: &str) -> FeedResponse {
    FeedResponse::Items {
        items,
        next_cursor: Some(cursor.to_owned()),
    }
}

fn session_item(jti: &str, sid: &str) -> Value {
    json!({"jti": jti, "sid": sid})
}

/// OCL-21・OCL-30・RVPG-38: 初回の取得は ChangeFeed の口へ assertion を form で渡し、カーソルは持たない。
#[test]
fn first_fetch_calls_change_feed_with_assertion_and_no_cursor() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(vec![session_item("j1", "s1")], "c1")]);
    let got = f.fetch(&c, &mut p, "a1", T0).unwrap();
    assert_eq!((got.items, got.new_entries), (1, 1));
    assert_eq!(p.endpoints, vec![CalledEndpoint::ChangeFeed]);
    assert_eq!(p.assertion_types, vec![Some(JWT_BEARER.to_owned())]);
    assert_eq!(p.has_assertion, vec![true]);
    assert_eq!(p.cursors, vec![None]);
    assert_eq!(f.cursor(), Some("c1"));
    assert_eq!(f.applied_jtis(), vec!["j1"]);
    assert!(matches!(f.entries()[0].kind, FeedEntryKind::Session { .. }));
}

/// RVPG-42: 2 回目の取得は前回のカーソルを送る。
#[test]
fn second_fetch_sends_previous_cursor() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(vec![], "c1"), items(vec![], "c2")]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    f.fetch(&c, &mut p, "a2", T0 + MIN).unwrap();
    assert_eq!(p.cursors, vec![None, Some("c1".to_owned())]);
    assert_eq!(f.cursor(), Some("c2"));
}

/// 受け入れ条件: 下限の前の取得は口へ届かず、記録に残る。
#[test]
fn fetch_before_min_interval_is_too_soon_and_not_sent() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(vec![session_item("j1", "s1")], "c1")]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    let err = f.fetch(&c, &mut p, "a2", T0 + MIN - 1).unwrap_err();
    assert_eq!(err, FetchError::TooSoon);
    assert_eq!(p.received, 1);
    assert_eq!(f.last_attempt_at(), Some(T0));
    assert_eq!(f.cursor(), Some("c1"));
    assert_eq!(f.records().last().unwrap().outcome, FetchOutcome::TooSoon);
}

/// 書き手の読み: 経過がちょうど下限なら取得できる。
#[test]
fn fetch_at_exactly_min_interval_is_allowed_writer_reading() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(vec![], "c1"), items(vec![], "c2")]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    assert!(f.fetch(&c, &mut p, "a2", T0 + MIN).is_ok());
    assert_eq!(p.received, 2);
}

/// 書き手の読み: 時計の巻き戻りは安全側で TooSoon。
#[test]
fn clock_going_backwards_is_too_soon_writer_reading() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(vec![], "c1")]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    assert_eq!(f.fetch(&c, &mut p, "a2", T0 - 1), Err(FetchError::TooSoon));
    assert_eq!(p.received, 1);
}

/// RVPG-40: 口の拒否は成功として数えず、項目もカーソルも変えない。
#[test]
fn rejection_changes_neither_entries_nor_cursor() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![
        items(vec![session_item("j1", "s1")], "c1"),
        FeedResponse::Rejected { status: 401 },
    ]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    let err = f.fetch(&c, &mut p, "a2", T0 + MIN).unwrap_err();
    assert_eq!(err, FetchError::Rejected { status: 401 });
    assert_eq!(f.entries().len(), 1);
    assert_eq!(f.cursor(), Some("c1"));
    assert_eq!(
        f.records().last().unwrap().outcome,
        FetchOutcome::Rejected { status: 401 }
    );
}

/// RVPG-28・RVPG-42: 同じ jti の再取得は印付きで残り、適用には 1 回だけ数える。
#[test]
fn duplicate_jti_is_marked_and_applied_once() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![
        items(vec![session_item("j1", "s1")], "c1"),
        items(
            vec![session_item("j1", "s1"), session_item("j2", "s2")],
            "c2",
        ),
    ]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    let got = f.fetch(&c, &mut p, "a2", T0 + MIN).unwrap();
    assert_eq!((got.items, got.new_entries), (2, 1));
    assert_eq!(f.entries().len(), 3);
    assert!(f.entries()[1].duplicate);
    assert_eq!(f.applied_jtis(), vec!["j1", "j2"]);
}

/// 形の誤った項目を含む応答は全体を拒否し、項目もカーソルも変えない。
#[test]
fn malformed_item_rejects_whole_response() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(
        vec![
            session_item("j1", "s1"),
            json!({"jti": "j2", "sid": "s2", "session_pairs": []}),
        ],
        "c1",
    )]);
    assert_eq!(f.fetch(&c, &mut p, "a1", T0), Err(FetchError::Malformed));
    assert!(f.entries().is_empty());
    assert_eq!(f.cursor(), None);
    assert_eq!(
        f.fetch(
            &c,
            &mut FakePort::new(vec![items(vec![json!("x")], "c")]),
            "a2",
            T0 + MIN
        ),
        Err(FetchError::Malformed)
    );
    // 形の誤りも記録に残る。
    assert_eq!(f.records().last().unwrap().outcome, FetchOutcome::Malformed);
}

/// SECR-11・SECR-13・SECR-23〜27: Debug に項目・カーソルの値が出ない。
#[test]
fn debug_output_does_not_contain_items_or_cursor() {
    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let resp = items(
        vec![session_item("jti-marker-1", "sid-marker-1")],
        "cursor-marker-1",
    );
    let resp_dbg = format!("{resp:?}");
    let mut p = FakePort::new(vec![resp]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    let all = format!("{resp_dbg} {f:?} {:?}", f.entries());
    for marker in ["jti-marker-1", "sid-marker-1", "cursor-marker-1"] {
        assert!(!all.contains(marker), "{marker} leaked");
    }
}

/// RVPG-38: push とフィードの両方で届いた失効を jti で 1 回にまとめる。
#[test]
fn applied_jtis_across_merges_push_and_feed() {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let enc = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    let input = format!(
        "{}.{}",
        enc(&json!({"alg": "ES256", "typ": "logout+jwt", "kid": "k1"})),
        enc(&json!({
            "iss": ISS, "sub": "user-1", "aud": CLIENT_ID, "iat": 900, "exp": 1_020,
            "jti": "j1", "sid": "s1", "events": {EVENT: {}}
        }))
    );
    let sig = key.sign(&rng, input.as_bytes()).unwrap();
    let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()));
    let mut r = LogoutTokenReceiver::new(
        vec![VerificationKey::es256("k1", key.public_key().as_ref())],
        LogoutExpectations {
            issuer: ISS.to_owned(),
            audience: CLIENT_ID.to_owned(),
        },
    );
    r.receive(&token, T0).unwrap();

    let c = caller();
    let mut f = ChangeFeedFetcher::new(&config());
    let mut p = FakePort::new(vec![items(
        vec![session_item("j1", "s1"), session_item("j2", "s2")],
        "c1",
    )]);
    f.fetch(&c, &mut p, "a1", T0).unwrap();
    assert_eq!(applied_jtis_across(&r, &f), vec!["j1", "j2"]);
}
