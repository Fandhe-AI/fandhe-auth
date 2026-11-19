//! ハーネスの console の stub 構成要素（`crates/console-stub` を in-process で起動・停止する。TASK-10 (4/6)・TASK-8）。
//!
//! 役割: 結合試験が console の stub の模擬一式を [`Component`] として他の構成要素と同じ入口（[`crate::harness::Harness`]）
//! で起動・停止し、起動前に取った [`ConsoleStubHandle`] を通して呼べるようにする。呼び出し元は結合試験だけ。
//! 使い方は [`crate::hub_stub`] と同じ（Box に入れる前にハンドルを取る）。
//!
//! 「起動」と「つながる」の意味: stub は sans-IO・同期の模擬で、ソケット・スレッド・子プロセスを持たない
//! （`docs/stubs/console-stub.md`）。起動は設定から模擬一式を組んでハンドルに置くことで、試験が起動中のハンドルから
//! 模擬を呼び応答を受けることが「つながる」にあたる。console の stub は `fandhe-auth` を呼ぶ側の模擬なので、
//! 相手の口（`DeliveryPorts`・`CredentialPorts`）は試験が渡す Scripted の偽物で、セッションには含めない。
//! 未達: 最小の組込み先（TASK-716）から stub への接続は Host の実体が無いので示せない。
//! Hub の stub との結線もしない（Hub の stub に ADLG-31 ③ の口が無い）。試験内の Fake の Host は達成に数えない（ACPT-7）。
//!
//! 安全: 環境変数・ファイル・ネットワークを使わない（DPOL-32）。stub を飛ばす・検証を外す分岐は作らない
//! （DPOL-30〜32）。`Debug` は起動中かだけを出し、設定の値（URL・`client_id`）を出さない（SECR-11・SECR-23）。
//! token は呼び出しごとに借りるだけで、セッションに保持しない（SECR-23〜27）。
//! stub の合格を実物の console の合格に数えない（A-2026-10-28-9・ACPT-7）。障害注入の組み込みは TASK-13。

use std::fmt;
use std::sync::{Arc, Mutex};

use fandhe_auth_console_stub::config::{AgentCredentialConfig, ConsoleStubConfig};
use fandhe_auth_console_stub::contract::{ContractRecord, contract_record};
use fandhe_auth_console_stub::credential::ConsoleCredentialCaller;
use fandhe_auth_console_stub::delivery::ConsoleDeliveryChecker;
use fandhe_auth_console_stub::introspection::ConsoleIntrospector;

use crate::component::{Component, ComponentKind};
use crate::harness::{HarnessError, HarnessErrorKind};

/// 起動中の console の stub の模擬一式。設定だけで組める 3 つを持つ。
///
/// 相手側の口の偽物・障害注入は含めない。試験が自分で組んで渡す。
pub struct ConsoleStubSession {
    config: ConsoleStubConfig,
    credential_config: AgentCredentialConfig,
    /// introspection の呼び出しの模擬。
    pub introspector: ConsoleIntrospector,
    /// 配送の時点の確認の模擬。
    pub delivery: ConsoleDeliveryChecker,
    /// エージェントの資格情報の発行・失効の呼び出しの模擬。記録はセッションの寿命の間だけ持つ。
    pub credential: ConsoleCredentialCaller,
}

impl ConsoleStubSession {
    fn new(config: ConsoleStubConfig, credential_config: AgentCredentialConfig) -> Self {
        Self {
            introspector: ConsoleIntrospector::new(&config),
            delivery: ConsoleDeliveryChecker::new(&config),
            credential: ConsoleCredentialCaller::new(&credential_config),
            config,
            credential_config,
        }
    }

    /// 起動時の設定。
    pub fn config(&self) -> &ConsoleStubConfig {
        &self.config
    }

    /// 起動時の資格情報の呼び出しの設定。
    pub fn credential_config(&self) -> &AgentCredentialConfig {
        &self.credential_config
    }
}

impl fmt::Debug for ConsoleStubSession {
    /// 種類だけを出し、設定の値を出さない（SECR-11・SECR-23）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConsoleStubSession")
    }
}

/// 起動中の模擬一式への共有ハンドル。起動前と停止後は何も返さない。
#[derive(Clone, Default)]
pub struct ConsoleStubHandle(Arc<Mutex<Option<ConsoleStubSession>>>);

impl ConsoleStubHandle {
    /// 起動中だけ `Some(f の戻り値)`。起動前・停止後・lock の毒化は `None`。
    pub fn with<R>(&self, f: impl FnOnce(&mut ConsoleStubSession) -> R) -> Option<R> {
        self.0.lock().ok()?.as_mut().map(f)
    }

    /// 起動中か。
    pub fn is_running(&self) -> bool {
        self.0.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// 契約が仮定である旨の記録（A-2026-10-28-12）。起動中だけ値がある。設定の値は載せない。
    pub fn contract_record(&self) -> Option<ContractRecord> {
        self.with(|_| contract_record())
    }

    fn set(&self, session: Option<ConsoleStubSession>) -> bool {
        match self.0.lock() {
            Ok(mut g) => {
                *g = session;
                true
            }
            Err(_) => false,
        }
    }
}

impl fmt::Debug for ConsoleStubHandle {
    /// 起動中かだけを出す（SECR-11・SECR-23）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsoleStubHandle")
            .field("running", &self.is_running())
            .finish()
    }
}

/// console の stub の構成要素。設定は 2 つとも必須で、既定値を持たない（A-2026-10-28-14）。
pub struct ConsoleStubComponent {
    config: ConsoleStubConfig,
    credential_config: AgentCredentialConfig,
    handle: ConsoleStubHandle,
}

impl ConsoleStubComponent {
    /// 設定を渡して作る。
    pub fn new(config: ConsoleStubConfig, credential_config: AgentCredentialConfig) -> Self {
        Self {
            config,
            credential_config,
            handle: ConsoleStubHandle::default(),
        }
    }

    /// 共有ハンドル。`Box<dyn Component>` に入れる前に試験が取る。
    pub fn handle(&self) -> ConsoleStubHandle {
        self.handle.clone()
    }

    /// 契約が仮定である旨の記録。起動前でも取れる。
    pub fn contract_record(&self) -> ContractRecord {
        contract_record()
    }
}

impl fmt::Debug for ConsoleStubComponent {
    /// 設定の値を出さない（SECR-11・SECR-23）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsoleStubComponent")
            .field("running", &self.handle.is_running())
            .finish()
    }
}

impl Component for ConsoleStubComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::ConsoleStub
    }

    /// 模擬一式を組んでハンドルに置く。既に起動中なら失敗する。
    fn start(&mut self) -> Result<(), HarnessError> {
        let fail = HarnessError::new(HarnessErrorKind::StartFailed, ComponentKind::ConsoleStub);
        if self.handle.is_running() {
            return Err(fail);
        }
        if self.handle.set(Some(ConsoleStubSession::new(
            self.config.clone(),
            self.credential_config.clone(),
        ))) {
            Ok(())
        } else {
            Err(fail)
        }
    }

    /// ハンドルを空に戻す。何度呼んでも成功する。
    fn stop(&mut self) -> Result<(), HarnessError> {
        if self.handle.set(None) {
            Ok(())
        } else {
            Err(HarnessError::new(
                HarnessErrorKind::StopFailed,
                ComponentKind::ConsoleStub,
            ))
        }
    }
}
