//! Hub が `fandhe-auth` のインスタンスから生存確認を受ける側の模擬（TASK-7・#64）。試験専用で、実物の Hub ではない。
//!
//! 役割: 実物の Hub は生存確認が途絶えたインスタンスを分断と見なす（Hub AUTHC-20・Hub AUTHC-21、
//! Hub D-2026-10-16-11）。生存確認の送出・受信・監視は Hub の AEVT-23〜AEVT-26 へ移り、本体に残るのは
//! 状態の観測の契約（RVPG-62）だけである（A-2026-10-31-3）。呼び出し元は試験で、`receive` で受信を
//! 作り、`status` で「生存」「非生存」の両方を作り出して確かめる。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻 `now`（UNIX 秒）は引数で受け、時計・環境変数・ファイルは読まない（DPOL-32）。
//! - 閾値は設定で受ける（[`HubStubConfig::heartbeat_loss_threshold`]）。数値はここに書かない（A-2026-10-28-1）。
//! - DB の状態・件数は記録するだけで、生存の判定に使わない。DB の停止を非生存にしない（Hub AUTHC-10）。
//! - 識別子は `Debug` に出さず、件数と状態だけを出す（SECR-23〜27）。
//!
//! 判定しないもの: 生存確認の要求の認証（共有値・署名）は、この repo の spec に定めが無く（Hub AEVT-26 側）、
//! ここでは検証しない。偽の生存確認で分断が隠れうる点は実物との差である。HTTP の表現（`x-db` 等）も
//! Hub AEVT-26 の担当で検討中のため、型付きの入力で受ける。送出側の間隔・公表値（Hub AEVT-24）は受けない。
//!
//! 書き手の読み（spec に定めが無い。実物との差は #69・#70 で文書にする）:
//! - 経過がちょうど閾値なら生存、超えたら非生存（Hub AUTHC-21 の「超」の読み）。
//! - 未受信は生存と数えない。`now < 最終受信時刻`（時計の巻き戻り）は安全側に倒して非生存とする。
//!   `now` は秒単位なので、1 秒未満の閾値は切り上げて扱う。
//! - 受信時刻が直前より後退する受信は記録に残すが、最終受信時刻は巻き戻さない。

use std::collections::HashMap;
use std::time::Duration;

use crate::config::HubStubConfig;

/// DB の状態の語彙（RVPG-62 の仮置き）。生存の判定には使わない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DbState {
    Ok,
    Error,
    Timeout,
    Unknown,
}

/// 生存確認 1 回分の入力。HTTP の解析はしない（Hub AEVT-26 の担当で検討中）。
#[derive(Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub instance: String,
    pub db: Option<DbState>,
    /// 未配信の件数。不明は `None` で、0 と区別する（RVPG-62）。
    pub pending: Option<u64>,
}

impl std::fmt::Debug for Heartbeat {
    /// 識別子の値は出さない（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Heartbeat")
            .field("instance", &"<redacted>")
            .field("db", &self.db)
            .field("pending", &self.pending)
            .finish()
    }
}

/// インスタンスの生存の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    /// 閾値を超えて途絶えた（Hub AUTHC-21）。時計の巻き戻りもここに含める。
    Lost,
    /// 一度も受信していない。生存と数えない。
    NeverSeen,
}

/// 受信の記録。識別子は持たない（SECR-23〜27）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatRecord {
    pub at: u64,
    pub db: Option<DbState>,
    pub pending: Option<u64>,
}

/// 生存確認の受け口の模擬。
pub struct HeartbeatReceiver {
    threshold: Duration,
    last_received: HashMap<String, u64>,
    records: Vec<HeartbeatRecord>,
}

impl std::fmt::Debug for HeartbeatReceiver {
    /// 識別子は出さず、件数だけを出す（SECR-23〜27）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeartbeatReceiver")
            .field("threshold", &self.threshold)
            .field("instances", &self.last_received.len())
            .field("records", &self.records.len())
            .finish()
    }
}

impl HeartbeatReceiver {
    pub fn new(config: &HubStubConfig) -> Self {
        Self {
            threshold: config.heartbeat_loss_threshold,
            last_received: HashMap::new(),
            records: Vec::new(),
        }
    }

    /// 生存確認を受ける。最終受信時刻は後退させない。
    pub fn receive(&mut self, heartbeat: &Heartbeat, now: u64) {
        self.records.push(HeartbeatRecord {
            at: now,
            db: heartbeat.db,
            pending: heartbeat.pending,
        });
        let slot = self
            .last_received
            .entry(heartbeat.instance.clone())
            .or_insert(now);
        if now > *slot {
            *slot = now;
        }
    }

    /// 受信の記録（受理順）。
    pub fn records(&self) -> &[HeartbeatRecord] {
        &self.records
    }

    /// `now` 時点でのインスタンスの状態（Hub AUTHC-21）。
    pub fn status(&self, instance: &str, now: u64) -> Liveness {
        let Some(&last) = self.last_received.get(instance) else {
            return Liveness::NeverSeen;
        };
        if now < last {
            return Liveness::Lost;
        }
        // 1 秒未満の閾値は秒へ切り上げる。
        let limit = self
            .threshold
            .as_secs()
            .saturating_add(u64::from(self.threshold.subsec_nanos() > 0));
        if now - last <= limit {
            Liveness::Alive
        } else {
            Liveness::Lost
        }
    }

    /// 追跡中のインスタンスのうち 1 つでも生存していれば真。追跡中が無ければ偽。
    pub fn any_alive(&self, now: u64) -> bool {
        self.last_received
            .keys()
            .any(|i| self.status(i, now) == Liveness::Alive)
    }

    /// 追跡中のインスタンスがあり、すべて生存していれば真。追跡中が無ければ偽（生存と数えない）。
    pub fn all_alive(&self, now: u64) -> bool {
        !self.last_received.is_empty()
            && self
                .last_received
                .keys()
                .all(|i| self.status(i, now) == Liveness::Alive)
    }
}

// TODO(TASK-7): 要求の認証の欠け・HTTP 表現・送出側の値との差は実物との差として #69・#70 で文書にする。
