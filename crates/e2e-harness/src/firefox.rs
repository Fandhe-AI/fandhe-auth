//! Firefox（geckodriver）の [`E2eComponent`] の実体。起動・停止は [`crate::driver_process`] の共通部に任せる。
//!
//! 役割: `BrowserDriver` として geckodriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`FirefoxHandle`]（[`DriverHandle`] を包む別型）経由。Chrome の [`crate::chrome`] と同じ形。
//!
//! 安全: Firefox の引数は `-headless` だけ、pref は仮想 authenticator に要る 2 つだけで、検証を外す意味の引数・pref を
//! 足さない（DPOL-30）。環境変数は読まない（DPOL-32）。停止・起動の失敗・`Drop` の手順と受け入れた制限は
//! [`crate::driver_process`] に書く。unix 限定。

use std::ops::Deref;

use serde_json::{Value, json};

use crate::browser::Browser;
use crate::component::{E2eComponent, E2eComponentKind};
use crate::driver_process::{DriverComponent, DriverConfig, DriverHandle, DriverSpec};
use crate::harness::E2eError;

/// geckodriver の起動設定（共通の設定型）。`driver_program` は PATH 上の名前か絶対パスを呼び出し側が渡す（環境から読まない）。
pub type GeckoDriverConfig = DriverConfig;

/// Firefox の操作の入口。共通の [`DriverHandle`] を内部で共有する別型で、他のブラウザの入口と取り違えると型で弾く。
/// 操作は [`Deref`] 経由で [`DriverHandle`] のメソッドを使う。
#[derive(Clone, Default)]
pub struct FirefoxHandle(DriverHandle);

impl Deref for FirefoxHandle {
    type Target = DriverHandle;

    fn deref(&self) -> &DriverHandle {
        &self.0
    }
}

/// `POST /session` の本文。引数は `-headless` だけ。pref は 2 つだけ（PoC-3 の値。要件の値ではなく、人の確認待ち）。
/// Firefox の既定の設定では仮想 authenticator の作成が `ConstraintError` になるため、softtoken を有効・usbtoken を無効にする。
/// 検証を外す意味の引数・pref は足さない（DPOL-30）。
pub fn new_session_request() -> Value {
    json!({
        "capabilities": {
            "alwaysMatch": {
                "browserName": "firefox",
                "moz:firefoxOptions": {
                    "args": ["-headless"],
                    "prefs": {
                        "security.webauth.webauthn_enable_softtoken": true,
                        "security.webauth.webauthn_enable_usbtoken": false
                    }
                }
            }
        }
    })
}

fn port_args(port: u16) -> Vec<String> {
    vec!["--port".to_owned(), port.to_string()]
}

/// geckodriver の構成要素。
pub struct GeckoDriverComponent(DriverComponent);

impl GeckoDriverComponent {
    /// 作る。起動はしない。
    pub fn new(config: GeckoDriverConfig) -> Self {
        Self(DriverComponent::new(
            config,
            DriverSpec {
                port_args,
                session_request: new_session_request,
            },
        ))
    }

    /// 操作の入口。`Box` に入れる前に取る。
    pub fn handle(&self) -> FirefoxHandle {
        FirefoxHandle(self.0.handle())
    }
}

impl E2eComponent for GeckoDriverComponent {
    fn kind(&self) -> E2eComponentKind {
        E2eComponentKind::BrowserDriver
    }

    fn browser(&self) -> Option<Browser> {
        Some(Browser::Firefox)
    }

    fn start(&mut self) -> Result<(), E2eError> {
        self.0.start()
    }

    fn stop(&mut self) -> Result<(), E2eError> {
        self.0.stop()
    }
}
