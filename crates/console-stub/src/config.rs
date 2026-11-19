//! console の stub が受ける設定の型（TASK-8、A-2026-10-28-14）。
//!
//! 呼び出し元は stub を組み立てる試験側。値はすべて設定で渡す試験用の仮の値で、本番の値ではない。
//! 既定値を持たせず、必須の値を落とすとコンパイルできない形にする。値の範囲の検査はここでは持たない。
//! 秘密値は持たない。鍵などを足す段では `fandhe-auth-secret` の型を使い、許可表を見直す。

use std::time::Duration;

/// console の値を受ける設定。他所有者（Hub 等）の値は写さず、ID で引く。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleStubConfig {
    /// 試験対象の `fandhe-auth` の `iss`。`aud` の形が `iss` に依る（OCL-58）。試験用の仮の値。
    pub issuer: String,
    /// console のクライアントの `aud`（OCL-58・D-2026-11-05-5）。受けるだけで、組み立て・登録はしない。
    pub audience: String,
    /// console のクライアントの `client_id`（ADLG-15・D-2026-11-05-5）。実値は未登録のため試験用の仮の値。
    pub client_id: String,
    /// introspection の口の URL（AGCR-32・OPRV-54）。文字列で受けるだけで取得しない。
    pub introspection_endpoint: String,
    /// 有効性の照会の口の URL（ADLG-29）。口の形の正本は Hub（Hub ARUN-60）なので、パスは写さず受ける。
    pub validity_endpoint: String,
    /// 進行中の配送の窓の上限（ADLG-26・TASK-451・A-2026-10-28-14）。console の数値で、stub の値。
    pub delivery_window_max: Duration,
}

// TODO(TASK-8): 仮定の契約の印（#77）を足す。
// TODO(TASK-8): 秘密値（private_key_jwt の鍵等）を足す段で fandhe-auth-secret の型を使い、許可表を見直す。
