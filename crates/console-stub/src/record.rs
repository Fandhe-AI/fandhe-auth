//! console の stub が相手側へ行った呼び出しの記録（TASK-8・#75）。試験専用で、実物の console ではない。
//!
//! 役割: 統合試験が「何をどの順で呼び、どう応答されたか」を確かめる拠り所（PoC-11 手順 3、ADLG-11・ADLG-15）。
//! 呼び出し元は試験。[`crate::credential::ConsoleCredentialCaller`] が発行・失効の呼び出しを積み、
//! [`RecordingDeliveryPorts`] が配送の時点の確認（[`crate::delivery`]）の口を包んで積む。
//!
//! 設計上の約束:
//! - 記録は種別と結果（状態コード／有効・無効／送れなかった）だけ。token・`device_code`・DPoP の proof・
//!   `sid`・登録 ID・URL は持たない（SECR-23〜27）。
//! - 失敗・送れなかった呼び出しも必ず記録する。記録を外す設定・feature は作らない（DPOL-30〜32）。
//! - 順序は呼んだ順。記録の追加は crate 内だけで、試験は読むだけ。

use crate::delivery::{DeliveryPorts, HubValidityRequest, ValidityAnswer, ValidityRequest};
use crate::introspection::{IntrospectionHttpResponse, IntrospectionRequest, TransportFailure};

/// 呼び出しの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// Device Authorization の開始（DGRT-1）。
    DeviceAuthorization,
    /// `device_code` grant の交換（DGRT-7・DGRT-8）。
    DeviceToken,
    /// RFC 7009 の失効（AGCR-43・OPRV-53）。
    TokenRevocation,
    /// エージェントの登録の失効（AGCR-35・AGCR-36）。
    RegistrationRevocation,
    /// ① introspection（ADLG-15 ①）。
    Introspection,
    /// ② 決めた本人・委任の有効性の照会（ADLG-29）。
    DeciderValidity,
    /// ③ Hub への有効性の問い合わせ（ADLG-31）。
    HubValidity,
}

impl CallKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeviceAuthorization => "device_authorization",
            Self::DeviceToken => "device_token",
            Self::TokenRevocation => "token_revocation",
            Self::RegistrationRevocation => "registration_revocation",
            Self::Introspection => "introspection",
            Self::DeciderValidity => "decider_validity",
            Self::HubValidity => "hub_validity",
        }
    }
}

/// 呼び出しの結果。理由・本文は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    /// 状態コードを伴う応答。
    Responded { status: u16 },
    /// 有効性の照会の答え（②③）。
    Answered(ValidityAnswer),
    /// 送れなかった・応答が得られなかった。
    TransportFailed,
}

/// 1 件の記録。秘密値・識別子を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallRecord {
    pub kind: CallKind,
    pub outcome: CallOutcome,
}

/// 呼んだ順の記録。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallLog {
    records: Vec<CallRecord>,
}

impl CallLog {
    pub(crate) fn push(&mut self, kind: CallKind, outcome: CallOutcome) {
        self.records.push(CallRecord { kind, outcome });
    }

    /// 呼んだ順の全記録。
    pub fn records(&self) -> &[CallRecord] {
        &self.records
    }

    /// 種別ごとの件数。
    pub fn count(&self, kind: CallKind) -> usize {
        self.records.iter().filter(|r| r.kind == kind).count()
    }
}

/// 任意の [`DeliveryPorts`] を包み、呼び出しを記録して内側へ転送する。内側の振る舞いは変えない。
#[derive(Debug, Clone)]
pub struct RecordingDeliveryPorts<P: DeliveryPorts> {
    inner: P,
    log: CallLog,
}

impl<P: DeliveryPorts> RecordingDeliveryPorts<P> {
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            log: CallLog::default(),
        }
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }

    pub fn into_inner(self) -> P {
        self.inner
    }

    pub fn log(&self) -> &CallLog {
        &self.log
    }
}

impl<P: DeliveryPorts> DeliveryPorts for RecordingDeliveryPorts<P> {
    fn introspect(
        &mut self,
        request: &IntrospectionRequest,
        token: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure> {
        let r = self.inner.introspect(request, token);
        let outcome = match &r {
            Ok(resp) => CallOutcome::Responded {
                status: resp.status,
            },
            Err(_) => CallOutcome::TransportFailed,
        };
        self.log.push(CallKind::Introspection, outcome);
        r
    }

    fn query_validity(
        &mut self,
        request: &ValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        let r = self.inner.query_validity(request);
        self.log.push(CallKind::DeciderValidity, answer_outcome(&r));
        r
    }

    fn query_hub(
        &mut self,
        request: &HubValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        let r = self.inner.query_hub(request);
        self.log.push(CallKind::HubValidity, answer_outcome(&r));
        r
    }
}

fn answer_outcome(r: &Result<ValidityAnswer, TransportFailure>) -> CallOutcome {
    match r {
        Ok(a) => CallOutcome::Answered(*a),
        Err(_) => CallOutcome::TransportFailed,
    }
}
