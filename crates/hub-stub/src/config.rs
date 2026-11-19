//! Hub の stub が受ける設定の型（TASK-7、A-2026-10-28-1）。
//!
//! 呼び出し元は stub を組み立てる試験側。値はすべて設定で渡す試験用の仮の値で、本番の値ではない。
//! 既定値を持たせず、必須の値を落とすとコンパイルできない形にする。値の範囲の検査は本体側の
//! 設定の検証（OCL-31・SIGN-47）の担当で、ここでは持たない。
//! 秘密値（署名鍵）は設定に持たず、`assertion::AssertionSigningKey` で受ける。
//!
//! 仮置きの印（TASK-7、A-2026-10-28-1、A-2026-10-28-9）: 各値は [`StubValue`] に包み、[`ValueMark`] の印を必ず付ける。
//! 印の種類は「PoC の仮置き」と「試験用の仮の値」の 2 つだけで、本番・確定・実物を表す種類は作らない（ACPT-7）。
//! 数値はこの repo に置かず、呼び出し元の試験が渡す。PoC の値を使うときは PoC の ID で引く。
//! 印の記録は [`HubStubConfig::provisional_record`] が返す。sans-IO で、欄の名前と印だけを持ち、値は載せない（DPOL-32）。
//! stub の合格を実物の Hub の合格に数えない（A-2026-10-28-9）。
//! TODO(TASK-511): 秘密値の型の実装後に `fandhe-auth-secret` の型へ置き換え、許可表を見直す（表の変更は利用者に確かめる）。

use std::fmt;
use std::time::Duration;

/// Hub の値を受ける設定。数値の正本は Hub の ID で引き、ここには写さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubStubConfig {
    /// Hub が `fandhe-auth` を呼ぶときの `client_id`。assertion の `iss`・`sub` になる（OCL-24・OCL-42、
    /// Hub D-2026-10-22-2）。値は写さず、試験用の仮の値で受ける（A-2026-10-28-1）。
    pub client_id: StubValue<String>,
    /// Hub の公開ホスト（A-2026-10-28-1、Hub TASK-527）。
    pub public_host: StubValue<String>,
    /// `jwks_uri` の URL（OCL-31・TASK-310）。文字列で受けるだけで取得しない。
    pub jwks_uri: StubValue<String>,
    /// Logout Token の受け口の URL（OCL-42）。
    pub logout_receiver_uri: StubValue<String>,
    /// JWKS の `max-age`（Hub D-2026-10-20-2・Hub D-2026-10-22-1・SIGN-47）。Hub の JWKS の配信（`jwks::HubJwksEndpoint`）で使う。
    pub jwks_max_age: StubValue<Duration>,
    /// JWKS の取得の間隔の下限（Hub D-2026-10-20-2・SIGN-47）。Hub が `fandhe-auth` の JWKS を取得する模擬（`jwks::AuthJwksFetcher`）で使う。
    pub jwks_fetch_min_interval: StubValue<Duration>,
    /// キャッシュの上限 C_max（Hub D-2026-10-22-1・OCL-37）。Hub の JWKS の配信（`jwks::HubJwksEndpoint`）で使い、U には流用しない。
    pub jwks_cache_max: StubValue<Duration>,
    /// Hub が `fandhe-auth` の JWKS を最後に取得できた鍵の集合で使い続ける上限 U（Hub D-2026-10-16-17・
    /// Hub D-2026-10-20-2・SIGN-39 → Hub AOPS-42）。OCL-34 の T（`fandhe-auth` 側の値）とは別。
    /// 欄の名前は仮置き。数値は写さず試験用の仮の値で受ける（A-2026-10-28-1）。
    pub jwks_stale_max: StubValue<Duration>,
    /// assertion の有効期限の上限 M（OCL-25・Hub AUTHC-39）。
    pub assertion_max_lifetime: StubValue<Duration>,
    /// 変更のフィードを読む間隔の下限。フィードを読む周期は Hub が正本を持ち、spec 上は未決（Hub AUTHC-35、
    /// Hub TASK-14 待ち）。JWKS の `jwks_fetch_min_interval`（Hub D-2026-10-20-2）とは別の値で、
    /// 数値は写さず試験用の仮の値で受ける（A-2026-10-28-1）。
    pub feed_fetch_min_interval: StubValue<Duration>,
    /// 生存確認の途絶と見なす閾値。経過がこれを超えたら途絶（Hub AUTHC-21）。閾値の正本は Hub が持つ
    /// （Hub AUTHC-37・Hub D-2026-10-16-11・Hub D-2026-10-20-1・Hub AOPS-84）。数値は写さず、
    /// 試験用の仮の値で受ける（A-2026-10-28-1）。
    pub heartbeat_loss_threshold: StubValue<Duration>,
}

/// 値が本番のものではないことを表す印（A-2026-10-28-1）。
/// 本番・確定・実物の種類は持たない。種類を足すときは試験の網羅の `match` が検出する（ACPT-7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueMark {
    /// PoC のパラメータを仮置きで使った値。`poc` は PoC の ID で、数値は持たない。
    PocProvisional { poc: &'static str },
    /// 試験のために選んだ仮の値（A-2026-10-28-1）。
    TestProvisional,
}

impl ValueMark {
    /// 記録・表示で使う語。どの種類でも「仮置き」。
    pub fn label(&self) -> &'static str {
        match self {
            ValueMark::PocProvisional { .. } | ValueMark::TestProvisional => "仮置き",
        }
    }
}

impl fmt::Display for ValueMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueMark::PocProvisional { poc } => write!(f, "{}（{poc}）", self.label()),
            ValueMark::TestProvisional => write!(f, "{}（試験用）", self.label()),
        }
    }
}

/// 値と必須の印の組。印を省けず、`Deref`・`From`・`Default` を実装しないので、
/// 仮の値が暗黙に本番の値として流れない（A-2026-10-28-1）。
#[derive(Clone, PartialEq, Eq)]
pub struct StubValue<T> {
    value: T,
    mark: ValueMark,
}

impl<T> StubValue<T> {
    /// 試験用の仮の値。
    pub fn test(value: T) -> Self {
        Self {
            value,
            mark: ValueMark::TestProvisional,
        }
    }

    /// PoC の仮置きの値。`poc` は PoC の ID（数値は写さない）。
    pub fn poc(value: T, poc: &'static str) -> Self {
        Self {
            value,
            mark: ValueMark::PocProvisional { poc },
        }
    }

    /// 値の取り出しの唯一の経路。
    pub fn get(&self) -> &T {
        &self.value
    }

    /// 値に付いた印。
    pub fn mark(&self) -> &ValueMark {
        &self.mark
    }
}

impl<T: fmt::Debug> fmt::Debug for StubValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:?})", self.mark, self.value)
    }
}

/// 記録の 1 欄分。欄の名前と印だけで、値は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvisionalEntry {
    pub field: &'static str,
    pub mark: ValueMark,
}

/// 設定の全欄の印の記録（「仮置き」と記録に残す。TASK-7）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionalRecord {
    pub entries: Vec<ProvisionalEntry>,
}

impl fmt::Display for ProvisionalRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Hub の stub（実物ではない）。stub の合格を実物の合格に数えない（A-2026-10-28-9）"
        )?;
        for e in &self.entries {
            writeln!(f, "{}: {}", e.field, e.mark)?;
        }
        Ok(())
    }
}

fn entry<T>(field: &'static str, v: &StubValue<T>) -> ProvisionalEntry {
    ProvisionalEntry {
        field,
        mark: v.mark,
    }
}

impl HubStubConfig {
    /// 全欄の印の記録。網羅の分解で書くので、欄を足して記録を直し忘れるとコンパイルできない。
    pub fn provisional_record(&self) -> ProvisionalRecord {
        let Self {
            client_id,
            public_host,
            jwks_uri,
            logout_receiver_uri,
            jwks_max_age,
            jwks_fetch_min_interval,
            jwks_cache_max,
            jwks_stale_max,
            assertion_max_lifetime,
            feed_fetch_min_interval,
            heartbeat_loss_threshold,
        } = self;
        ProvisionalRecord {
            entries: vec![
                entry("client_id", client_id),
                entry("public_host", public_host),
                entry("jwks_uri", jwks_uri),
                entry("logout_receiver_uri", logout_receiver_uri),
                entry("jwks_max_age", jwks_max_age),
                entry("jwks_fetch_min_interval", jwks_fetch_min_interval),
                entry("jwks_cache_max", jwks_cache_max),
                entry("jwks_stale_max", jwks_stale_max),
                entry("assertion_max_lifetime", assertion_max_lifetime),
                entry("feed_fetch_min_interval", feed_fetch_min_interval),
                entry("heartbeat_loss_threshold", heartbeat_loss_threshold),
            ],
        }
    }
}
