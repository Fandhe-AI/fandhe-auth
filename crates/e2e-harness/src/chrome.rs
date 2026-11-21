//! Chrome（chromedriver）の [`E2eComponent`] の実体。起動・停止は [`crate::driver_process`] の共通部に任せる。
//!
//! 役割: `BrowserDriver` として chromedriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`ChromeHandle`]（[`DriverHandle`] を包む別型）経由（`Box<dyn E2eComponent>` に入れた後は直接操作できないため。it-harness の handle と同じ形）。
//!
//! 安全: Chrome の flag は `--headless=new` だけで、検証を外す flag を足さない（DPOL-30）。停止・起動の失敗・`Drop` の手順と
//! 受け入れた制限は [`crate::driver_process`] に書く。unix 限定。

use std::ops::Deref;

use serde_json::{Value, json};

use crate::browser::Browser;
use crate::component::{E2eComponent, E2eComponentKind};
use crate::driver_process::{DriverComponent, DriverConfig, DriverHandle, DriverSpec};
use crate::harness::E2eError;

/// chromedriver の起動設定（共通の設定型）。`driver_program` は PATH 上の名前か絶対パスを呼び出し側が渡す（環境から読まない）。
pub type ChromeDriverConfig = DriverConfig;

/// Chrome の操作の入口。共通の [`DriverHandle`] を内部で共有する別型で、他のブラウザの入口と取り違えると型で弾く。
/// 操作は [`Deref`] 経由で [`DriverHandle`] のメソッドを使う。
#[derive(Clone, Default)]
pub struct ChromeHandle(DriverHandle);

impl Deref for ChromeHandle {
    type Target = DriverHandle;

    fn deref(&self) -> &DriverHandle {
        &self.0
    }
}

/// `POST /session` の本文。Chrome の flag は `--headless=new` だけ（DPOL-30）。
pub fn new_session_request() -> Value {
    json!({
        "capabilities": {
            "alwaysMatch": {
                "browserName": "chrome",
                "goog:chromeOptions": { "args": ["--headless=new"] }
            }
        }
    })
}

fn port_args(port: u16) -> Vec<String> {
    vec![format!("--port={port}")]
}

/// chromedriver の構成要素。
pub struct ChromeDriverComponent(DriverComponent);

impl ChromeDriverComponent {
    /// 作る。起動はしない。
    pub fn new(config: ChromeDriverConfig) -> Self {
        Self(DriverComponent::new(
            config,
            DriverSpec {
                port_args,
                session_request: new_session_request,
                browser_name: "chrome",
            },
        ))
    }

    /// 操作の入口。`Box` に入れる前に取る。
    pub fn handle(&self) -> ChromeHandle {
        ChromeHandle(self.0.handle())
    }
}

impl E2eComponent for ChromeDriverComponent {
    fn kind(&self) -> E2eComponentKind {
        E2eComponentKind::BrowserDriver
    }

    fn browser(&self) -> Option<Browser> {
        Some(Browser::Chrome)
    }

    fn start(&mut self) -> Result<(), E2eError> {
        self.0.start()
    }

    fn stop(&mut self) -> Result<(), E2eError> {
        self.0.stop()
    }
}
