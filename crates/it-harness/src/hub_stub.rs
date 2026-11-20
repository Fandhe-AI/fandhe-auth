//! ハーネスの Hub の stub 構成要素（`crates/hub-stub` を in-process で起動・停止する。TASK-10 (3/6)・TASK-7）。
//!
//! 役割: 結合試験が Hub の stub の模擬一式を [`Component`] として他の構成要素と同じ入口（[`crate::harness::Harness`]）
//! で起動・停止し、起動前に取った [`HubStubHandle`] を通して呼べるようにする。呼び出し元は結合試験だけ。
//! DB の接続情報を渡す [`crate::database::EndpointHandle`] と同じ使い方（Box に入れる前にハンドルを取る）。
//!
//! 「起動」と「つながる」の意味: stub は sans-IO・同期の模擬で、ソケット・スレッド・子プロセスを持たない
//! （`docs/stubs/hub-stub.md`）。起動は設定から模擬一式を組んでハンドルに置くことで、試験が起動中のハンドルから
//! 模擬を呼び応答を受けることが「つながる」にあたる。最小の組込み先（TASK-716）から stub への接続は、
//! Host の実体が無いので本段では示せない（未達）。試験内の Fake の Host は達成に数えない（ACPT-7）。
//!
//! 安全: 環境変数・ファイル・ネットワークを使わない（DPOL-32）。stub を飛ばす・検証を外す分岐は作らない
//! （DPOL-30〜32）。`Debug` は種類と起動中かだけを出し、設定の値・鍵を出さない（SECR-11・SECR-23）。
//! stub の合格を実物の Hub の合格に数えない（A-2026-10-28-9・ACPT-7）。

use std::fmt;
use std::sync::{Arc, Mutex};

use fandhe_auth_hub_stub::config::{HubStubConfig, ProvisionalRecord};
use fandhe_auth_hub_stub::feed::ChangeFeedFetcher;
use fandhe_auth_hub_stub::heartbeat::HeartbeatReceiver;
use fandhe_auth_hub_stub::jwks::{AuthJwksFetcher, HubJwksEndpoint};

use crate::component::{Component, ComponentKind};
use crate::harness::{HarnessError, HarnessErrorKind};

/// 起動中の Hub の stub の模擬一式。設定だけで組める 4 つを持つ。
///
/// 鍵・期待値の要る受け口（本人トークン・Logout Token・assertion）は含めない。試験が [`Self::config`] から
/// 自分で組む。障害注入は `TASK-13` のハーネスの担当で、ここには入れない。
pub struct HubStubSession {
    config: HubStubConfig,
    /// Hub の JWKS を返す模擬。
    pub jwks: HubJwksEndpoint,
    /// 生存確認の受け口。
    pub heartbeat: HeartbeatReceiver,
    /// Hub が `fandhe-auth` の JWKS を取る模擬。
    pub auth_jwks: AuthJwksFetcher,
    /// 変更のフィードの取得の模擬。
    pub feed: ChangeFeedFetcher,
}

impl HubStubSession {
    fn new(config: HubStubConfig) -> Self {
        Self {
            jwks: HubJwksEndpoint::new(&config),
            heartbeat: HeartbeatReceiver::new(&config),
            auth_jwks: AuthJwksFetcher::new(&config),
            feed: ChangeFeedFetcher::new(&config),
            config,
        }
    }

    /// 起動時の設定。含めない受け口を試験側が組むために使う。
    pub fn config(&self) -> &HubStubConfig {
        &self.config
    }
}

/// 起動中の模擬一式への共有ハンドル。起動前と停止後は何も返さない。
#[derive(Clone, Default)]
pub struct HubStubHandle(Arc<Mutex<Option<HubStubSession>>>);

impl HubStubHandle {
    /// 起動中だけ `Some(f の戻り値)`。起動前・停止後・lock の毒化は `None`。
    ///
    /// `f` の実行中はセッションの lock を握る。`f` の中で `HubStubFaultInjector` の受け口を呼ぶと、遅れの `Wait` も
    /// lock を握ったまま走る。`Wait` の中で同じハンドル（`with`・`is_running` 等）を触ると自己待ちで止まるので、触らない。
    /// 遅れを lock の外で実行する構造にはしていない（遅れの実行は hub-stub の注入器の内側で、`f` の戻りより前に起きる）。
    pub fn with<R>(&self, f: impl FnOnce(&mut HubStubSession) -> R) -> Option<R> {
        self.0.lock().ok()?.as_mut().map(f)
    }

    /// 起動中か。
    pub fn is_running(&self) -> bool {
        self.0.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// 設定の欄ごとの「仮置き」の印の記録。起動中だけ値がある。値は載せない。
    pub fn provisional_record(&self) -> Option<ProvisionalRecord> {
        self.with(|s| s.config.provisional_record())
    }

    fn set(&self, session: Option<HubStubSession>) -> bool {
        match self.0.lock() {
            Ok(mut g) => {
                *g = session;
                true
            }
            Err(_) => false,
        }
    }
}

impl fmt::Debug for HubStubHandle {
    /// 起動中かだけを出し、設定の値・鍵を出さない（SECR-11・SECR-23）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubStubHandle")
            .field("running", &self.is_running())
            .finish()
    }
}

/// Hub の stub の構成要素。設定は必須で、既定値を持たない（A-2026-10-28-1）。
pub struct HubStubComponent {
    config: HubStubConfig,
    handle: HubStubHandle,
}

impl HubStubComponent {
    /// 設定を渡して作る。
    pub fn new(config: HubStubConfig) -> Self {
        Self {
            config,
            handle: HubStubHandle::default(),
        }
    }

    /// 共有ハンドル。`Box<dyn Component>` に入れる前に試験が取る。
    pub fn handle(&self) -> HubStubHandle {
        self.handle.clone()
    }

    /// 設定の欄ごとの「仮置き」の印の記録。起動前でも取れる。
    pub fn provisional_record(&self) -> ProvisionalRecord {
        self.config.provisional_record()
    }
}

impl fmt::Debug for HubStubComponent {
    /// 設定の値を出さない（SECR-11・SECR-23）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubStubComponent")
            .field("running", &self.handle.is_running())
            .finish()
    }
}

impl Component for HubStubComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::HubStub
    }

    /// 模擬一式を組んでハンドルに置く。既に起動中なら失敗する。
    fn start(&mut self) -> Result<(), HarnessError> {
        let fail = HarnessError::new(HarnessErrorKind::StartFailed, ComponentKind::HubStub);
        if self.handle.is_running() {
            return Err(fail);
        }
        if self
            .handle
            .set(Some(HubStubSession::new(self.config.clone())))
        {
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
                ComponentKind::HubStub,
            ))
        }
    }
}
