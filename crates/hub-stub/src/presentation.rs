//! 本人トークン（アクセストークン）の提示を Hub 側で受ける口の模擬（TASK-7・#60）。試験専用で、実物の Hub ではない。
//!
//! 役割: Hub のトークンエンドポイント側が、`fandhe-auth` の発行した本人トークンを受けて署名と基本のクレームを
//! 確かめる動きを模す（CLM-21・CLM-26・CLM-28 の「Hub へ提示する」向きの書き手の読み）。呼び出し元は試験で、
//! 試験が実行時に生成した試験用の鍵で署名したトークンを [`IdentityTokenReceiver::present`] へ渡す。
//! 受理した結果は [`ReceivedIdentity`] として記録し、後続の失効通知の模擬が突き合わせに使える。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻は引数 `now`（UNIX 秒）で受け、ここでは読まない。環境変数・ファイルも読まない（DPOL-32）。
//! - 署名の確認は `ring` に任せ、自前の暗号は書かない（DPOL-4・DPOL-47）。
//! - 受けるのは ES256 だけ。`alg`・`typ` はコードに固定し、トークンや設定から導かない（SIGN-1・SIGN-6・SIGN-9・SIGN-10）。
//!   `jku`・`x5u`・`jwk` は読まない（SIGN-16）。`exp == now` は拒否する（SIGN-17）。
//! - 署名を確かめる前に、ペイロードの値を判定に使わない。
//! - 必須クレームは CLM-1、任意は CLM-2、認証イベント ID のクレーム名は `<issuer>/claims/auth_event_id`（CLM-17）。
//! - トークンの文字列は保持せず、誤りの `Display`・`Debug` にも出さない（SECR-23〜27）。
//! - 判定しないもの: Hub が所有する値（有効期限の上限・`iat` の許容など）。実物との差として #69・#70 で文書にする。
//!   期待値の `issuer`・`audience` は試験用の仮の値で受ける（A-2026-10-28-1）。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use serde_json::{Map, Value};

/// 検証に使う公開鍵。公開鍵だけなので秘密値ではない。後続の JWKS の模擬（#65）に置き換わる前提。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationKey {
    pub(crate) kid: String,
    pub(crate) public_key: Vec<u8>,
}

impl VerificationKey {
    /// ES256（P-256）の公開鍵。`public_key` は非圧縮の点（65 バイト）。
    pub fn es256(kid: impl Into<String>, public_key: impl Into<Vec<u8>>) -> Self {
        Self {
            kid: kid.into(),
            public_key: public_key.into(),
        }
    }
}

/// 試験側が渡す期待値。試験用の仮の値で、本番の値を置かない（A-2026-10-28-1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationExpectations {
    pub issuer: String,
    pub audience: String,
}

/// 受理した本人トークンの記録。識別子のクレームだけを持ち、トークンの文字列は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedIdentity {
    pub iss: String,
    pub sub: String,
    pub aud: Vec<String>,
    pub iat: u64,
    pub exp: u64,
    pub jti: String,
    pub sid: String,
    pub client_id: String,
    pub scope: String,
    pub auth_time: Option<u64>,
    pub acr: Option<String>,
    pub amr: Option<Vec<String>>,
    /// `<issuer>/claims/auth_event_id`（CLM-17）。
    pub auth_event_id: Option<String>,
}

/// 提示の拒否の種類。固定の文言だけを出し、トークンやクレームの値を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationRejection {
    Malformed,
    HeaderMismatch,
    UnknownKey,
    SignatureMismatch,
    IssuerMismatch,
    AudienceMismatch,
    Expired,
    MissingClaim,
}

impl std::fmt::Display for PresentationRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "presentation rejected: malformed",
            Self::HeaderMismatch => "presentation rejected: header mismatch",
            Self::UnknownKey => "presentation rejected: unknown key",
            Self::SignatureMismatch => "presentation rejected: signature mismatch",
            Self::IssuerMismatch => "presentation rejected: issuer mismatch",
            Self::AudienceMismatch => "presentation rejected: audience mismatch",
            Self::Expired => "presentation rejected: expired",
            Self::MissingClaim => "presentation rejected: missing claim",
        })
    }
}

impl std::error::Error for PresentationRejection {}

/// 本人トークンの提示を受ける口の模擬。
#[derive(Debug)]
pub struct IdentityTokenReceiver {
    keys: Vec<VerificationKey>,
    expectations: PresentationExpectations,
    received: Vec<ReceivedIdentity>,
}

impl IdentityTokenReceiver {
    pub fn new(keys: Vec<VerificationKey>, expectations: PresentationExpectations) -> Self {
        Self {
            keys,
            expectations,
            received: Vec::new(),
        }
    }

    /// 受理した提示の記録。
    pub fn received(&self) -> &[ReceivedIdentity] {
        &self.received
    }

    /// 本人トークンの提示を受ける。受理したら記録に積んで返し、拒否したときは記録を変えない。
    pub fn present(
        &mut self,
        token: &str,
        now: u64,
    ) -> Result<ReceivedIdentity, PresentationRejection> {
        use PresentationRejection as R;
        let mut parts = token.split('.');
        let (h, p, s) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(h), Some(p), Some(s), None) => (h, p, s),
            _ => return Err(R::Malformed),
        };
        let header = decode_object(h)?;
        let sig = URL_SAFE_NO_PAD.decode(s).map_err(|_| R::Malformed)?;
        let payload_bytes = URL_SAFE_NO_PAD.decode(p).map_err(|_| R::Malformed)?;

        // SIGN-1・SIGN-6・SIGN-9・SIGN-10: alg・typ は固定値との完全一致だけを受ける。
        if header.get("alg").and_then(Value::as_str) != Some("ES256")
            || header.get("typ").and_then(Value::as_str) != Some("at+jwt")
        {
            return Err(R::HeaderMismatch);
        }
        // 未対応の拡張は実装しない段階のため、crit を持つヘッダは拒否する（RFC 7515 4.1.11）。
        if header.contains_key("crit") {
            return Err(R::HeaderMismatch);
        }
        // SIGN-16: kid は手元の一覧からだけ引く。
        let kid = header
            .get("kid")
            .and_then(Value::as_str)
            .ok_or(R::UnknownKey)?;
        let key = self
            .keys
            .iter()
            .find(|k| k.kid == kid)
            .ok_or(R::UnknownKey)?;
        let signing_input = &token[..h.len() + 1 + p.len()];
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &key.public_key)
            .verify(signing_input.as_bytes(), &sig)
            .map_err(|_| R::SignatureMismatch)?;

        // 署名の確認後にだけペイロードを使う。
        let claims = match serde_json::from_slice::<Value>(&payload_bytes) {
            Ok(Value::Object(m)) => m,
            _ => return Err(R::Malformed),
        };
        let identity = self.build_identity(&claims)?;
        if identity.iss != self.expectations.issuer {
            return Err(R::IssuerMismatch);
        }
        if !identity.aud.contains(&self.expectations.audience) {
            return Err(R::AudienceMismatch);
        }
        // SIGN-17: exp == now も拒否。
        if identity.exp <= now {
            return Err(R::Expired);
        }
        self.received.push(identity.clone());
        Ok(identity)
    }

    fn build_identity(
        &self,
        c: &Map<String, Value>,
    ) -> Result<ReceivedIdentity, PresentationRejection> {
        use PresentationRejection as R;
        let st = |k: &str| {
            c.get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(R::MissingClaim)
        };
        let num = |k: &str| c.get(k).and_then(Value::as_u64).ok_or(R::MissingClaim);
        let aud = match c.get("aud") {
            Some(Value::String(a)) => vec![a.clone()],
            Some(Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or(R::MissingClaim))
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(R::MissingClaim),
        };
        // 任意クレームは「欠落」なら None、「存在するが型不正」なら拒否する。
        let opt_str = |k: &str| match c.get(k) {
            None => Ok(None),
            Some(Value::String(v)) => Ok(Some(v.clone())),
            Some(_) => Err(R::MissingClaim),
        };
        let auth_time = match c.get("auth_time") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or(R::MissingClaim)?),
        };
        let amr = match c.get("amr") {
            None => None,
            Some(Value::Array(a)) => Some(
                a.iter()
                    .map(|v| v.as_str().map(str::to_owned).ok_or(R::MissingClaim))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Some(_) => return Err(R::MissingClaim),
        };
        // CLM-17: 認証イベント ID のクレーム名は期待値の issuer から組み立てる。
        let event_key = format!("{}/claims/auth_event_id", self.expectations.issuer);
        Ok(ReceivedIdentity {
            iss: st("iss")?,
            sub: st("sub")?,
            aud,
            iat: num("iat")?,
            exp: num("exp")?,
            jti: st("jti")?,
            sid: st("sid")?,
            client_id: st("client_id")?,
            scope: st("scope")?,
            auth_time,
            acr: opt_str("acr")?,
            amr,
            auth_event_id: opt_str(&event_key)?,
        })
    }
}

pub(crate) fn decode_object(part: &str) -> Result<Map<String, Value>, PresentationRejection> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| PresentationRejection::Malformed)?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(PresentationRejection::Malformed),
    }
}
