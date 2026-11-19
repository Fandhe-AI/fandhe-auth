//! console の配送の時点の確認の模擬（TASK-8・#74）。試験専用で、実物の console ではない。
//!
//! 役割: console は配送のたびに、現在の状態を読む 3 つの確認を最初から行う（ADLG-15）。
//! ① エージェントの資格情報（`fandhe-auth` への introspection。[`crate::introspection`] を再利用）、
//! ② 決めた本人・委任の記録の有効性の照会（ADLG-29）、③ Hub への現在の有効性の問い合わせ（ADLG-31）。
//! 呼び出し元は試験で、相手側の応答は [`DeliveryPorts`] で返す。入口は [`ConsoleDeliveryChecker::check`] の 1 つ。
//!
//! 設計上の約束:
//! - sans-IO・同期。時刻は引数 `now`（UNIX 秒）で受け、環境変数・ファイル・ネットワークを読まない（DPOL-32）。
//! - 配送のたびに ①②③ を 1 回ずつ呼ぶ。過去の結果を保持・再利用しない（ADLG-15）。①が無効でも ②③ を省かない
//!   （ADLG-16 の構成 B は、現在の状態を読む確認を最後にまとめて行う）。
//! - 成功になる経路は「3 つとも有効」の 1 つだけ。どの口の失敗・送れないことも成功にしない（AVLB-18・AVLB-22 の向き）。
//! - ②③ の応答は有効か無効かだけを受け、理由・申告値を判定に使わない（ADLG-29・ADLG-31・D-2026-10-17-49 の (3)）。
//! - 切り替えられるのは相手側の応答の値だけ。確認を省く設定・分岐は作らない（DPOL-30・DPOL-32）。
//! - token は引数で借りて ① の口へ渡すだけで、構造体・誤り・出力に保持しない（SECR-23〜27）。
//! - 判定しないもの: 静的な確認（署名・DPoP の proof・ローカルの `exp`。呼び出し元が済ませた前提。#79）、
//!   遅延・拒否の注入（#76）、呼び出しの記録（#75）、窓（`t_done − t_commit`）の計測と上限（TASK-451・TASK-30）。

use crate::config::ConsoleStubConfig;
use crate::introspection::{
    AuthorityRequirement, ConsoleIntrospector, CurrentAuthority, IntrospectionFailure,
    IntrospectionHttpResponse, IntrospectionRequest, TransportFailure,
};

// TODO(TASK-8): ② ③ の口の本文の形は Hub が正本（Hub ARUN-60 等）。抽象の型で受けた点を #77 で仮定の契約として印を付け、
// 静的な確認を持たない点・① の拒否理由を 1 種類にまとめた点・③ の口の URL を設定に持たない点を #79 に書く。

/// 有効性の照会の応答。理由を持たない（ADLG-29・ADLG-31）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidityAnswer {
    Valid,
    Invalid,
}

/// ② の照会の対象。人の回答は回答者の認証セッション、自動回答は委任に結び付いた登録 ID（D-2026-11-05-2・D-2026-11-05-3）。
#[derive(Clone, PartialEq, Eq)]
pub enum ValidityTarget {
    Session { sid: String },
    Registration { registration_id: String },
}

impl std::fmt::Debug for ValidityTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // sid・登録 ID は出力に残さず、種別だけを出す。
        f.write_str(match self {
            Self::Session { .. } => "ValidityTarget::Session",
            Self::Registration { .. } => "ValidityTarget::Registration",
        })
    }
}

/// ② の口へ渡す要求の記述（ADLG-29）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidityRequest {
    /// 有効性の照会の口の URL（設定の値。取得はしない）。
    pub endpoint: String,
    pub client_id: String,
    pub target: ValidityTarget,
}

/// ③ の口へ渡す要求の記述（ADLG-31）。口の URL・本文の形は Hub が正本のため持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubValidityRequest {
    /// Hub 側の委任の参照を表す不透明な文字列。
    pub delegation_ref: String,
}

/// 相手側の 3 つの口。呼び出し元の試験が実装する。`Err` は送れなかった・応答が得られなかったことを表す。
pub trait DeliveryPorts {
    /// ① introspection（ADLG-15 ①）。
    fn introspect(
        &mut self,
        request: &IntrospectionRequest,
        token: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure>;
    /// ② 有効性の照会（ADLG-29）。
    fn query_validity(
        &mut self,
        request: &ValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure>;
    /// ③ Hub への問い合わせ（ADLG-31）。
    fn query_hub(
        &mut self,
        request: &HubValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure>;
}

/// 配送ごとに試験が渡す文脈。token は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryContext {
    pub requirement: AuthorityRequirement,
    pub decider: ValidityTarget,
    pub delegation_ref: String,
}

/// 3 つの確認の結果。成功は全て有効のときだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryDecision {
    /// `checked_at` は確認の応答の時点（ADLG-24 の取り決め。呼び出し元の `now`）。
    Delivered {
        checked_at: u64,
    },
    Rejected(DeliveryRejection),
}

/// 拒否の理由。①②③ の順で最初に通らなかったものを返す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryRejection {
    /// ① が無効（ADLG-11）。scope・`cnf.jkt` の不一致も含む（OPRV-54 で理由を持たない）。
    IntrospectInactive,
    /// ② が無効（ADLG-12）。
    DeciderSessionInactive,
    /// ③ が無効（ADLG-31。理由は 1 種類）。
    HubInvalid,
}

impl DeliveryRejection {
    /// PoC-11 の console の語彙。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IntrospectInactive => "introspect_inactive",
            Self::DeciderSessionInactive => "decider_session_inactive",
            Self::HubInvalid => "hub_invalid",
        }
    }
}

/// 確認できなかったこと。どれも成功にならない（AVLB-18・AVLB-22 の向き）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryFailure {
    Introspection(IntrospectionFailure),
    DeciderValidityUnavailable,
    HubValidityUnavailable,
}

impl std::fmt::Display for DeliveryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Introspection(e) => write!(f, "delivery check: {e}"),
            Self::DeciderValidityUnavailable => {
                f.write_str("delivery check: decider validity unavailable")
            }
            Self::HubValidityUnavailable => f.write_str("delivery check: hub validity unavailable"),
        }
    }
}

impl std::error::Error for DeliveryFailure {}

/// 配送の時点の確認の入口。確認の結果を保持しない（キャッシュ 0。ADLG-15）。
#[derive(Debug, Clone)]
pub struct ConsoleDeliveryChecker {
    introspector: ConsoleIntrospector,
    validity_endpoint: String,
    client_id: String,
}

impl ConsoleDeliveryChecker {
    /// 設定から必要な値だけを写す。
    pub fn new(config: &ConsoleStubConfig) -> Self {
        Self {
            introspector: ConsoleIntrospector::new(config),
            validity_endpoint: config.validity_endpoint.clone(),
            client_id: config.client_id.clone(),
        }
    }

    /// 配送の時点の確認の唯一の入口。①②③ を毎回 1 回ずつ呼び、全て有効のときだけ配送の成功とする（ADLG-15）。
    /// `agent_token` は ① の口へ渡すだけで保持しない。
    pub fn check<P: DeliveryPorts>(
        &self,
        agent_token: &str,
        ctx: &DeliveryContext,
        now: u64,
        ports: &mut P,
    ) -> Result<DeliveryDecision, DeliveryFailure> {
        // 構成 B（ADLG-16）: 3 つを最後にまとめて行う。先の失敗・無効でも後ろの呼び出しを省かない。
        let agent = self
            .introspector
            .check(agent_token, &ctx.requirement, now, |req, tok| {
                ports.introspect(req, tok)
            });
        let decider = ports.query_validity(&ValidityRequest {
            endpoint: self.validity_endpoint.clone(),
            client_id: self.client_id.clone(),
            target: ctx.decider.clone(),
        });
        let hub = ports.query_hub(&HubValidityRequest {
            delegation_ref: ctx.delegation_ref.clone(),
        });

        // ①②③ の順で、最初に通らなかったものが結果を決める。
        let agent = agent.map_err(DeliveryFailure::Introspection)?;
        if matches!(agent, CurrentAuthority::Invalid) {
            return Ok(DeliveryDecision::Rejected(
                DeliveryRejection::IntrospectInactive,
            ));
        }
        match decider.map_err(|_| DeliveryFailure::DeciderValidityUnavailable)? {
            ValidityAnswer::Valid => {}
            ValidityAnswer::Invalid => {
                return Ok(DeliveryDecision::Rejected(
                    DeliveryRejection::DeciderSessionInactive,
                ));
            }
        }
        match hub.map_err(|_| DeliveryFailure::HubValidityUnavailable)? {
            ValidityAnswer::Valid => {}
            ValidityAnswer::Invalid => {
                return Ok(DeliveryDecision::Rejected(DeliveryRejection::HubInvalid));
            }
        }
        Ok(DeliveryDecision::Delivered { checked_at: now })
    }
}

/// ① の相手側の応答の設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCredentialScript {
    Active {
        scope: String,
        jkt: Option<String>,
        exp: u64,
    },
    Inactive,
}

/// 相手側の応答（有効／無効）の設定。コードを変えずに結果を切り替える（#74 の受け入れ条件）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryScript {
    pub agent: AgentCredentialScript,
    pub decider: ValidityAnswer,
    pub hub: ValidityAnswer,
}

/// 口ごとの呼び出し回数。要求の中身・token は保持しない（記録は #75）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallCounts {
    pub introspect: u32,
    pub decider: u32,
    pub hub: u32,
}

/// 設定どおりに応答する [`DeliveryPorts`]。
#[derive(Debug, Clone)]
pub struct ScriptedDeliveryPorts {
    script: DeliveryScript,
    calls: CallCounts,
}

impl ScriptedDeliveryPorts {
    pub fn new(script: DeliveryScript) -> Self {
        Self {
            script,
            calls: CallCounts::default(),
        }
    }

    /// 配送と配送の間に結果を切り替える（ADLG-15 の「事象の後の配送」を試す）。
    pub fn set_script(&mut self, script: DeliveryScript) {
        self.script = script;
    }

    pub fn calls(&self) -> CallCounts {
        self.calls
    }
}

impl DeliveryPorts for ScriptedDeliveryPorts {
    fn introspect(
        &mut self,
        _request: &IntrospectionRequest,
        _token: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure> {
        self.calls.introspect += 1;
        // JSON は serde_json で組み立てる（文字列の連結はしない）。
        let body = match &self.script.agent {
            AgentCredentialScript::Inactive => serde_json::json!({ "active": false }),
            AgentCredentialScript::Active { scope, jkt, exp } => {
                let mut v = serde_json::json!({ "active": true, "scope": scope, "exp": exp });
                if let (Some(jkt), Some(obj)) = (jkt, v.as_object_mut()) {
                    obj.insert("token_type".to_owned(), "DPoP".into());
                    obj.insert("cnf".to_owned(), serde_json::json!({ "jkt": jkt }));
                }
                v
            }
        };
        Ok(IntrospectionHttpResponse {
            status: 200,
            body: body.to_string().into_bytes(),
        })
    }

    fn query_validity(
        &mut self,
        _request: &ValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        self.calls.decider += 1;
        Ok(self.script.decider)
    }

    fn query_hub(
        &mut self,
        _request: &HubValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        self.calls.hub += 1;
        Ok(self.script.hub)
    }
}
