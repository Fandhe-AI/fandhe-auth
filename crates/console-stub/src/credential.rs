//! エージェントの資格情報の発行・失効の呼び出しの模擬（TASK-8・#75）。試験専用で、実物の console ではない。
//!
//! 役割: 統合試験で「資格情報を発行する → 失効する → 配送の時点の確認（[`crate::delivery`]）で拒否される」
//! 流れ（PoC-11 手順 3 の (i)、ADLG-11）を駆動する。呼び出し元は試験で、相手側の応答は [`CredentialPorts`] で返す。
//!
//! 呼び出しの向き（書き手の読み）: エージェントの実装は console の `LAGT` が持つ（D-2026-11-05-6）。
//! したがって本モジュールは console 側（エージェント）が `fandhe-auth` を呼ぶ側を模し、`fandhe-auth` の口の偽物ではない。
//! - 発行: Device Authorization の開始（DGRT-1。DPoP の proof を開始に付けられる AGCR-8）と、
//!   `device_code` grant での交換（DGRT-7・DGRT-8。DPoP 結合は `token_type=DPoP`・`cnf.jkt`、AGCR-1）。
//! - 失効: エージェント自身の revocation（RFC 7009。AGCR-43・OPRV-53）と、エージェントの登録の失効
//!   （AGCR-35・AGCR-36。受付の正本は Hub ARUN-58）。後者は本人の画面操作を試験が代わりに起こす呼び出しで、
//!   判定・承認・自動失効を行う経路ではない（AGCR-35・HAPR-1）。
//! - 経路名は最終でなく（D-2026-10-17-84）、URL はすべて設定で受ける。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻・ネットワーク・環境変数・ファイルを読まない（DPOL-32）。
//! - `device_code`・DPoP の proof・失効させる token は引数で借りて口へ渡すだけで、構造体・誤り・記録・`Debug`
//!   に残さない（SECR-23〜27）。発行の答えも token の値を含まない型で受ける。
//! - 成功の経路は「発行の答えが `Issued` で、proof を付けたなら DPoP 結合」だけ。拒否・送れない・結合の不一致は
//!   全て `Err` か非成功の値で、成功に数えない（AVLB-18・AVLB-22 の向き）。承認・拒否・期限切れは口の答え
//!   （試験の設定値）として現れるだけで、本モジュールは承認を模さない。
//! - 各呼び出しは結果に関わらず 1 件記録してから返す（[`crate::record`]）。
//! - 判定しないもの: DPoP の proof の生成、遅延・拒否の注入（[`crate::fault`]。#76）、仮定の契約の印（[`crate::contract`]）、実物との差（#79）。

use crate::config::AgentCredentialConfig;
use crate::introspection::TransportFailure;
use crate::record::{CallKind, CallLog, CallOutcome};

// TODO(TASK-8): proof を生成しない点・型付きの答えで受け
// HTTP の本文を解釈しない点・refresh の grant を模さない点・登録の失効が人の画面操作の代わりである点は #79 に書く。
// TODO(TASK-8): 鍵・proof を持たせる段で fandhe-auth-secret の型を使い、許可表を見直す。

/// `device_code` grant の識別子（DGRT-11）。
pub const DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// RFC 7009 の `token_type_hint`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenTypeHint {
    AccessToken,
    RefreshToken,
}

impl TokenTypeHint {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AccessToken => "access_token",
            Self::RefreshToken => "refresh_token",
        }
    }
}

/// 開始の口へ渡す要求の記述（DGRT-1）。公開クライアントで `client_id` だけで識別する（AGCR-8）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAuthorizationRequest {
    pub endpoint: String,
    pub client_id: String,
    pub scope: String,
}

/// 交換の口へ渡す要求の記述（DGRT-7・DGRT-8）。`device_code` は含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceTokenRequest {
    pub endpoint: String,
    pub client_id: String,
    pub grant_type: &'static str,
}

/// RFC 7009 の失効の口へ渡す要求の記述（AGCR-43・OPRV-53）。token は含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRevocationRequest {
    pub endpoint: String,
    pub client_id: String,
    pub token_type_hint: TokenTypeHint,
}

/// 登録の失効の口へ渡す要求の記述（AGCR-35・AGCR-36）。`Debug` は登録 ID を出さない。
#[derive(Clone, PartialEq, Eq)]
pub struct RegistrationRevocationRequest {
    pub endpoint: String,
    pub registration_id: String,
}

impl std::fmt::Debug for RegistrationRevocationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationRevocationRequest")
            .finish_non_exhaustive()
    }
}

/// 口の誤りの語彙（RFC 8628 3.5・RFC 6749・RFC 9449）。未知の値は文字列を持たず `Other` にまとめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthErrorCode {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
    InvalidGrant,
    InvalidDpopProof,
    InvalidClient,
    UnauthorizedClient,
    InvalidRequest,
    Other,
}

impl OAuthErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthorizationPending => "authorization_pending",
            Self::SlowDown => "slow_down",
            Self::AccessDenied => "access_denied",
            Self::ExpiredToken => "expired_token",
            Self::InvalidGrant => "invalid_grant",
            Self::InvalidDpopProof => "invalid_dpop_proof",
            Self::InvalidClient => "invalid_client",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::InvalidRequest => "invalid_request",
            Self::Other => "other",
        }
    }
}

/// 開始の答え。`device_code`・`user_code` は stub を通さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAnswer {
    Started { expires_in: u64, interval: u64 },
    Rejected { status: u16, error: OAuthErrorCode },
}

/// 発行された token の型（AGCR-1・AGCR-2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssuedTokenType {
    DPoP,
    Bearer,
}

/// 発行の答えの値。アクセストークン・refresh の値は持たない。`cnf_jkt` は公開鍵の thumbprint で秘密値ではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedCredential {
    pub token_type: IssuedTokenType,
    pub expires_in: u64,
    pub scope: Vec<String>,
    pub refresh_issued: bool,
    pub cnf_jkt: Option<String>,
}

/// 交換の答え。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenAnswer {
    Issued(IssuedCredential),
    Rejected { status: u16, error: OAuthErrorCode },
}

/// 失効の答え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeAnswer {
    /// RFC 7009 では不明な token も 200（OPRV-53）。
    Revoked { status: u16 },
    Rejected {
        status: u16,
        error: Option<OAuthErrorCode>,
    },
}

/// 相手側の口。呼び出し元の試験が実装する。`Err` は送れなかったことを表す。
/// `device_code`・proof・token は借りるだけで、実装側も保持しないこと（SECR-23〜27）。
pub trait CredentialPorts {
    fn device_authorization(
        &mut self,
        request: &DeviceAuthorizationRequest,
        dpop_proof: Option<&str>,
    ) -> Result<StartAnswer, TransportFailure>;
    fn token(
        &mut self,
        request: &DeviceTokenRequest,
        device_code: &str,
        dpop_proof: Option<&str>,
    ) -> Result<TokenAnswer, TransportFailure>;
    fn revoke_token(
        &mut self,
        request: &TokenRevocationRequest,
        token: &str,
        dpop_proof: Option<&str>,
    ) -> Result<RevokeAnswer, TransportFailure>;
    fn revoke_registration(
        &mut self,
        request: &RegistrationRevocationRequest,
    ) -> Result<RevokeAnswer, TransportFailure>;
}

/// 開始の成功の値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    pub expires_in: u64,
    pub interval: u64,
}

/// 交換の結果。ポーリングの継続は試験側のループで、間隔は数えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOutcome {
    Issued(IssuedCredential),
    Pending,
    SlowDown,
}

/// 成功にならなかったこと。値を含まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialFailure {
    Rejected {
        status: u16,
        error: Option<OAuthErrorCode>,
    },
    /// proof を付けたのに DPoP 結合で発行されなかった（AGCR-1・AGCR-8）。
    BindingMismatch,
    Transport,
}

impl std::fmt::Display for CredentialFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rejected { .. } => "credential call: rejected",
            Self::BindingMismatch => "credential call: dpop binding mismatch",
            Self::Transport => "credential call: transport failure",
        })
    }
}

impl std::error::Error for CredentialFailure {}

/// 発行・失効の呼び出しの入口。呼び出しごとに 1 件記録する。token 等は保持しない。
#[derive(Debug, Clone)]
pub struct ConsoleCredentialCaller {
    client_id: String,
    device_authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    registration_revocation_endpoint: String,
    log: CallLog,
}

impl ConsoleCredentialCaller {
    /// 設定から必要な値だけを写す。
    pub fn new(config: &AgentCredentialConfig) -> Self {
        Self {
            client_id: config.client_id.clone(),
            device_authorization_endpoint: config.device_authorization_endpoint.clone(),
            token_endpoint: config.token_endpoint.clone(),
            revocation_endpoint: config.revocation_endpoint.clone(),
            registration_revocation_endpoint: config.registration_revocation_endpoint.clone(),
            log: CallLog::default(),
        }
    }

    /// これまでの呼び出しの記録。
    pub fn log(&self) -> &CallLog {
        &self.log
    }

    /// Device Authorization の開始（DGRT-1）。
    pub fn start<P: CredentialPorts>(
        &mut self,
        scope: &str,
        dpop_proof: Option<&str>,
        ports: &mut P,
    ) -> Result<Started, CredentialFailure> {
        let req = DeviceAuthorizationRequest {
            endpoint: self.device_authorization_endpoint.clone(),
            client_id: self.client_id.clone(),
            scope: scope.to_owned(),
        };
        match ports.device_authorization(&req, dpop_proof) {
            Err(_) => Err(self.transport(CallKind::DeviceAuthorization)),
            Ok(StartAnswer::Started {
                expires_in,
                interval,
            }) => {
                self.log.push(
                    CallKind::DeviceAuthorization,
                    CallOutcome::Responded { status: 200 },
                );
                Ok(Started {
                    expires_in,
                    interval,
                })
            }
            Ok(StartAnswer::Rejected { status, error }) => {
                Err(self.rejected(CallKind::DeviceAuthorization, status, Some(error)))
            }
        }
    }

    /// `device_code` での交換（DGRT-7・DGRT-8）。proof を付けたのに DPoP 結合でなければ `BindingMismatch`（AGCR-1）。
    pub fn exchange<P: CredentialPorts>(
        &mut self,
        device_code: &str,
        dpop_proof: Option<&str>,
        ports: &mut P,
    ) -> Result<ExchangeOutcome, CredentialFailure> {
        let req = DeviceTokenRequest {
            endpoint: self.token_endpoint.clone(),
            client_id: self.client_id.clone(),
            grant_type: DEVICE_CODE_GRANT_TYPE,
        };
        match ports.token(&req, device_code, dpop_proof) {
            Err(_) => Err(self.transport(CallKind::DeviceToken)),
            Ok(TokenAnswer::Issued(cred)) => {
                self.log.push(
                    CallKind::DeviceToken,
                    CallOutcome::Responded { status: 200 },
                );
                let bound = cred.token_type == IssuedTokenType::DPoP && cred.cnf_jkt.is_some();
                if dpop_proof.is_some() && !bound {
                    return Err(CredentialFailure::BindingMismatch);
                }
                Ok(ExchangeOutcome::Issued(cred))
            }
            Ok(TokenAnswer::Rejected { status, error }) => {
                let f = self.rejected(CallKind::DeviceToken, status, Some(error));
                match error {
                    OAuthErrorCode::AuthorizationPending => Ok(ExchangeOutcome::Pending),
                    OAuthErrorCode::SlowDown => Ok(ExchangeOutcome::SlowDown),
                    _ => Err(f),
                }
            }
        }
    }

    /// エージェント自身の revocation（RFC 7009。AGCR-43・OPRV-53）。`Revoked` のみ成功。
    pub fn revoke_token<P: CredentialPorts>(
        &mut self,
        token: &str,
        hint: TokenTypeHint,
        dpop_proof: Option<&str>,
        ports: &mut P,
    ) -> Result<(), CredentialFailure> {
        let req = TokenRevocationRequest {
            endpoint: self.revocation_endpoint.clone(),
            client_id: self.client_id.clone(),
            token_type_hint: hint,
        };
        let r = ports.revoke_token(&req, token, dpop_proof);
        self.finish_revoke(CallKind::TokenRevocation, r)
    }

    /// エージェントの登録の失効（AGCR-35・AGCR-36）。404 を含む拒否は `Err`。
    pub fn revoke_registration<P: CredentialPorts>(
        &mut self,
        registration_id: &str,
        ports: &mut P,
    ) -> Result<(), CredentialFailure> {
        let req = RegistrationRevocationRequest {
            endpoint: self.registration_revocation_endpoint.clone(),
            registration_id: registration_id.to_owned(),
        };
        let r = ports.revoke_registration(&req);
        self.finish_revoke(CallKind::RegistrationRevocation, r)
    }

    fn finish_revoke(
        &mut self,
        kind: CallKind,
        r: Result<RevokeAnswer, TransportFailure>,
    ) -> Result<(), CredentialFailure> {
        match r {
            Err(_) => Err(self.transport(kind)),
            Ok(RevokeAnswer::Revoked { status }) => {
                self.log.push(kind, CallOutcome::Responded { status });
                Ok(())
            }
            Ok(RevokeAnswer::Rejected { status, error }) => Err(self.rejected(kind, status, error)),
        }
    }

    fn transport(&mut self, kind: CallKind) -> CredentialFailure {
        self.log.push(kind, CallOutcome::TransportFailed);
        CredentialFailure::Transport
    }

    fn rejected(
        &mut self,
        kind: CallKind,
        status: u16,
        error: Option<OAuthErrorCode>,
    ) -> CredentialFailure {
        self.log.push(kind, CallOutcome::Responded { status });
        CredentialFailure::Rejected { status, error }
    }
}

/// 設定どおりに答える口の組。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialScript {
    pub start: StartAnswer,
    pub token: TokenAnswer,
    pub revoke_token: RevokeAnswer,
    pub revoke_registration: RevokeAnswer,
}

/// 設定どおりに応答する [`CredentialPorts`]。借りた値は読まずに捨てる。送れない・遅延の注入は持たず、[`crate::fault`] で包む（#76）。
#[derive(Debug, Clone)]
pub struct ScriptedCredentialPorts {
    script: CredentialScript,
}

impl ScriptedCredentialPorts {
    pub fn new(script: CredentialScript) -> Self {
        Self { script }
    }

    /// 呼び出しと呼び出しの間に答えを切り替える。
    pub fn set_script(&mut self, script: CredentialScript) {
        self.script = script;
    }
}

impl CredentialPorts for ScriptedCredentialPorts {
    fn device_authorization(
        &mut self,
        _request: &DeviceAuthorizationRequest,
        _dpop_proof: Option<&str>,
    ) -> Result<StartAnswer, TransportFailure> {
        Ok(self.script.start)
    }

    fn token(
        &mut self,
        _request: &DeviceTokenRequest,
        _device_code: &str,
        _dpop_proof: Option<&str>,
    ) -> Result<TokenAnswer, TransportFailure> {
        Ok(self.script.token.clone())
    }

    fn revoke_token(
        &mut self,
        _request: &TokenRevocationRequest,
        _token: &str,
        _dpop_proof: Option<&str>,
    ) -> Result<RevokeAnswer, TransportFailure> {
        Ok(self.script.revoke_token)
    }

    fn revoke_registration(
        &mut self,
        _request: &RegistrationRevocationRequest,
    ) -> Result<RevokeAnswer, TransportFailure> {
        Ok(self.script.revoke_registration)
    }
}
