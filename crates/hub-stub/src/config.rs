//! Hub の stub が受ける設定の型（TASK-7、A-2026-10-28-1）。
//!
//! 呼び出し元は stub を組み立てる試験側。値はすべて設定で渡す試験用の仮の値で、本番の値ではない。
//! 既定値を持たせず、必須の値を落とすとコンパイルできない形にする。値の範囲の検査は本体側の
//! 設定の検証（OCL-31・SIGN-47）の担当で、ここでは持たない。
//! 秘密値は持たない。秘密値を足す段（#61）では `fandhe-auth-secret` の型を使い、許可表を見直す。

use std::time::Duration;

/// Hub の値を受ける設定。数値の正本は Hub の ID で引き、ここには写さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubStubConfig {
    /// Hub の公開ホスト（A-2026-10-28-1、Hub TASK-527）。
    pub public_host: String,
    /// `jwks_uri` の URL（OCL-31・TASK-310）。文字列で受けるだけで取得しない。
    pub jwks_uri: String,
    /// Logout Token の受け口の URL（OCL-42）。
    pub logout_receiver_uri: String,
    /// JWKS の `max-age`（Hub D-2026-10-20-2・Hub D-2026-10-22-1・SIGN-47）。
    pub jwks_max_age: Duration,
    /// JWKS の取得の間隔の下限（Hub D-2026-10-20-2・SIGN-47）。
    pub jwks_fetch_min_interval: Duration,
    /// キャッシュの上限 C_max（Hub D-2026-10-22-1・OCL-37）。
    pub jwks_cache_max: Duration,
    /// assertion の有効期限の上限 M（OCL-25・Hub AUTHC-39）。
    pub assertion_max_lifetime: Duration,
}

// TODO(TASK-7): 仮置きの値の明示の仕組み（#67）で、各値に仮置きかどうかの印を足す。
