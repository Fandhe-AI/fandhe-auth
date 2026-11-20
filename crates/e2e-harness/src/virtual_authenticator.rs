//! 仮想 authenticator の options と、WebDriver の WebAuthn 拡張のキーへの写像。
//!
//! 役割: options を 1 か所の写像でドライバへ渡す。`Default` を持たず、値は呼び出す試験が明示で渡す
//! （PoC の値〔QP-1〕を要件の値として置かない。`A-2026-10-28-1`〜`14`）。結果は
//! [`crate::browser::AuthenticatorKind::WebDriverVirtual`]、つまり〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。

use crate::json::Value;

/// authenticator のプロトコル。今使う変種だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// CTAP 2。
    Ctap2,
}

/// authenticator の transport。今使う変種だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// 内蔵（platform）。
    Internal,
}

/// 仮想 authenticator の options。`Default` は無い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualAuthenticatorOptions {
    /// プロトコル。
    pub protocol: Protocol,
    /// transport。
    pub transport: Transport,
    /// resident key を持てるか。
    pub has_resident_key: bool,
    /// user verification に対応するか。
    pub has_user_verification: bool,
    /// user verification を成功として返すか。
    pub is_user_verified: bool,
}

impl VirtualAuthenticatorOptions {
    /// WebDriver の WebAuthn 拡張のキー（`protocol`・`transport`・`hasResidentKey`・`hasUserVerification`・
    /// `isUserVerified`）への写像。ここだけに置く。
    pub fn to_webdriver_json(&self) -> Value {
        Value::object([
            (
                "protocol",
                Value::from(match self.protocol {
                    Protocol::Ctap2 => "ctap2",
                }),
            ),
            (
                "transport",
                Value::from(match self.transport {
                    Transport::Internal => "internal",
                }),
            ),
            ("hasResidentKey", Value::from(self.has_resident_key)),
            (
                "hasUserVerification",
                Value::from(self.has_user_verification),
            ),
            ("isUserVerified", Value::from(self.is_user_verified)),
        ])
    }
}
