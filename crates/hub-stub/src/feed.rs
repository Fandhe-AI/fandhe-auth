//! Hub が `fandhe-auth` の変更のフィードを取得する側の模擬（TASK-7・#63）。試験専用で、実物の Hub ではない。
//!
//! 役割: 実物の Hub は push（`logout_token`）と pull（変更のフィード）を併用する（Hub D-2026-10-16-7、
//! RVPG-28・RVPG-31・RVPG-38）。呼び出し元は試験（と後の統合試験）で、`private_key_jwt` の assertion で
//! フィードの口を呼び、返った項目を取り込む動きを駆動する（OCL-21・OCL-30・RVPG-39・RVPG-40）。
//! 呼び出し先の口は [`ChangeFeedPort`] で表す。実物の口（`backend`）への配線は後続のタスクで行う。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻 `now`（UNIX 秒）と assertion の `jti` は引数で受け、時計・環境変数・ファイルは
//!   読まない（DPOL-32）。署名と要求の組み立ては [`HubAssertionCaller`] を借り、自前の暗号は書かない（DPOL-47）。
//! - 取得間隔の下限は設定で受け（[`HubStubConfig::feed_fetch_min_interval`]）、下限の前の取得は口へ届かせない。
//! - 口が拒否した取得は成功として数えず、項目もカーソルも変えない（RVPG-40 の受け側）。
//! - 応答の 1 件でも形が誤っていれば全体を拒否し、部分的に取り込まない。
//! - 項目・カーソル・assertion の文字列は誤り・記録・`Debug` の文言に出さない（SECR-23〜27）。
//!
//! 判定しないもの: 項目の形は `fandhe-auth` が正本を持ち検討中（RVPG-38・RVPG-41、Hub AUTHC-35）。
//! 種別と版による適用の順序（RVPG-22・RVPG-23）は Hub 側の担当で、ここでは判定しない。
//!
//! 書き手の読み（spec に定めが無い。実物との差は #69・#70 で文書にする）:
//! - 経過がちょうど下限なら取得してよい。`now < last_attempt_at`（時計の巻き戻り）は安全側に倒して拒む。
//!   `now` は秒単位なので、1 秒未満の下限は切り上げて扱う。
//! - 口に届いた要求は、拒否されても間隔の計数に入れる。
//! - 項目が `sid` も [`GROUPED_CLAIM`] も持たなければ、本人単位の項目（RVPG-41）として中身を保持するだけにする。

use std::collections::HashSet;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::assertion::{AssertionRequest, CalledEndpoint, ClientAssertionType, HubAssertionCaller};
use crate::config::HubStubConfig;
use crate::logout::{GROUPED_CLAIM, LogoutTokenReceiver};

/// カーソルを運ぶ form の名前。spec に定めが無い仮置き。
/// TODO(TASK-7): 本体の `ChangeFeed::read_since`（仮称。RVPG-42）の形が決まったら置き換える（#67・#69・#70）。
const CURSOR_PARAM: &str = "cursor";

/// 口が返した結果。項目は検討中の形のまま JSON で受ける（RVPG-38・RVPG-41）。
#[derive(Clone, PartialEq)]
pub enum FeedResponse {
    Items {
        items: Vec<Value>,
        next_cursor: Option<String>,
    },
    Rejected {
        status: u16,
    },
}

impl std::fmt::Debug for FeedResponse {
    /// 項目・カーソルの値は出さず、件数と有無だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Items { items, next_cursor } => f
                .debug_struct("Items")
                .field("items", &items.len())
                .field("next_cursor", &next_cursor.as_ref().map(|_| "<redacted>"))
                .finish(),
            Self::Rejected { status } => {
                f.debug_struct("Rejected").field("status", status).finish()
            }
        }
    }
}

/// `fandhe-auth` のフィードの口を表す注入点。本段では試験側の偽の口が実装する。
pub trait ChangeFeedPort {
    fn fetch(&mut self, request: &AssertionRequest) -> FeedResponse;
}

/// 項目の種別。
#[derive(Clone, PartialEq, Eq)]
pub enum FeedEntryKind {
    /// 認証セッションの失効。`sid` か組の一覧のどちらか一方（RVPG-38）。
    Session {
        sid: Option<String>,
        pairs: Vec<(String, String)>,
    },
    /// 本人単位の項目（RVPG-41）。中身は `claims` に保持するだけ。
    Principal,
}

impl std::fmt::Debug for FeedEntryKind {
    /// `sid`・組の値は出さない（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session { sid, pairs } => f
                .debug_struct("Session")
                .field("sid", &sid.as_ref().map(|_| "<redacted>"))
                .field("pairs", &pairs.len())
                .finish(),
            Self::Principal => f.write_str("Principal"),
        }
    }
}

/// 取り込んだ項目。
#[derive(Clone, PartialEq)]
pub struct FeedEntry {
    pub jti: Option<String>,
    pub kind: FeedEntryKind,
    pub claims: Map<String, Value>,
    /// 同じ `jti` を既に取り込んでいた再送。適用には数えない（RVPG-28・RVPG-42）。
    pub duplicate: bool,
}

impl std::fmt::Debug for FeedEntry {
    /// `jti`・クレームの値は出さない（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedEntry")
            .field("jti", &self.jti.as_ref().map(|_| "<redacted>"))
            .field("kind", &self.kind)
            .field("claims", &"<redacted>")
            .field("duplicate", &self.duplicate)
            .finish()
    }
}

/// 取得の結果の記録。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    TooSoon,
    Rejected {
        status: u16,
    },
    /// assertion の署名に失敗した。口は呼ばれず、間隔の計数にも入らない。
    SigningFailed,
    /// 応答の形が誤っていた。口は呼ばれており、間隔の計数に入る。
    Malformed,
    Fetched {
        items: usize,
        new_entries: usize,
    },
}

/// 取得の記録。assertion の文字列は持たない（SECR-23〜27）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRecord {
    pub at: u64,
    pub assertion_jti: String,
    pub outcome: FetchOutcome,
}

/// 取得に成功したときの要約。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchSummary {
    pub items: usize,
    pub new_entries: usize,
}

/// 取得の誤り。値を載せない（SECR-23〜27）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    TooSoon,
    Rejected { status: u16 },
    Signing,
    Malformed,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSoon => f.write_str("feed fetch is earlier than the minimum interval"),
            Self::Rejected { status } => write!(f, "feed port rejected the fetch: status {status}"),
            Self::Signing => f.write_str("assertion signing failed"),
            Self::Malformed => f.write_str("feed response is malformed"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Hub がフィードを取得して項目を取り込む模擬の入口。
pub struct ChangeFeedFetcher {
    min_interval: Duration,
    cursor: Option<String>,
    last_attempt_at: Option<u64>,
    entries: Vec<FeedEntry>,
    /// 取り込み済みの `jti`（重複判定用）。
    seen_jtis: HashSet<String>,
    records: Vec<FetchRecord>,
}

impl std::fmt::Debug for ChangeFeedFetcher {
    /// カーソル・項目の値は出さず、件数だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeFeedFetcher")
            .field("min_interval", &self.min_interval)
            .field("cursor", &self.cursor.as_ref().map(|_| "<redacted>"))
            .field("last_attempt_at", &self.last_attempt_at)
            .field("entries", &self.entries.len())
            .field("records", &self.records.len())
            .finish()
    }
}

impl ChangeFeedFetcher {
    pub fn new(config: &HubStubConfig) -> Self {
        Self {
            min_interval: config.feed_fetch_min_interval,
            cursor: None,
            last_attempt_at: None,
            entries: Vec::new(),
            seen_jtis: HashSet::new(),
            records: Vec::new(),
        }
    }

    /// 取り込んだ項目（受理順。再送も印付きで残る）。
    pub fn entries(&self) -> &[FeedEntry] {
        &self.entries
    }

    /// 適用として数える項目（重複でないもの）。
    pub fn applied(&self) -> impl Iterator<Item = &FeedEntry> {
        self.entries.iter().filter(|e| !e.duplicate)
    }

    /// 適用した項目の `jti`（受理順。`jti` の無い項目は含まない）。
    pub fn applied_jtis(&self) -> Vec<&str> {
        self.applied().filter_map(|e| e.jti.as_deref()).collect()
    }

    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub fn records(&self) -> &[FetchRecord] {
        &self.records
    }

    /// 口を最後に呼んだ時刻。
    pub fn last_attempt_at(&self) -> Option<u64> {
        self.last_attempt_at
    }

    /// フィードを 1 回取得する。下限の前なら口を呼ばずに `TooSoon`。
    pub fn fetch<P: ChangeFeedPort>(
        &mut self,
        caller: &HubAssertionCaller,
        port: &mut P,
        assertion_jti: &str,
        now: u64,
    ) -> Result<FetchSummary, FetchError> {
        let record = |outcome| FetchRecord {
            at: now,
            assertion_jti: assertion_jti.to_owned(),
            outcome,
        };
        if let Some(last) = self.last_attempt_at {
            let too_soon = now < last || Duration::from_secs(now - last) < self.min_interval;
            if too_soon {
                self.records.push(record(FetchOutcome::TooSoon));
                return Err(FetchError::TooSoon);
            }
        }
        let claims = caller.conforming_claims(assertion_jti, now);
        let assertion = match caller.sign(&claims) {
            Ok(a) => a,
            Err(_) => {
                self.records.push(record(FetchOutcome::SigningFailed));
                return Err(FetchError::Signing);
            }
        };
        let extra = match &self.cursor {
            Some(c) => vec![(CURSOR_PARAM.to_owned(), c.clone())],
            None => Vec::new(),
        };
        let request = caller.request(
            CalledEndpoint::ChangeFeed,
            &assertion,
            ClientAssertionType::JwtBearer,
            true,
            extra,
        );
        self.last_attempt_at = Some(now);
        match port.fetch(&request) {
            FeedResponse::Rejected { status } => {
                self.records.push(record(FetchOutcome::Rejected { status }));
                Err(FetchError::Rejected { status })
            }
            FeedResponse::Items { items, next_cursor } => {
                let mut parsed = Vec::with_capacity(items.len());
                for item in &items {
                    match parse_entry(item) {
                        Some(e) => parsed.push(e),
                        None => {
                            self.records.push(record(FetchOutcome::Malformed));
                            return Err(FetchError::Malformed);
                        }
                    }
                }
                let total = parsed.len();
                let mut new_entries = 0;
                for mut e in parsed {
                    e.duplicate = e
                        .jti
                        .as_ref()
                        .is_some_and(|j| !self.seen_jtis.insert(j.clone()));
                    if !e.duplicate {
                        new_entries += 1;
                    }
                    self.entries.push(e);
                }
                if next_cursor.is_some() {
                    self.cursor = next_cursor;
                }
                self.records.push(record(FetchOutcome::Fetched {
                    items: total,
                    new_entries,
                }));
                Ok(FetchSummary {
                    items: total,
                    new_entries,
                })
            }
        }
    }
}

/// 項目を最小限だけ解釈する。形が誤っていれば `None`（RVPG-38・RVPG-41）。
fn parse_entry(item: &Value) -> Option<FeedEntry> {
    let claims = item.as_object()?;
    let jti = match claims.get("jti") {
        None => None,
        Some(Value::String(j)) => Some(j.clone()),
        Some(_) => return None,
    };
    let kind = match (claims.get("sid"), claims.get(GROUPED_CLAIM)) {
        (Some(_), Some(_)) => return None,
        (Some(Value::String(sid)), None) => FeedEntryKind::Session {
            sid: Some(sid.clone()),
            pairs: Vec::new(),
        },
        (None, Some(Value::Array(list))) if !list.is_empty() => {
            let mut pairs = Vec::new();
            for p in list {
                let obj = p.as_object()?;
                let get = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_owned);
                match (obj.len(), get("client_id"), get("sid")) {
                    (2, Some(c), Some(s)) => pairs.push((c, s)),
                    _ => return None,
                }
            }
            FeedEntryKind::Session { sid: None, pairs }
        }
        (None, None) => FeedEntryKind::Principal,
        _ => return None,
    };
    // 認証セッションの失効の項目は logout_token と同じクレームの集合で、jti を必ず持つ（RVPG-38）。
    if matches!(kind, FeedEntryKind::Session { .. }) && jti.is_none() {
        return None;
    }
    Some(FeedEntry {
        jti,
        kind,
        claims: claims.clone(),
        duplicate: false,
    })
}

/// push（`logout_token`）とフィードの両方から届いた失効を `jti` で 1 回と数えた一覧（受理順。RVPG-38）。
pub fn applied_jtis_across(
    receiver: &LogoutTokenReceiver,
    fetcher: &ChangeFeedFetcher,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let all = receiver
        .applied()
        .map(|r| r.jti.as_str())
        .chain(fetcher.applied_jtis());
    for j in all {
        if !out.iter().any(|o| o == j) {
            out.push(j.to_owned());
        }
    }
    out
}
