//! 失効の通知（`logout_token`）を Hub 側で受ける口の模擬（TASK-7・#62）。試験専用で、実物の Hub ではない。
//!
//! 役割: 実物の Hub は `fandhe-auth` の送信器が送る Logout Token（Back-Channel Logout）を受け口で受け、
//! 検証して認証セッションの失効を適用する（RVPG-2・RVPG-7・RVPG-28）。呼び出し元は試験で、
//! 試験が実行時に生成した試験用の鍵で署名した Logout Token を [`LogoutTokenReceiver::receive`] へ渡す。
//! 受信の結果は [`LogoutReceipt`] として記録に残し、試験が「何件受け、どの `sid` が届いたか・重複はないか」を
//! 数えられるようにする（RVPG-2・RVPG-28 の受信側の計数）。送信器・outbox は本体側の担当で、ここには置かない。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻は引数 `now`（UNIX 秒）で受け、ここでは読まない。環境変数・ファイルも読まない（DPOL-32）。
//! - 署名の確認は `ring` に任せ、自前の暗号は書かない（DPOL-4・DPOL-47）。受けるのは ES256 だけで、
//!   `alg`・`typ`（`logout+jwt`）はコードに固定し、トークンや設定から導かない（SIGN-1・SIGN-6・SIGN-9・SIGN-10）。
//!   `jku`・`x5u`・`jwk` は読まない（SIGN-16）。`exp == now` は拒否する（SIGN-17）。
//! - 署名を確かめる前に、ペイロードの値を判定に使わない。
//! - 必須クレームは `iss`・`aud`・`exp`・`iat`・`jti`・`sub`、`events` は Back-Channel Logout のイベントを持つ。
//!   失効の対象は次のどちらか 1 つだけで、両方・どちらも無いものは拒否する。
//!   (1) クライアント別の `sid`（RVPG-7）。(2) `sid` を持たず、独自のクレーム（[`GROUPED_CLAIM`]）に
//!   `{"client_id":…,"sid":…}` の組の一覧を持つ Hub の sector 向けの形（RVPG-14。`sub` は Hub 用 sector の `sub`）。
//!   禁止は `nonce`・`scope`・`at_hash`・`client_id`（RVPG-7）。集合の正本は spec の RVPG-7・RVPG-14 で、ここは受け口の模擬。
//!   `iat` が `now` より先、または `iat >= exp` の通知は拒否する。
//! - 同じ `jti` の再受信（送信器の再送。RVPG-28）は拒否せず、重複の印を付けて記録する。適用は 1 回だけ数える。
//! - トークンの文字列は保持せず、誤りの `Display`・`Debug` にも出さない（SECR-23〜27）。拒否は記録を変えない。
//! - 判定しないもの: Hub が所有する値（`exp − iat` の上限など）。実物との差は `docs/stubs/hub-stub.md` に書いた（#69）。
//!   期待値の `issuer`・`audience` は試験用の仮の値で受ける（A-2026-10-28-1）。

use crate::presentation::{VerificationKey, decode_object};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
use serde_json::Value;

/// Back-Channel Logout のイベント名（OpenID Connect Back-Channel Logout 1.0）。
const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

/// RVPG-14: 組の一覧を載せる独自のクレーム名。spec は名前を示していない（「独自のクレーム」）ため、試験用の仮置き。
/// TODO(TASK-7): 本体側の送信器・Hub の実物のクレーム名が決まったら置き換える（#62）。
pub const GROUPED_CLAIM: &str = "session_pairs";

/// RVPG-7: Logout Token が持ってはならないクレーム。
const FORBIDDEN_CLAIMS: [&str; 4] = ["nonce", "scope", "at_hash", "client_id"];

/// 試験側が渡す期待値。試験用の仮の値で、本番の値を置かない（A-2026-10-28-1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoutExpectations {
    pub issuer: String,
    /// 受け口が自分宛と見なす `aud`（Hub の `client_id`。値は写さず試験用の仮の値で受ける）。
    pub audience: String,
}

/// 受理した失効の通知の記録。識別子のクレームだけを持ち、トークンの文字列は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoutReceipt {
    pub iss: String,
    pub sub: String,
    pub aud: Vec<String>,
    pub iat: u64,
    pub exp: u64,
    pub jti: String,
    /// クライアント別の `sid`（RVPG-7）。組の一覧の形（RVPG-14）では `None`。
    pub sid: Option<String>,
    /// `(client_id, sid)` の組の一覧（RVPG-14）。クライアント別の `sid` の形では空。
    pub pairs: Vec<(String, String)>,
    /// 同じ `jti` を既に受けていたか（再送。RVPG-28）。`true` の通知は失効の適用に数えない。
    pub duplicate: bool,
}

/// 受信の拒否の種類。固定の文言だけを出し、トークンやクレームの値を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutRejection {
    Malformed,
    HeaderMismatch,
    UnknownKey,
    SignatureMismatch,
    IssuerMismatch,
    AudienceMismatch,
    Expired,
    MissingClaim,
    InvalidIssuedAt,
    ForbiddenClaim,
    EventMismatch,
}

impl std::fmt::Display for LogoutRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "logout rejected: malformed",
            Self::HeaderMismatch => "logout rejected: header mismatch",
            Self::UnknownKey => "logout rejected: unknown key",
            Self::SignatureMismatch => "logout rejected: signature mismatch",
            Self::IssuerMismatch => "logout rejected: issuer mismatch",
            Self::AudienceMismatch => "logout rejected: audience mismatch",
            Self::Expired => "logout rejected: expired",
            Self::MissingClaim => "logout rejected: missing claim",
            Self::InvalidIssuedAt => "logout rejected: invalid issued-at",
            Self::ForbiddenClaim => "logout rejected: forbidden claim",
            Self::EventMismatch => "logout rejected: event mismatch",
        })
    }
}

impl std::error::Error for LogoutRejection {}

/// 失効の通知（`logout_token`）を受ける口の模擬。
#[derive(Debug)]
pub struct LogoutTokenReceiver {
    keys: Vec<VerificationKey>,
    expectations: LogoutExpectations,
    received: Vec<LogoutReceipt>,
}

impl LogoutTokenReceiver {
    pub fn new(keys: Vec<VerificationKey>, expectations: LogoutExpectations) -> Self {
        Self {
            keys,
            expectations,
            received: Vec::new(),
        }
    }

    /// 受信の記録（受理した順。重複の再送も印付きで残る）。
    pub fn received(&self) -> &[LogoutReceipt] {
        &self.received
    }

    /// 失効の適用として数える通知（重複でないもの）。
    pub fn applied(&self) -> impl Iterator<Item = &LogoutReceipt> {
        self.received.iter().filter(|r| !r.duplicate)
    }

    /// 失効が反映された `sid` の一覧（重複排除。クライアント別の `sid` の形と組の一覧の両方から集める。受理順）。
    /// 同じ `sid` が別の通知・再送で届いても 1 回と数える（RVPG-14・RVPG-28）。
    pub fn applied_sids(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for r in self.applied() {
            let sids = r
                .sid
                .iter()
                .map(String::as_str)
                .chain(r.pairs.iter().map(|(_, s)| s.as_str()));
            for sid in sids {
                if !out.contains(&sid) {
                    out.push(sid);
                }
            }
        }
        out
    }

    /// 失効が反映された `(client_id, sid)` の組（重複排除。受理順。RVPG-14）。
    pub fn applied_pairs(&self) -> Vec<(&str, &str)> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for r in self.applied() {
            for (c, s) in &r.pairs {
                let pair = (c.as_str(), s.as_str());
                if !out.contains(&pair) {
                    out.push(pair);
                }
            }
        }
        out
    }

    /// Logout Token を受ける。受理したら記録に積んで返し、拒否したときは記録を変えない。
    pub fn receive(&mut self, token: &str, now: u64) -> Result<LogoutReceipt, LogoutRejection> {
        use LogoutRejection as R;
        let mut parts = token.split('.');
        let (h, p, s) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(h), Some(p), Some(s), None) => (h, p, s),
            _ => return Err(R::Malformed),
        };
        let header = decode_object(h).map_err(|_| R::Malformed)?;
        let sig = URL_SAFE_NO_PAD.decode(s).map_err(|_| R::Malformed)?;
        let payload_bytes = URL_SAFE_NO_PAD.decode(p).map_err(|_| R::Malformed)?;

        // SIGN-1・SIGN-6・SIGN-9・SIGN-10: alg・typ は固定値との完全一致だけを受ける。
        if header.get("alg").and_then(Value::as_str) != Some("ES256")
            || header.get("typ").and_then(Value::as_str) != Some("logout+jwt")
            || header.contains_key("crit")
        {
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
        // RVPG-7: 禁止のクレームを持つ通知は拒否する。
        if FORBIDDEN_CLAIMS.iter().any(|k| claims.contains_key(*k)) {
            return Err(R::ForbiddenClaim);
        }
        let st = |k: &str| {
            claims
                .get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(R::MissingClaim)
        };
        let num = |k: &str| claims.get(k).and_then(Value::as_u64).ok_or(R::MissingClaim);
        let aud = match claims.get("aud") {
            Some(Value::String(a)) => vec![a.clone()],
            Some(Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or(R::MissingClaim))
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(R::MissingClaim),
        };
        let mut receipt = LogoutReceipt {
            iss: st("iss")?,
            sub: st("sub")?,
            aud,
            iat: num("iat")?,
            exp: num("exp")?,
            jti: st("jti")?,
            sid: None,
            pairs: Vec::new(),
            duplicate: false,
        };
        // RVPG-7 / RVPG-14: 失効の対象は sid か組の一覧のどちらか 1 つだけ。
        match (claims.get("sid"), claims.get(GROUPED_CLAIM)) {
            (Some(_), Some(_)) | (None, None) => return Err(R::MissingClaim),
            (Some(Value::String(sid)), None) => receipt.sid = Some(sid.clone()),
            (None, Some(Value::Array(list))) if !list.is_empty() => {
                for item in list {
                    let obj = item.as_object().ok_or(R::Malformed)?;
                    let get = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_owned);
                    match (obj.len(), get("client_id"), get("sid")) {
                        (2, Some(c), Some(s)) => receipt.pairs.push((c, s)),
                        _ => return Err(R::Malformed),
                    }
                }
            }
            _ => return Err(R::Malformed),
        }
        // events は Back-Channel Logout のイベントをオブジェクトとして持つ。
        match claims.get("events") {
            Some(Value::Object(e))
                if matches!(e.get(BACKCHANNEL_LOGOUT_EVENT), Some(Value::Object(_))) => {}
            Some(_) => return Err(R::EventMismatch),
            None => return Err(R::MissingClaim),
        }
        if receipt.iss != self.expectations.issuer {
            return Err(R::IssuerMismatch);
        }
        if !receipt.aud.contains(&self.expectations.audience) {
            return Err(R::AudienceMismatch);
        }
        // iat は未来であってはならず、exp より前でなければならない。
        if receipt.iat > now || receipt.iat >= receipt.exp {
            return Err(R::InvalidIssuedAt);
        }
        // SIGN-17: exp == now も拒否。
        if receipt.exp <= now {
            return Err(R::Expired);
        }
        // RVPG-28: 再送（同じ jti）は受けるが適用には数えない。
        receipt.duplicate = self.received.iter().any(|r| r.jti == receipt.jti);
        self.received.push(receipt.clone());
        Ok(receipt)
    }
}
