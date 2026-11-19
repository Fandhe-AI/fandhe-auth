//! 応答の遅れと拒否の注入（TASK-8・#76）。試験専用で、実物の console ではない。
//!
//! 役割: 障害注入のハーネス（TASK-13）が、相手側の口（[`DeliveryPorts`]・[`CredentialPorts`]）の応答に
//! 遅れ・送れない・拒否を設定で入れるための部品。任意の口の実装を包み、同じ trait を実装する。
//! 判定の入口（`ConsoleDeliveryChecker`・`ConsoleCredentialCaller`）のコードは変えない。
//! 単独の `ConsoleIntrospector::check` へは、ラッパの `introspect` を呼ぶクロージャで渡す。
//!
//! 設計上の約束:
//! - sans-IO・同期。stub は sleep せず時刻も読まない（DPOL-32）。注入した遅れの `Duration` は、呼び出し元が渡す
//!   [`Wait`] のフックへ渡すだけ。実際に待つか偽の時計を進めるかは試験・ハーネス（TASK-13）が決める。
//! - 遅れの値は試験用の仮の値で、要件の値ではない。stub での測定を数値の根拠にしない（TASK-30・THRT-29・A-2026-10-28-14）。
//! - 注入で起こせるのは失敗の向きだけ。成功を作る経路・確認を省く経路は無い（AVLB-18・AVLB-22・DPOL-30〜32）。
//!   ①へ拒否を注入しても ②③ は呼ばれる（ADLG-15・ADLG-16）。
//! - 規則は口ごとの呼び出し回数（1 始まり、通算）の範囲で当てる。最初に当たった 1 件だけを使う。
//! - 注入の記録（[`InjectionLog`]）は全ての呼び出しで 1 件ずつ積む。注入なしは `injected: None` で、基準線も同じ形になる。
//!   記録は種別・回数・遅れ・状態コードだけで、値・token・URL・識別子を持たない（SECR-23〜27）。時刻は記録しない。
//!   注入の時刻をハーネスの時計で付けるのは TASK-13 の責務。
//! - 借りた token・`device_code`・proof は内側へ渡すだけで、ラッパ・記録・`Debug` に残さない（SECR-23〜27）。
//! - 重ね方: `RecordingDeliveryPorts<FaultInjectingDeliveryPorts<..>>` の順にすると、注入後の結果が `CallLog` に載る。

use std::time::Duration;

use crate::config::{
    CredentialFaultConfig, CredentialRefusal, DeliveryFaultConfig, FaultRule, IntrospectionRefusal,
    ValidityRefusal,
};
use crate::credential::{
    CredentialPorts, DeviceAuthorizationRequest, DeviceTokenRequest, RegistrationRevocationRequest,
    RevokeAnswer, StartAnswer, TokenAnswer, TokenRevocationRequest,
};
use crate::delivery::{DeliveryPorts, HubValidityRequest, ValidityAnswer, ValidityRequest};
use crate::introspection::{IntrospectionHttpResponse, IntrospectionRequest, TransportFailure};
use crate::record::CallKind;

/// 注入した遅れを受けるフック。待つ・偽の時計を進める等は実装側（試験・TASK-13）が決める。
pub trait Wait {
    fn wait(&mut self, delay: Duration);
}

impl<F: FnMut(Duration)> Wait for F {
    fn wait(&mut self, delay: Duration) {
        self(delay)
    }
}

/// 記録する拒否の種類。資格情報の `Rejected` は状態コードだけを残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalRecord {
    Unreachable,
    Status(u16),
}

/// 注入した内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectedFault {
    pub delay: Option<Duration>,
    pub refusal: Option<RefusalRecord>,
}

/// 1 件の記録。`call_index` は口ごとの通算（1 始まり）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionRecord {
    pub kind: CallKind,
    pub call_index: u32,
    pub injected: Option<InjectedFault>,
}

/// 呼んだ順の注入の記録。追加は crate 内だけで、外は読むだけ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InjectionLog {
    records: Vec<InjectionRecord>,
}

impl InjectionLog {
    pub fn records(&self) -> &[InjectionRecord] {
        &self.records
    }
}

/// 口ごとの通算の回数。
#[derive(Debug, Clone, Copy, Default)]
struct Counter(u32);

impl Counter {
    fn next(&mut self) -> u32 {
        self.0 = self.0.saturating_add(1);
        self.0
    }
}

/// 先頭から見て最初に当たった規則。
fn pick<R: Clone>(rules: &[FaultRule<R>], n: u32) -> Option<FaultRule<R>> {
    rules.iter().find(|r| r.calls.contains(n)).cloned()
}

fn push(log: &mut InjectionLog, kind: CallKind, n: u32, injected: Option<InjectedFault>) {
    log.records.push(InjectionRecord {
        kind,
        call_index: n,
        injected,
    });
}

fn introspection_injected(rule: &Option<FaultRule<IntrospectionRefusal>>) -> Option<InjectedFault> {
    rule.as_ref().map(|r| InjectedFault {
        delay: r.delay,
        refusal: r.refusal.map(|x| match x {
            IntrospectionRefusal::Unreachable => RefusalRecord::Unreachable,
            IntrospectionRefusal::Status(s) => RefusalRecord::Status(s),
        }),
    })
}

fn validity_injected(rule: &Option<FaultRule<ValidityRefusal>>) -> Option<InjectedFault> {
    rule.as_ref().map(|r| InjectedFault {
        delay: r.delay,
        refusal: r.refusal.map(|_| RefusalRecord::Unreachable),
    })
}

fn credential_injected(rule: &Option<FaultRule<CredentialRefusal>>) -> Option<InjectedFault> {
    rule.as_ref().map(|r| InjectedFault {
        delay: r.delay,
        refusal: r.refusal.map(|x| match x {
            CredentialRefusal::Unreachable => RefusalRecord::Unreachable,
            CredentialRefusal::Rejected { status, .. } => RefusalRecord::Status(status),
        }),
    })
}

/// 配送の時点の確認の口を包んで注入する。
pub struct FaultInjectingDeliveryPorts<P: DeliveryPorts, W: Wait> {
    inner: P,
    wait: W,
    faults: DeliveryFaultConfig,
    n_introspect: Counter,
    n_decider: Counter,
    n_hub: Counter,
    log: InjectionLog,
}

impl<P: DeliveryPorts, W: Wait> std::fmt::Debug for FaultInjectingDeliveryPorts<P, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaultInjectingDeliveryPorts")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl<P: DeliveryPorts, W: Wait> FaultInjectingDeliveryPorts<P, W> {
    pub fn new(inner: P, faults: DeliveryFaultConfig, wait: W) -> Self {
        Self {
            inner,
            wait,
            faults,
            n_introspect: Counter::default(),
            n_decider: Counter::default(),
            n_hub: Counter::default(),
            log: InjectionLog::default(),
        }
    }

    /// 呼び出しの間に設定を切り替える。回数は通算のまま戻らない。
    pub fn set_faults(&mut self, faults: DeliveryFaultConfig) {
        self.faults = faults;
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }

    pub fn into_inner(self) -> P {
        self.inner
    }

    pub fn log(&self) -> &InjectionLog {
        &self.log
    }
}

impl<P: DeliveryPorts, W: Wait> DeliveryPorts for FaultInjectingDeliveryPorts<P, W> {
    fn introspect(
        &mut self,
        request: &IntrospectionRequest,
        token: &str,
    ) -> Result<IntrospectionHttpResponse, TransportFailure> {
        let n = self.n_introspect.next();
        let rule = pick(&self.faults.introspection, n);
        push(
            &mut self.log,
            CallKind::Introspection,
            n,
            introspection_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.introspect(request, token);
        };
        match rule.refusal {
            None => {
                let r = self.inner.introspect(request, token);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(refusal) => {
                wait_for(&mut self.wait, rule.delay);
                match refusal {
                    IntrospectionRefusal::Unreachable => Err(TransportFailure),
                    IntrospectionRefusal::Status(status) => Ok(IntrospectionHttpResponse {
                        status,
                        body: Vec::new(),
                    }),
                }
            }
        }
    }

    fn query_validity(
        &mut self,
        request: &ValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        let n = self.n_decider.next();
        let rule = pick(&self.faults.decider, n);
        push(
            &mut self.log,
            CallKind::DeciderValidity,
            n,
            validity_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.query_validity(request);
        };
        match rule.refusal {
            None => {
                let r = self.inner.query_validity(request);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(ValidityRefusal::Unreachable) => {
                wait_for(&mut self.wait, rule.delay);
                Err(TransportFailure)
            }
        }
    }

    fn query_hub(
        &mut self,
        request: &HubValidityRequest,
    ) -> Result<ValidityAnswer, TransportFailure> {
        let n = self.n_hub.next();
        let rule = pick(&self.faults.hub, n);
        push(
            &mut self.log,
            CallKind::HubValidity,
            n,
            validity_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.query_hub(request);
        };
        match rule.refusal {
            None => {
                let r = self.inner.query_hub(request);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(ValidityRefusal::Unreachable) => {
                wait_for(&mut self.wait, rule.delay);
                Err(TransportFailure)
            }
        }
    }
}

fn wait_for<W: Wait>(wait: &mut W, delay: Option<Duration>) {
    if let Some(d) = delay {
        wait.wait(d);
    }
}

/// 資格情報の口を包んで注入する。
pub struct FaultInjectingCredentialPorts<P: CredentialPorts, W: Wait> {
    inner: P,
    wait: W,
    faults: CredentialFaultConfig,
    n_start: Counter,
    n_token: Counter,
    n_revoke_token: Counter,
    n_revoke_registration: Counter,
    log: InjectionLog,
}

impl<P: CredentialPorts, W: Wait> std::fmt::Debug for FaultInjectingCredentialPorts<P, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaultInjectingCredentialPorts")
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

impl<P: CredentialPorts, W: Wait> FaultInjectingCredentialPorts<P, W> {
    pub fn new(inner: P, faults: CredentialFaultConfig, wait: W) -> Self {
        Self {
            inner,
            wait,
            faults,
            n_start: Counter::default(),
            n_token: Counter::default(),
            n_revoke_token: Counter::default(),
            n_revoke_registration: Counter::default(),
            log: InjectionLog::default(),
        }
    }

    /// 呼び出しの間に設定を切り替える。回数は通算のまま戻らない。
    pub fn set_faults(&mut self, faults: CredentialFaultConfig) {
        self.faults = faults;
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }

    pub fn into_inner(self) -> P {
        self.inner
    }

    pub fn log(&self) -> &InjectionLog {
        &self.log
    }
}

impl<P: CredentialPorts, W: Wait> CredentialPorts for FaultInjectingCredentialPorts<P, W> {
    fn device_authorization(
        &mut self,
        request: &DeviceAuthorizationRequest,
        dpop_proof: Option<&str>,
    ) -> Result<StartAnswer, TransportFailure> {
        let n = self.n_start.next();
        let rule = pick(&self.faults.device_authorization, n);
        push(
            &mut self.log,
            CallKind::DeviceAuthorization,
            n,
            credential_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.device_authorization(request, dpop_proof);
        };
        match rule.refusal {
            None => {
                let r = self.inner.device_authorization(request, dpop_proof);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(refusal) => {
                wait_for(&mut self.wait, rule.delay);
                match refusal {
                    CredentialRefusal::Unreachable => Err(TransportFailure),
                    CredentialRefusal::Rejected { status, error } => {
                        Ok(StartAnswer::Rejected { status, error })
                    }
                }
            }
        }
    }

    fn token(
        &mut self,
        request: &DeviceTokenRequest,
        device_code: &str,
        dpop_proof: Option<&str>,
    ) -> Result<TokenAnswer, TransportFailure> {
        let n = self.n_token.next();
        let rule = pick(&self.faults.token, n);
        push(
            &mut self.log,
            CallKind::DeviceToken,
            n,
            credential_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.token(request, device_code, dpop_proof);
        };
        match rule.refusal {
            None => {
                let r = self.inner.token(request, device_code, dpop_proof);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(refusal) => {
                wait_for(&mut self.wait, rule.delay);
                match refusal {
                    CredentialRefusal::Unreachable => Err(TransportFailure),
                    CredentialRefusal::Rejected { status, error } => {
                        Ok(TokenAnswer::Rejected { status, error })
                    }
                }
            }
        }
    }

    fn revoke_token(
        &mut self,
        request: &TokenRevocationRequest,
        token: &str,
        dpop_proof: Option<&str>,
    ) -> Result<RevokeAnswer, TransportFailure> {
        let n = self.n_revoke_token.next();
        let rule = pick(&self.faults.revoke_token, n);
        push(
            &mut self.log,
            CallKind::TokenRevocation,
            n,
            credential_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.revoke_token(request, token, dpop_proof);
        };
        match rule.refusal {
            None => {
                let r = self.inner.revoke_token(request, token, dpop_proof);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(refusal) => {
                wait_for(&mut self.wait, rule.delay);
                revoke_refusal(refusal)
            }
        }
    }

    fn revoke_registration(
        &mut self,
        request: &RegistrationRevocationRequest,
    ) -> Result<RevokeAnswer, TransportFailure> {
        let n = self.n_revoke_registration.next();
        let rule = pick(&self.faults.revoke_registration, n);
        push(
            &mut self.log,
            CallKind::RegistrationRevocation,
            n,
            credential_injected(&rule),
        );
        let Some(rule) = rule else {
            return self.inner.revoke_registration(request);
        };
        match rule.refusal {
            None => {
                let r = self.inner.revoke_registration(request);
                wait_for(&mut self.wait, rule.delay);
                r
            }
            Some(refusal) => {
                wait_for(&mut self.wait, rule.delay);
                revoke_refusal(refusal)
            }
        }
    }
}

/// 失効の口への拒否。`error` は `Some` に写す。
fn revoke_refusal(refusal: CredentialRefusal) -> Result<RevokeAnswer, TransportFailure> {
    match refusal {
        CredentialRefusal::Unreachable => Err(TransportFailure),
        CredentialRefusal::Rejected { status, error } => Ok(RevokeAnswer::Rejected {
            status,
            error: Some(error),
        }),
    }
}
