//! console の stub が受ける設定の型（TASK-8、A-2026-10-28-14）。
//!
//! 呼び出し元は stub を組み立てる試験側。値はすべて設定で渡す試験用の仮の値で、本番の値ではない。
//! 既定値を持たせず、必須の値を落とすとコンパイルできない形にする。値の範囲の検査はここでは持たない。
//! 秘密値は持たない。鍵などを足す段では `fandhe-auth-secret` の型を使い、許可表を見直す。

use std::time::Duration;

use crate::credential::OAuthErrorCode;

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

/// エージェントの資格情報の発行・失効の呼び出しを模す側が受ける設定（#75）。
/// 全項目必須・既定値なし・秘密値なし。口の URL は取得せず、記述として口へ渡すだけ。
/// 経路名は最終でなく（D-2026-10-17-84）、登録の失効の口は Hub が正本（Hub ARUN-58）のため、パスは写さず受ける。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCredentialConfig {
    /// エージェントの公開クライアントの `client_id`（AGCR-1・AGCR-2）。試験用の仮の値。
    pub client_id: String,
    /// Device Authorization の開始の口の URL（DGRT-1）。
    pub device_authorization_endpoint: String,
    /// `device_code` grant の交換の口の URL（DGRT-7・DGRT-8）。
    pub token_endpoint: String,
    /// RFC 7009 の失効の口の URL（AGCR-43・OPRV-53）。
    pub revocation_endpoint: String,
    /// エージェントの登録の失効の口の URL（AGCR-35・AGCR-36。受付の正本は Hub ARUN-58）。
    pub registration_revocation_endpoint: String,
}

/// 口ごとの呼び出し回数の範囲（1 始まり、両端を含む）。範囲の検査はしない。
/// `first == 0` は 1 と同じに振る舞う。`last: None` は以降すべて。回数は口ごとの通算で、設定を切り替えても戻らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallWindow {
    pub first: u32,
    pub last: Option<u32>,
}

impl CallWindow {
    /// `n` 回目（1 始まり）の呼び出しが範囲に入るか。
    pub fn contains(&self, n: u32) -> bool {
        n >= self.first.max(1) && self.last.is_none_or(|l| n <= l)
    }
}

/// 注入する 1 件の規則。`delay` は呼び出し元の待ちのフック（[`crate::fault::Wait`]）へ渡す値で、stub は待たない。
/// 遅れの値は試験用の仮の値で、要件の値ではない。stub での測定を数値の根拠にしない（TASK-30・THRT-29・A-2026-10-28-14）。
/// 注入で起こせるのは失敗の向き（遅れ・送れない・拒否）だけで、成功を作る型は持たない（AVLB-18・AVLB-22）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultRule<R> {
    pub calls: CallWindow,
    pub delay: Option<Duration>,
    pub refusal: Option<R>,
}

/// introspection の口の拒否。`Status` は本文を空にして返す（401・503 はそれぞれ既存の失敗に写る。
/// `Status(200)` は本文が空のため `MalformedResponse` になり、成功にならない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrospectionRefusal {
    Unreachable,
    Status(u16),
}

/// 有効性の照会（②③）の口の拒否。口は有効か無効かしか返せない抽象の型（Hub が正本）のため、送れないだけを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidityRefusal {
    Unreachable,
}

/// 資格情報の口の拒否。失効の口へは `error: Some(..)` に写る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialRefusal {
    Unreachable,
    Rejected { status: u16, error: OAuthErrorCode },
}

/// 配送の時点の確認の口（①②③）へ注入する規則。複数の規則が当たるときは Vec の先頭から見て最初の 1 件だけを使う。
/// 既定値は持たない。注入なしの基準線は [`DeliveryFaultConfig::baseline`] で明示する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryFaultConfig {
    pub introspection: Vec<FaultRule<IntrospectionRefusal>>,
    pub decider: Vec<FaultRule<ValidityRefusal>>,
    pub hub: Vec<FaultRule<ValidityRefusal>>,
}

impl DeliveryFaultConfig {
    /// 注入なし（基準線）。同じラッパを通して流すための空の設定。
    pub fn baseline() -> Self {
        Self {
            introspection: Vec::new(),
            decider: Vec::new(),
            hub: Vec::new(),
        }
    }
}

/// 資格情報の口（発行・失効）へ注入する規則。規則の選び方は [`DeliveryFaultConfig`] と同じ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFaultConfig {
    pub device_authorization: Vec<FaultRule<CredentialRefusal>>,
    pub token: Vec<FaultRule<CredentialRefusal>>,
    pub revoke_token: Vec<FaultRule<CredentialRefusal>>,
    pub revoke_registration: Vec<FaultRule<CredentialRefusal>>,
}

impl CredentialFaultConfig {
    /// 注入なし（基準線）。
    pub fn baseline() -> Self {
        Self {
            device_authorization: Vec::new(),
            token: Vec::new(),
            revoke_token: Vec::new(),
            revoke_registration: Vec::new(),
        }
    }
}

// 契約の仮定の印と記録は [`crate::contract`]（#77・A-2026-10-28-12）。本 module の値の仮置きは A-2026-10-28-14。
// TODO(TASK-511): 秘密値（private_key_jwt の鍵等）を足す段で fandhe-auth-secret の型を使い、許可表を見直す。
