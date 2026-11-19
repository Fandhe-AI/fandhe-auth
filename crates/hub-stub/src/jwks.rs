//! JWKS の取得の模擬（TASK-7・#65）。試験専用で、実物の Hub ではない。
//!
//! 役割: Hub と `fandhe-auth` の間の JWKS の流れは 2 方向ある（書き手の読み。人の確認を待つ）。
//! - [`HubJwksEndpoint`]: `fandhe-auth` が Hub の JWKS（assertion の検証用の鍵）を取得する側の相手。
//!   Hub の JWKS を返し、前段の中間キャッシュの上限（C_max）を模す（OCL-31〜37・Hub D-2026-10-22-1）。
//! - [`AuthJwksFetcher`]: Hub が `fandhe-auth` の JWKS を取得して鍵の集合を保つ側（SIGN-39・SIGN-45・SIGN-47、
//!   Hub D-2026-10-20-2・Hub D-2026-10-16-17）。取得の下限は `jwks_fetch_min_interval`、保持の上限 U は
//!   `jwks_stale_max` で受け、U を超えたら鍵を返さない（fail-closed）。呼び出し先は [`AuthJwksPort`]。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻 `now`（UNIX 秒）は引数で受け、時計・環境変数・ファイルは読まない（DPOL-32）。
//!   `jwks_uri` は参照せず、取得もしない。
//! - 出す `alg` は ES256 に固定し、設定や入力から導かない（SIGN-1・SIGN-6）。JWKS に出すのは公開の欄だけで、
//!   秘密鍵の欄（`d`）は型として持たない。点は長さと接頭辞を確かめてから x・y に分ける。
//! - 数値は設定で受ける（[`HubStubConfig`]）。本番の値を置かない（A-2026-10-28-1）。
//! - 本文は文字列で持つ。壊れた本文などの注入は #66 で足す。
//!
//! 書き手の読み（spec に定めが無い。実物との差は #69・#70 で文書にする）:
//! - 経過がちょうど C_max ならまだ保持する。1 秒未満の C_max は秒へ切り上げる。
//! - 時計の巻き戻り（`now < stored_at`）では古い本文を返さず origin を取り直す。
//! - `max-age` は整数の秒で、1 秒未満の端数は切り捨てる。
//! - 同じ `kid` を重ねて載せられる（OCL-35 の作り分け用）。

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use crate::assertion::AssertionSigningKey;
use crate::config::HubStubConfig;
use crate::presentation::VerificationKey;

/// 公開鍵の点が非圧縮の P-256（0x04 ‖ x ‖ y の 65 バイト）でなかった。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidKey;

impl std::fmt::Display for InvalidKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid ES256 public key point")
    }
}

impl std::error::Error for InvalidKey {}

/// ES256 の公開鍵 1 本を JWK にする。`alg` は ES256 に固定する（SIGN-1・SIGN-6）。
fn es256_jwk(kid: &str, point: &[u8]) -> Result<Value, InvalidKey> {
    if point.len() != 65 || point[0] != 0x04 {
        return Err(InvalidKey);
    }
    Ok(json!({
        "kty": "EC",
        "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
        "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
        "kid": kid,
        "alg": "ES256",
        "use": "sig",
    }))
}

/// 秒へ切り上げる（`now` が秒単位のため）。
fn ceil_secs(d: Duration) -> u64 {
    d.as_secs().saturating_add(u64::from(d.subsec_nanos() > 0))
}

/// JWKS の応答 1 回分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwksResponse {
    pub status: u16,
    /// `{"keys":[…]}` の JSON の文字列。
    pub body: String,
    /// `Cache-Control` の `max-age` の秒（端数は切り捨て）。
    pub max_age: u64,
    /// 本文がキャッシュに入ってからの経過（秒）。origin から取り直したときは 0。
    pub age: u64,
    /// 本文の元になった origin の版。
    pub version: u64,
    pub from_cache: bool,
}

impl JwksResponse {
    /// `Cache-Control` ヘッダの値。
    pub fn cache_control(&self) -> String {
        format!("max-age={}", self.max_age)
    }
}

/// 応答の記録。本文は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JwksServeRecord {
    pub at: u64,
    pub version: u64,
    pub from_cache: bool,
    pub age: u64,
}

struct CacheEntry {
    version: u64,
    body: String,
    stored_at: u64,
}

/// Hub の JWKS を返す模擬。origin（載せている鍵）と前段の中間キャッシュ（C_max）を持つ。
pub struct HubJwksEndpoint {
    max_age: Duration,
    cache_max: Duration,
    keys: Vec<Value>,
    version: u64,
    cache: Option<CacheEntry>,
    records: Vec<JwksServeRecord>,
}

impl std::fmt::Debug for HubJwksEndpoint {
    /// 鍵の数・版・記録の件数だけを出す。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubJwksEndpoint")
            .field("keys", &self.keys.len())
            .field("version", &self.version)
            .field("records", &self.records.len())
            .finish()
    }
}

impl HubJwksEndpoint {
    pub fn new(config: &HubStubConfig) -> Self {
        Self {
            max_age: config.jwks_max_age,
            cache_max: config.jwks_cache_max,
            keys: Vec::new(),
            version: 0,
            cache: None,
            records: Vec::new(),
        }
    }

    /// 署名鍵の公開鍵を載せる。新旧の鍵を同時に載せられる（OCL-37）。
    pub fn publish(&mut self, key: &AssertionSigningKey) {
        // ring が返す P-256 の点は 65 バイトの非圧縮形で、検査は通る。
        let ok = self.publish_es256(key.kid(), key.public_key()).is_ok();
        debug_assert!(ok);
    }

    /// 非圧縮の点（65 バイト）を直接載せる。同じ `kid` も重ねて載せられる。
    pub fn publish_es256(&mut self, kid: &str, point: &[u8]) -> Result<(), InvalidKey> {
        let jwk = es256_jwk(kid, point)?;
        self.keys.push(jwk);
        self.version += 1;
        Ok(())
    }

    /// `kid` の鍵をすべて外す。外したものがあれば `true`。
    pub fn retire(&mut self, kid: &str) -> bool {
        let before = self.keys.len();
        self.keys.retain(|k| k["kid"] != kid);
        let removed = self.keys.len() != before;
        if removed {
            self.version += 1;
        }
        removed
    }

    /// origin の現在の版。
    pub fn current_version(&self) -> u64 {
        self.version
    }

    /// 応答の記録（返した順）。
    pub fn records(&self) -> &[JwksServeRecord] {
        &self.records
    }

    fn origin_body(&self) -> String {
        json!({ "keys": self.keys }).to_string()
    }

    /// `now` に JWKS を返す。C_max 以内のキャッシュがあればそれを返し、なければ origin を取り直す。
    /// TODO(TASK-7): 応答の遅れ・拒否・壊れた本文の注入は #66 で足す。
    pub fn serve(&mut self, now: u64) -> JwksResponse {
        let limit = ceil_secs(self.cache_max);
        let max_age = self.max_age.as_secs();
        let hit = self
            .cache
            .as_ref()
            .filter(|c| limit > 0 && now >= c.stored_at && now - c.stored_at <= limit)
            .map(|c| (c.version, c.body.clone(), now - c.stored_at));
        let (version, body, age, from_cache) = match hit {
            Some((v, b, a)) => (v, b, a, true),
            None => {
                let body = self.origin_body();
                self.cache = (limit > 0).then(|| CacheEntry {
                    version: self.version,
                    body: body.clone(),
                    stored_at: now,
                });
                (self.version, body, 0, false)
            }
        };
        self.records.push(JwksServeRecord {
            at: now,
            version,
            from_cache,
            age,
        });
        JwksResponse {
            status: 200,
            body,
            max_age,
            age,
            version,
            from_cache,
        }
    }
}

/// 口が返した JWKS の応答。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthJwksResponse {
    Ok { body: String, max_age: Option<u64> },
    Rejected { status: u16 },
}

/// `fandhe-auth` の JWKS の口を表す注入点。実物の配線は後続のタスクで行い、本段では試験側の偽の口が実装する。
pub trait AuthJwksPort {
    fn fetch_jwks(&mut self) -> AuthJwksResponse;
}

/// 取得の契機。記録にだけ使い、どちらも同じ下限に従う（SIGN-45 の (3)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwksFetchTrigger {
    Periodic,
    UnknownKid,
}

/// 取得の失敗。値・本文は載せない（SECR-23〜27）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwksFetchError {
    TooSoon,
    Rejected { status: u16 },
    Malformed,
    DuplicateKid,
    NoUsableKey,
}

impl std::fmt::Display for JwksFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooSoon => "jwks fetch refused: below minimum interval",
            Self::Rejected { .. } => "jwks fetch rejected by port",
            Self::Malformed => "jwks body malformed",
            Self::DuplicateKid => "jwks body has duplicate kid",
            Self::NoUsableKey => "jwks body has no usable key",
        })
    }
}

impl std::error::Error for JwksFetchError {}

/// 取得に成功した鍵の件数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySetSummary {
    pub keys: usize,
    /// ES256 以外で数えるだけにした件数（受け口は ES256 だけを扱う）。
    pub skipped: usize,
}

/// 取得の結果の記録。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwksFetchOutcome {
    TooSoon,
    Rejected { status: u16 },
    Malformed,
    DuplicateKid,
    NoUsableKey,
    Fetched { keys: usize, skipped: usize },
}

/// 取得の記録。本文は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JwksFetchRecord {
    pub at: u64,
    pub trigger: JwksFetchTrigger,
    pub outcome: JwksFetchOutcome,
}

enum JwkParse {
    Usable(VerificationKey),
    Skipped,
    Invalid,
}

fn parse_es256_jwk(v: &Value) -> JwkParse {
    let Some(o) = v.as_object() else {
        return JwkParse::Invalid;
    };
    match o.get("kty").and_then(Value::as_str) {
        Some("EC") => {}
        Some(_) => return JwkParse::Skipped,
        None => return JwkParse::Invalid,
    }
    // SIGN-1・SIGN-6: alg は無いか ES256 のときだけ受ける。
    if o.get("alg").is_some_and(|a| a.as_str() != Some("ES256")) {
        return JwkParse::Invalid;
    }
    // 用途の制限: use があれば "sig" のみ、key_ops があれば "verify" を含むものだけを検証鍵にする。
    // 署名検証以外の用途（enc・sign のみ等）の鍵は検証用集合から除外する（型が不正なら Invalid）。
    if let Some(u) = o.get("use") {
        match u.as_str() {
            Some("sig") => {}
            Some(_) => return JwkParse::Skipped,
            None => return JwkParse::Invalid,
        }
    }
    if let Some(ops) = o.get("key_ops") {
        let Some(ops) = ops.as_array() else {
            return JwkParse::Invalid;
        };
        if ops.iter().any(|v| !v.is_string()) {
            return JwkParse::Invalid;
        }
        if !ops.iter().any(|v| v.as_str() == Some("verify")) {
            return JwkParse::Skipped;
        }
    }
    let (Some("P-256"), Some(kid)) = (
        o.get("crv").and_then(Value::as_str),
        o.get("kid").and_then(Value::as_str),
    ) else {
        return JwkParse::Invalid;
    };
    let dec = |name: &str| {
        o.get(name)
            .and_then(Value::as_str)
            .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
            .filter(|b| b.len() == 32)
    };
    let (Some(x), Some(y)) = (dec("x"), dec("y")) else {
        return JwkParse::Invalid;
    };
    let mut point = vec![0x04];
    point.extend(x);
    point.extend(y);
    JwkParse::Usable(VerificationKey::es256(kid, point))
}

/// Hub が `fandhe-auth` の JWKS を取得して鍵の集合を保つ側の模擬。
pub struct AuthJwksFetcher {
    min_interval: Duration,
    stale_max: Duration,
    keys: Vec<VerificationKey>,
    last_attempt_at: Option<u64>,
    last_success_at: Option<u64>,
    last_max_age: Option<u64>,
    records: Vec<JwksFetchRecord>,
}

impl std::fmt::Debug for AuthJwksFetcher {
    /// 鍵の数と記録の件数だけを出す。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthJwksFetcher")
            .field("keys", &self.keys.len())
            .field("records", &self.records.len())
            .finish()
    }
}

impl AuthJwksFetcher {
    pub fn new(config: &HubStubConfig) -> Self {
        Self {
            min_interval: config.jwks_fetch_min_interval,
            stale_max: config.jwks_stale_max,
            keys: Vec::new(),
            last_attempt_at: None,
            last_success_at: None,
            last_max_age: None,
            records: Vec::new(),
        }
    }

    fn record(&mut self, at: u64, trigger: JwksFetchTrigger, outcome: JwksFetchOutcome) {
        self.records.push(JwksFetchRecord {
            at,
            trigger,
            outcome,
        });
    }

    /// 口から JWKS を取得する。下限より前は口を呼ばず `TooSoon`。
    /// 検査に落ちた本文は全体を捨て、直前の鍵の集合を保つ（部分的に取り込まない）。
    pub fn fetch<P: AuthJwksPort>(
        &mut self,
        port: &mut P,
        trigger: JwksFetchTrigger,
        now: u64,
    ) -> Result<KeySetSummary, JwksFetchError> {
        if let Some(last) = self.last_attempt_at
            && (now < last || now - last < ceil_secs(self.min_interval))
        {
            self.record(now, trigger, JwksFetchOutcome::TooSoon);
            return Err(JwksFetchError::TooSoon);
        }
        // 口に届いた要求は失敗でも間隔の計数に入れる。
        self.last_attempt_at = Some(now);
        let (body, max_age) = match port.fetch_jwks() {
            AuthJwksResponse::Ok { body, max_age } => (body, max_age),
            AuthJwksResponse::Rejected { status } => {
                self.record(now, trigger, JwksFetchOutcome::Rejected { status });
                return Err(JwksFetchError::Rejected { status });
            }
        };
        match Self::parse_body(&body) {
            Ok((keys, skipped)) => {
                let n = keys.len();
                self.keys = keys;
                self.last_success_at = Some(now);
                self.last_max_age = max_age;
                self.record(now, trigger, JwksFetchOutcome::Fetched { keys: n, skipped });
                Ok(KeySetSummary { keys: n, skipped })
            }
            Err(e) => {
                let outcome = match e {
                    JwksFetchError::DuplicateKid => JwksFetchOutcome::DuplicateKid,
                    JwksFetchError::NoUsableKey => JwksFetchOutcome::NoUsableKey,
                    _ => JwksFetchOutcome::Malformed,
                };
                self.record(now, trigger, outcome);
                Err(e)
            }
        }
    }

    fn parse_body(body: &str) -> Result<(Vec<VerificationKey>, usize), JwksFetchError> {
        let v: Value = serde_json::from_str(body).map_err(|_| JwksFetchError::Malformed)?;
        let arr = v
            .get("keys")
            .and_then(Value::as_array)
            .ok_or(JwksFetchError::Malformed)?;
        let mut keys = Vec::new();
        let mut skipped = 0;
        for e in arr {
            match parse_es256_jwk(e) {
                JwkParse::Usable(k) => keys.push(k),
                JwkParse::Skipped => skipped += 1,
                JwkParse::Invalid => return Err(JwksFetchError::Malformed),
            }
        }
        // OCL-35 と同じ向き: kid の重複は全体を失敗にする。
        let mut seen = std::collections::HashSet::new();
        if !keys.iter().all(|k| seen.insert(k.kid.as_str())) {
            return Err(JwksFetchError::DuplicateKid);
        }
        if keys.is_empty() {
            return Err(JwksFetchError::NoUsableKey);
        }
        Ok((keys, skipped))
    }

    /// `now` で使ってよい鍵の集合。最後の成功から U 以内ならその集合、超過・未取得・時計の巻き戻りは空（fail-closed）。
    pub fn keys(&self, now: u64) -> Vec<VerificationKey> {
        match self.last_success_at {
            Some(t) if now >= t && Duration::from_secs(now - t) <= self.stale_max => {
                self.keys.clone()
            }
            _ => Vec::new(),
        }
    }

    /// 最後に成功した応答の `max-age`。定期の取得の周期は計算せず、試験が組み立てる。
    pub fn last_max_age(&self) -> Option<u64> {
        self.last_max_age
    }

    pub fn last_success_at(&self) -> Option<u64> {
        self.last_success_at
    }

    pub fn last_attempt_at(&self) -> Option<u64> {
        self.last_attempt_at
    }

    /// 取得の記録（試行順）。
    pub fn records(&self) -> &[JwksFetchRecord] {
        &self.records
    }
}
