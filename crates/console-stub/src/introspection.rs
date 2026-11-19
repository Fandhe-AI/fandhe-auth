//! console が `fandhe-auth` の introspection を呼ぶ側の模擬（TASK-8・#73）。試験専用で、実物の console ではない。
//!
//! 役割: 委任の「現在の権限の確認」を模す。console は配送のたびに introspection で `active`・`cnf.jkt`・
//! scope を最初から確かめ、過去の確認を再利用しない（ADLG-15 ①）。呼び出し元は試験で、応答は試験が
//! 送る口のクロージャで返す（`fandhe-auth` の introspection の偽物ではない）。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻は引数 `now`（UNIX 秒）で受け、環境変数・ファイル・ネットワークを読まない（DPOL-32）。
//! - 「有効」になる経路は、`200` かつ `active` が真偽値 `true` で、要件を満たすときだけ。401・503・その他の状態・
//!   本文の形の誤り・送信の失敗は `Err` で、決して有効にしない（AVLB-18・AVLB-22 の向きの書き手の読み）。
//! - 判定は応答の値だけで行う。申告された値を権限の判定に使わない（D-2026-10-17-49 の (3)）。
//!   応答の `aud` は判定に使わない（照会できる `aud` の制限は Provider 側。OCL-57・OCL-58）。
//! - 無効に理由を持たせない（OPRV-54）。`sid` で照会する口は作らない（OPRV-56・AGCR-32）。
//! - token は引数で借りるだけで、構造体・誤り・出力に保持しない（SECR-23〜27）。
//! - 判定しないもの: 有効性の照会の口（#74）、遅延・拒否の注入（#76）、呼び出しの記録（#75）。

use serde_json::Value;

use crate::config::ConsoleStubConfig;

/// 送る口へ渡す要求の記述。POST の form（OPRV-54）として送る側が符号化する。token は含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrospectionRequest {
    /// introspection の口の URL（設定の値。取得はしない）。
    pub endpoint: String,
    /// console のクライアントの `client_id`。
    pub client_id: String,
    /// RFC 7662 2.1 の `token_type_hint`。固定。
    pub token_type_hint: &'static str,
}

// TODO(TASK-8): private_key_jwt の assertion は鍵を足す段で fandhe-auth-secret の型を使い、許可表を見直してから足す。

/// 送る口が返す応答。本文は秘密値ではないが、`Debug` は状態コードだけを出す。
#[derive(Clone, PartialEq, Eq)]
pub struct IntrospectionHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for IntrospectionHttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntrospectionHttpResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// 送れなかったこと（接続の失敗等）。理由は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportFailure;

/// 試験側が渡す要件。値は試験用の仮の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityRequirement {
    /// 応答の `scope`（空白区切り）に完全一致で含まれること。
    pub required_scope: String,
    /// `Some` のとき `token_type` が `DPoP` かつ `cnf.jkt` が完全一致すること（AGCR-32・ADLG-15 ①）。
    pub bound_jkt: Option<String>,
}

/// 有効と判定した応答の値。`sub` は読まない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGrant {
    pub scope: Vec<String>,
    pub client_id: Option<String>,
    pub token_type: Option<String>,
    pub cnf_jkt: Option<String>,
    pub exp: Option<u64>,
}

/// 現在の権限の確認の結果。`Invalid` は理由を持たない（OPRV-54）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentAuthority {
    Valid(ActiveGrant),
    Invalid,
}

/// 確認できなかったこと。どれも「有効」にならない（AVLB-18・AVLB-22）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrospectionFailure {
    /// 401。クライアント認証の誤り（OPRV-54）。
    ClientAuthRejected,
    /// 503。fail-closed の間（AVLB-18・AVLB-22）。
    Unavailable,
    /// 上記以外の状態コード。
    UnexpectedStatus,
    /// 本文が JSON でない、または必須項目の型が不正。
    MalformedResponse,
    /// 送れなかった。
    TransportFailure,
}

impl std::fmt::Display for IntrospectionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ClientAuthRejected => "introspection: client authentication rejected",
            Self::Unavailable => "introspection: service unavailable",
            Self::UnexpectedStatus => "introspection: unexpected status",
            Self::MalformedResponse => "introspection: malformed response",
            Self::TransportFailure => "introspection: transport failure",
        })
    }
}

impl std::error::Error for IntrospectionFailure {}

/// console 側の確認の入口。確認の結果を保持しない（キャッシュ 0。ADLG-15）。
#[derive(Debug, Clone)]
pub struct ConsoleIntrospector {
    endpoint: String,
    client_id: String,
}

impl ConsoleIntrospector {
    /// 設定から必要な値だけを写す。
    pub fn new(config: &ConsoleStubConfig) -> Self {
        Self {
            endpoint: config.introspection_endpoint.clone(),
            client_id: config.client_id.clone(),
        }
    }

    /// 送る口へ渡す要求の記述。
    pub fn request(&self) -> IntrospectionRequest {
        IntrospectionRequest {
            endpoint: self.endpoint.clone(),
            client_id: self.client_id.clone(),
            token_type_hint: "access_token",
        }
    }

    /// 毎回 `send` を 1 回呼んで確かめる。`token` は送る口へ渡すだけで保持しない。
    pub fn check<F>(
        &self,
        token: &str,
        requirement: &AuthorityRequirement,
        now: u64,
        send: F,
    ) -> Result<CurrentAuthority, IntrospectionFailure>
    where
        F: FnOnce(
            &IntrospectionRequest,
            &str,
        ) -> Result<IntrospectionHttpResponse, TransportFailure>,
    {
        let response =
            send(&self.request(), token).map_err(|_| IntrospectionFailure::TransportFailure)?;
        evaluate(&response, requirement, now)
    }
}

/// 応答の解釈だけを行う。有効になるのは `200`・`active:true`・要件の充足のときだけ。
pub fn evaluate(
    response: &IntrospectionHttpResponse,
    requirement: &AuthorityRequirement,
    now: u64,
) -> Result<CurrentAuthority, IntrospectionFailure> {
    match response.status {
        200 => {}
        401 => return Err(IntrospectionFailure::ClientAuthRejected),
        503 => return Err(IntrospectionFailure::Unavailable),
        _ => return Err(IntrospectionFailure::UnexpectedStatus),
    }
    let malformed = IntrospectionFailure::MalformedResponse;
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| malformed)?;
    let obj = value.as_object().ok_or(malformed)?;
    match obj.get("active") {
        Some(Value::Bool(false)) => return Ok(CurrentAuthority::Invalid),
        Some(Value::Bool(true)) => {}
        _ => return Err(malformed),
    }

    let opt_str = |key: &str| -> Result<Option<String>, IntrospectionFailure> {
        match obj.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(malformed),
        }
    };
    let scope: Vec<String> = match opt_str("scope")? {
        Some(s) => s.split_whitespace().map(str::to_owned).collect(),
        None => Vec::new(),
    };
    let exp = match obj.get("exp") {
        None => None,
        Some(v) => Some(v.as_u64().ok_or(malformed)?),
    };
    let cnf_jkt = match obj.get("cnf") {
        None => None,
        Some(Value::Object(cnf)) => match cnf.get("jkt") {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(malformed),
        },
        Some(_) => return Err(malformed),
    };
    let grant = ActiveGrant {
        scope,
        client_id: opt_str("client_id")?,
        token_type: opt_str("token_type")?,
        cnf_jkt,
        exp,
    };

    // SIGN-17 の向き: exp == now は無効。
    if matches!(grant.exp, Some(exp) if exp <= now) {
        return Ok(CurrentAuthority::Invalid);
    }
    if !grant.scope.contains(&requirement.required_scope) {
        return Ok(CurrentAuthority::Invalid);
    }
    if let Some(jkt) = &requirement.bound_jkt {
        let bound = grant.token_type.as_deref() == Some("DPoP")
            && grant.cnf_jkt.as_deref() == Some(jkt.as_str());
        if !bound {
            return Ok(CurrentAuthority::Invalid);
        }
    }
    Ok(CurrentAuthority::Valid(grant))
}
