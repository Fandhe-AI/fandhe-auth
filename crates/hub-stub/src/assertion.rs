//! Hub が `private_key_jwt` の assertion で `fandhe-auth` の口を呼ぶ側の模擬（TASK-7・#61）。
//! 試験専用で、実物の Hub ではない。
//!
//! 役割: 実物の Hub は assertion を作り、5 つの口（トークンエンドポイント・イントロスペクション・
//! revocation・変更のフィード・監査の照会）を呼ぶ（OCL-21）。呼び出し元は試験（と後の統合試験）で、
//! 規則に合う assertion、または欄を書き換えた assertion を作り、口へ渡して受理・拒否を記録する。
//! 呼び出し先の口は [`AuthPort`] で表す。実物の口（`backend`）への配線は後続のタスクで行う。
//!
//! 方向の読み（書き手の読み）: Issue の文言は「受けて検証する」とも読めるが、spec の TASK-7 と OCL-21 は
//! Hub が assertion を出して `fandhe-auth` を呼ぶ側とする。assertion の検証（OCL-22〜28・SIGN-7・SIGN-56）は
//! 本体（core）の担当で、5 つの口が同じ検査の部品を使う。ここには検証部を置かない。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻 `now`（UNIX 秒）と `jti` は引数で受け、時計は読まない。環境変数・ファイルも読まない（DPOL-32）。
//!   `jti` の一意性は呼び出し側が持つ。OCL-27 の重複の作り分けに要るため、ここでは重複を止めない。
//! - 署名は `ring` の ES256（r‖s の 64 バイト）だけ（OCL-23・DPOL-4・DPOL-47）。`alg` はコードに固定し、
//!   設定や入力から導かない（SIGN-1・SIGN-6）。`jku`・`x5u`・`jwk`・`x5c` は出さない（SIGN-16・SIGN-56）。
//! - `typ` は付けない。spec（OCL-21〜28）に定めが無く、推測で足さない。Hub `AUTHC-39` との照合と
//!   実物との差は #69・#70 で文書にする。
//! - 資格情報は POST の form の本文だけで渡す（OCL-30）。ここは名前と値の並びまでを作り、
//!   `application/x-www-form-urlencoded` への符号化は口の実装側が行う。
//! - assertion の文字列は `Debug`・誤り・記録に出さない（SECR-23〜27）。
//!
//! TODO(TASK-511): 署名鍵と assertion は秘密値の型（`fandhe-auth-secret`）が未実装のため、`ring` の不透明な型と
//! 固定の `Debug` を持つ自前の型で持つ。実装後にその型へ置き換え、許可表（`scripts/check-crate-deps.sh`）を
//! 見直す。表の変更は利用者に確かめる。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair};
use serde_json::{Map, Value, json};

use crate::config::HubStubConfig;

/// `client_assertion_type` の正しい値（OCL-21）。
const JWT_BEARER: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

/// assertion の署名鍵。試験が実行時に生成した ES256 の鍵を受ける。`Debug` は固定の文字列だけを出す（SECR-11）。
pub struct AssertionSigningKey {
    kid: String,
    key_pair: EcdsaKeyPair,
}

impl AssertionSigningKey {
    /// ES256（P-256）の鍵。`key_pair` は `ECDSA_P256_SHA256_FIXED_SIGNING` で作ったもの（OCL-23）。
    pub fn es256(kid: impl Into<String>, key_pair: EcdsaKeyPair) -> Self {
        Self {
            kid: kid.into(),
            key_pair,
        }
    }

    /// 非圧縮の公開鍵の点（65 バイト）。JWKS の模擬（#65）と試験側の署名の確認に使う。
    pub fn public_key(&self) -> &[u8] {
        self.key_pair.public_key().as_ref()
    }
}

impl std::fmt::Debug for AssertionSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssertionSigningKey(<redacted>)")
    }
}

/// 呼ぶ相手の発行者の識別子。assertion の `aud` になる（OCL-22）。`fandhe-auth` 側の値で、試験用の仮の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionTarget {
    pub issuer: String,
}

/// `aud` の形。規則に合うのは `Single(issuer)` だけで、`List` は OCL-22 の作り分けに使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audience {
    Single(String),
    List(Vec<String>),
}

/// assertion のクレーム。試験側が欄を書き換えて、欠けや食い違いのある assertion を作れる（OCL-22・24・25・27）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionClaims {
    pub iss: String,
    pub sub: String,
    pub aud: Audience,
    pub iat: Option<u64>,
    pub exp: Option<u64>,
    pub nbf: Option<u64>,
    pub jti: Option<String>,
}

/// 署名した assertion（compact な JWT）。`Debug` は固定の文字列で、`Display` は持たない。
/// 取り出しは [`ClientAssertion::expose_compact`] だけ（SECR-16 の考え方）。TODO(TASK-511): 秘密値の型へ置き換える。
pub struct ClientAssertion {
    compact: String,
    jti: Option<String>,
}

impl ClientAssertion {
    /// 口へ渡すために文字列を取り出す。ログ・誤り・記録に使わない（SECR-23〜27）。
    pub fn expose_compact(&self) -> &str {
        &self.compact
    }
}

impl std::fmt::Debug for ClientAssertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientAssertion(<redacted>)")
    }
}

/// 呼ぶ口の種類（OCL-21）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalledEndpoint {
    Token,
    Introspection,
    Revocation,
    ChangeFeed,
    AuditQuery,
}

/// `client_assertion_type` の作り分け。`JwtBearer` が正しい値で、他は OCL-21 の負の場合に使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientAssertionType {
    JwtBearer,
    Omitted,
    Other(String),
}

/// 口へ渡す要求。名前と値の並びで持ち、form の符号化は口の実装側に任せる（OCL-30）。
#[derive(Clone, PartialEq, Eq)]
pub struct AssertionRequest {
    endpoint: CalledEndpoint,
    params: Vec<(String, String)>,
    jti: Option<String>,
}

impl AssertionRequest {
    pub fn endpoint(&self) -> CalledEndpoint {
        self.endpoint
    }

    /// form の本文に入れる名前と値の並び。`client_assertion` の値を含むので、記録に使わない。
    pub fn params(&self) -> &[(String, String)] {
        &self.params
    }

    /// 名前で最初の値を引く。
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 要求に使った assertion の `jti`（記録用。秘密値ではない）。
    pub fn jti(&self) -> Option<&str> {
        self.jti.as_deref()
    }
}

impl std::fmt::Debug for AssertionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SECR-23〜27: client_assertion の値は伏せる。
        let masked: Vec<(&str, &str)> = self
            .params
            .iter()
            .map(|(k, v)| {
                if k == "client_assertion" {
                    (k.as_str(), "<redacted>")
                } else {
                    (k.as_str(), v.as_str())
                }
            })
            .collect();
        f.debug_struct("AssertionRequest")
            .field("endpoint", &self.endpoint)
            .field("params", &masked)
            .field("jti", &self.jti)
            .finish()
    }
}

/// 口が返した結果。拒否の形は OPRV-41・OPRV-42 の `invalid_client` の 401 を想定する（OCL-21・OCL-29）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortResponse {
    Accepted,
    Rejected { status: u16, error: Option<String> },
}

/// `fandhe-auth` の口を表す注入点。実物の配線は後続のタスクで行い、本段では試験側の偽の口が実装する。
pub trait AuthPort {
    fn call(&mut self, request: &AssertionRequest) -> PortResponse;
}

/// 呼び出しの結果の記録。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Accepted,
    Rejected { status: u16 },
}

/// 呼び出しの記録。assertion の文字列は持たない（SECR-23〜27）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub endpoint: CalledEndpoint,
    pub jti: Option<String>,
    pub outcome: CallOutcome,
}

/// 口が拒否したことを表す誤り。assertion も応答の本文も持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallRejected {
    pub status: u16,
}

impl std::fmt::Display for CallRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "port rejected the call: status {}", self.status)
    }
}

impl std::error::Error for CallRejected {}

/// 署名に失敗したことを表す誤り。値を載せない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SigningFailed;

impl std::fmt::Display for SigningFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("assertion signing failed")
    }
}

impl std::error::Error for SigningFailed {}

/// Hub が assertion を作って `fandhe-auth` の口を呼ぶ模擬の入口。
#[derive(Debug)]
pub struct HubAssertionCaller {
    client_id: String,
    max_lifetime_secs: u64,
    target: AssertionTarget,
    key: AssertionSigningKey,
    rng: SystemRandom,
    records: Vec<CallRecord>,
}

impl HubAssertionCaller {
    pub fn new(config: &HubStubConfig, target: AssertionTarget, key: AssertionSigningKey) -> Self {
        Self {
            client_id: config.client_id.clone(),
            max_lifetime_secs: config.assertion_max_lifetime.as_secs(),
            target,
            key,
            rng: SystemRandom::new(),
            records: Vec::new(),
        }
    }

    /// 規則に合うクレーム。`iss`＝`sub`＝`client_id`（OCL-24）、`aud` は発行者の識別子だけ（OCL-22）、
    /// `exp − iat` は上限ちょうど（OCL-25）、`jti` あり（OCL-27）。
    pub fn conforming_claims(&self, jti: impl Into<String>, now: u64) -> AssertionClaims {
        AssertionClaims {
            iss: self.client_id.clone(),
            sub: self.client_id.clone(),
            aud: Audience::Single(self.target.issuer.clone()),
            iat: Some(now),
            exp: Some(now.saturating_add(self.max_lifetime_secs)),
            nbf: None,
            jti: Some(jti.into()),
        }
    }

    /// クレームに ES256 で署名する。ヘッダは `alg` と `kid` だけ（SIGN-1・SIGN-6・SIGN-16）。
    pub fn sign(&self, claims: &AssertionClaims) -> Result<ClientAssertion, SigningFailed> {
        let header = json!({"alg": "ES256", "kid": self.key.kid});
        let mut payload = Map::new();
        payload.insert("iss".into(), Value::String(claims.iss.clone()));
        payload.insert("sub".into(), Value::String(claims.sub.clone()));
        payload.insert(
            "aud".into(),
            match &claims.aud {
                Audience::Single(a) => Value::String(a.clone()),
                Audience::List(l) => Value::Array(l.iter().cloned().map(Value::String).collect()),
            },
        );
        for (name, v) in [
            ("iat", claims.iat),
            ("exp", claims.exp),
            ("nbf", claims.nbf),
        ] {
            if let Some(v) = v {
                payload.insert(name.into(), Value::from(v));
            }
        }
        if let Some(j) = &claims.jti {
            payload.insert("jti".into(), Value::String(j.clone()));
        }
        let enc = |v: &Value| -> Result<String, SigningFailed> {
            Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).map_err(|_| SigningFailed)?))
        };
        let input = format!("{}.{}", enc(&header)?, enc(&Value::Object(payload))?);
        // OCL-23: 署名は r‖s の 64 バイト。
        let sig = self
            .key
            .key_pair
            .sign(&self.rng, input.as_bytes())
            .map_err(|_| SigningFailed)?;
        Ok(ClientAssertion {
            compact: format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())),
            jti: claims.jti.clone(),
        })
    }

    /// 口へ渡す要求を作る。`client_id` は設定の値と一致させる（OCL-24）。`extra` は口ごとの追加の値。
    pub fn request(
        &self,
        endpoint: CalledEndpoint,
        assertion: &ClientAssertion,
        assertion_type: ClientAssertionType,
        include_client_id: bool,
        extra: Vec<(String, String)>,
    ) -> AssertionRequest {
        let mut params = Vec::new();
        match assertion_type {
            ClientAssertionType::JwtBearer => {
                params.push(("client_assertion_type".to_owned(), JWT_BEARER.to_owned()));
            }
            ClientAssertionType::Omitted => {}
            ClientAssertionType::Other(v) => {
                params.push(("client_assertion_type".to_owned(), v));
            }
        }
        params.push((
            "client_assertion".to_owned(),
            assertion.expose_compact().to_owned(),
        ));
        if include_client_id {
            params.push(("client_id".to_owned(), self.client_id.clone()));
        }
        params.extend(extra);
        AssertionRequest {
            endpoint,
            params,
            jti: assertion.jti.clone(),
        }
    }

    /// 口を呼ぶ。受理なら `Ok`、拒否なら `Err`。どちらも記録に積み、拒否を受理として数えない。
    pub fn call<P: AuthPort>(
        &mut self,
        port: &mut P,
        request: &AssertionRequest,
    ) -> Result<(), CallRejected> {
        let (outcome, result) = match port.call(request) {
            PortResponse::Accepted => (CallOutcome::Accepted, Ok(())),
            PortResponse::Rejected { status, .. } => (
                CallOutcome::Rejected { status },
                Err(CallRejected { status }),
            ),
        };
        self.records.push(CallRecord {
            endpoint: request.endpoint,
            jti: request.jti.clone(),
            outcome,
        });
        result
    }

    /// 呼び出しの記録。
    pub fn records(&self) -> &[CallRecord] {
        &self.records
    }
}
