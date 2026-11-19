//! Hub の stub が受ける設定の型（TASK-7、A-2026-10-28-1）。
//!
//! 呼び出し元は stub を組み立てる試験側。値はすべて設定で渡す試験用の仮の値で、本番の値ではない。
//! 既定値を持たせず、必須の値を落とすとコンパイルできない形にする。値の範囲の検査は本体側の
//! 設定の検証（OCL-31・SIGN-47）の担当で、ここでは持たない。
//! 秘密値（署名鍵）は設定に持たず、`assertion::AssertionSigningKey` で受ける。
//! TODO(TASK-511): 秘密値の型の実装後に `fandhe-auth-secret` の型へ置き換え、許可表を見直す（表の変更は利用者に確かめる）。

use std::time::Duration;

/// Hub の値を受ける設定。数値の正本は Hub の ID で引き、ここには写さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubStubConfig {
    /// Hub が `fandhe-auth` を呼ぶときの `client_id`。assertion の `iss`・`sub` になる（OCL-24・OCL-42、
    /// Hub D-2026-10-22-2）。値は写さず、試験用の仮の値で受ける（A-2026-10-28-1）。
    pub client_id: String,
    /// Hub の公開ホスト（A-2026-10-28-1、Hub TASK-527）。
    pub public_host: String,
    /// `jwks_uri` の URL（OCL-31・TASK-310）。文字列で受けるだけで取得しない。
    pub jwks_uri: String,
    /// Logout Token の受け口の URL（OCL-42）。
    pub logout_receiver_uri: String,
    /// JWKS の `max-age`（Hub D-2026-10-20-2・Hub D-2026-10-22-1・SIGN-47）。Hub の JWKS の配信（`jwks::HubJwksEndpoint`）で使う。
    pub jwks_max_age: Duration,
    /// JWKS の取得の間隔の下限（Hub D-2026-10-20-2・SIGN-47）。Hub が `fandhe-auth` の JWKS を取得する模擬（`jwks::AuthJwksFetcher`）で使う。
    pub jwks_fetch_min_interval: Duration,
    /// キャッシュの上限 C_max（Hub D-2026-10-22-1・OCL-37）。Hub の JWKS の配信（`jwks::HubJwksEndpoint`）で使い、U には流用しない。
    pub jwks_cache_max: Duration,
    /// Hub が `fandhe-auth` の JWKS を最後に取得できた鍵の集合で使い続ける上限 U（Hub D-2026-10-16-17・
    /// Hub D-2026-10-20-2・SIGN-39 → Hub AOPS-42）。OCL-34 の T（`fandhe-auth` 側の値）とは別。
    /// 欄の名前は仮置き。数値は写さず試験用の仮の値で受ける（A-2026-10-28-1）。
    pub jwks_stale_max: Duration,
    /// assertion の有効期限の上限 M（OCL-25・Hub AUTHC-39）。
    pub assertion_max_lifetime: Duration,
    /// 変更のフィードを読む間隔の下限。フィードを読む周期は Hub が正本を持ち、spec 上は未決（Hub AUTHC-35、
    /// Hub TASK-14 待ち）。JWKS の `jwks_fetch_min_interval`（Hub D-2026-10-20-2）とは別の値で、
    /// 数値は写さず試験用の仮の値で受ける（A-2026-10-28-1）。
    pub feed_fetch_min_interval: Duration,
    /// 生存確認の途絶と見なす閾値。経過がこれを超えたら途絶（Hub AUTHC-21）。閾値の正本は Hub が持つ
    /// （Hub AUTHC-37・Hub D-2026-10-16-11・Hub D-2026-10-20-1・Hub AOPS-84）。数値は写さず、
    /// 試験用の仮の値で受ける（A-2026-10-28-1）。
    pub heartbeat_loss_threshold: Duration,
}

// TODO(TASK-7): 仮置きの値の明示の仕組み（#67）で、各値に仮置きかどうかの印を足す。
