//! WebAuthn の呼び出し（登録の `create()`・認証の `get()`）の失敗の名前の列挙。
//!
//! 役割: ブラウザが返す `DOMException.name` のうち既知のものだけを閉じた列挙で持つ。呼び出し元は
//! `registration` と `authentication` で、試験は列挙で失敗の種類を判定する。
//! 未知の値は固定の `Other` に畳み、ブラウザが返した生の文字列は持たない・出さない（SECR-23）。
//! 結果は〔仮想〕の試験の判定で、実機・Safari に数えない（ACPT-7・ACPT-8）。

/// WebAuthn の呼び出しの失敗の `DOMException.name`（既知のものだけ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebAuthnErrorName {
    NotAllowed,
    InvalidState,
    Security,
    NotSupported,
    Abort,
    Constraint,
    Data,
    Encoding,
    Network,
    Timeout,
    Unknown,
    Type,
    /// 呼び出しの前の確認で、ページの origin が試験用の origin と違った（ハーネス自身の固定の名前）。
    OriginMismatch,
    /// ハーネス自身の期限（ページ内の `AbortController`）が先に切れた。ブラウザの `NotAllowed` とは別（ハーネス自身の固定の名前）。
    HarnessTimeout,
    /// WebAuthn の呼び出しの前後のハーネスのスクリプトで起きた例外（ハーネス自身の固定の名前。ブラウザの拒否ではない）。
    HarnessScript,
    /// 既知の名前に当たらない値。
    Other,
}

impl WebAuthnErrorName {
    pub(crate) fn from_dom_name(name: &str) -> Self {
        match name {
            "NotAllowedError" => Self::NotAllowed,
            "InvalidStateError" => Self::InvalidState,
            "SecurityError" => Self::Security,
            "NotSupportedError" => Self::NotSupported,
            "AbortError" => Self::Abort,
            "ConstraintError" => Self::Constraint,
            "DataError" => Self::Data,
            "EncodingError" => Self::Encoding,
            "NetworkError" => Self::Network,
            "TimeoutError" => Self::Timeout,
            "UnknownError" => Self::Unknown,
            "TypeError" => Self::Type,
            "OriginMismatch" => Self::OriginMismatch,
            "HarnessTimeout" => Self::HarnessTimeout,
            "HarnessScript" => Self::HarnessScript,
            _ => Self::Other,
        }
    }
}
