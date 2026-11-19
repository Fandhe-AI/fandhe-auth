//! Chrome（chromedriver）の [`E2eComponent`] の実体と、操作の入口 [`ChromeHandle`]。
//!
//! 役割: `BrowserDriver` として chromedriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`ChromeHandle`] 経由（`Box<dyn E2eComponent>` に入れた後は直接操作できないため。it-harness の handle と同じ形）。
//!
//! 安全: 子プロセスは引数の配列で起動しシェルを通さない。stdin・stdout・stderr は捨てる。環境変数・ファイルは読まない
//! （DPOL-32）。Chrome の flag は `--headless=new` だけで、検証を外す flag を足さない（DPOL-30）。
//! 誤り・停止・`Drop` のすべてでセッションの削除と子プロセスの kill・wait を行う。

use std::ffi::OsString;
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::component::{E2eComponent, E2eComponentKind};
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;
use crate::webdriver::{AuthenticatorId, CredentialSummary, WebDriverClient, WebDriverSession};

fn err(kind: E2eErrorKind) -> E2eError {
    E2eError::new(kind, E2eComponentKind::BrowserDriver)
}

/// chromedriver の起動設定。`driver_program` は PATH 上の名前か絶対パスを呼び出し側が渡す（環境から読まない）。
#[derive(Debug, Clone)]
pub struct ChromeDriverConfig {
    driver_program: OsString,
    ready_timeout: Duration,
    command_timeout: Duration,
}

impl ChromeDriverConfig {
    /// 起動する実行ファイルを決めた設定。時間切れは既定（ハーネスの安全上の値で、要件の値ではない）。
    pub fn new(driver_program: impl Into<OsString>) -> Self {
        Self {
            driver_program: driver_program.into(),
            ready_timeout: Duration::from_secs(30),
            command_timeout: Duration::from_secs(30),
        }
    }

    /// 起動待ちの時間切れを差し替える。
    #[must_use]
    pub fn with_ready_timeout(mut self, d: Duration) -> Self {
        self.ready_timeout = d;
        self
    }

    /// 命令 1 つの時間切れを差し替える。
    #[must_use]
    pub fn with_command_timeout(mut self, d: Duration) -> Self {
        self.command_timeout = d;
        self
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

struct Live {
    session: WebDriverSession,
    browser_version: String,
}

/// ブラウザの操作の入口。起動前・停止後は `NotRunning`。
#[derive(Clone, Default)]
pub struct ChromeHandle(Arc<Mutex<Option<Live>>>);

impl ChromeHandle {
    fn session(&self) -> Result<WebDriverSession, E2eError> {
        let guard = self.0.lock().map_err(|_| err(E2eErrorKind::NotRunning))?;
        guard
            .as_ref()
            .map(|l| l.session.clone())
            .ok_or_else(|| err(E2eErrorKind::NotRunning))
    }

    /// 動いているか。
    pub fn is_running(&self) -> bool {
        self.session().is_ok()
    }

    /// ブラウザの版（実行の記録用。記録の形は 5/6）。
    pub fn browser_version(&self) -> Result<String, E2eError> {
        let guard = self.0.lock().map_err(|_| err(E2eErrorKind::NotRunning))?;
        guard
            .as_ref()
            .map(|l| l.browser_version.clone())
            .ok_or_else(|| err(E2eErrorKind::NotRunning))
    }

    /// 仮想 authenticator を足す。
    pub fn add_virtual_authenticator(
        &self,
        options: &VirtualAuthenticatorOptions,
    ) -> Result<AuthenticatorId, E2eError> {
        self.session()?.add_virtual_authenticator(options)
    }

    /// URL へ移動する。
    pub fn navigate(&self, url: &str) -> Result<(), E2eError> {
        self.session()?.navigate(url)
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        self.session()?.set_script_timeout(timeout)
    }

    /// 非同期スクリプトを実行する。
    pub fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
        self.session()?.execute_async(script, args)
    }

    /// 仮想 authenticator の credential の要約。
    pub fn credentials(&self, id: &AuthenticatorId) -> Result<Vec<CredentialSummary>, E2eError> {
        self.session()?.credentials(id)
    }
}

/// chromedriver の構成要素。
pub struct ChromeDriverComponent {
    config: ChromeDriverConfig,
    handle: ChromeHandle,
    child: Option<Child>,
}

impl ChromeDriverComponent {
    /// 作る。起動はしない。
    pub fn new(config: ChromeDriverConfig) -> Self {
        Self {
            config,
            handle: ChromeHandle::default(),
            child: None,
        }
    }

    /// 操作の入口。`Box` に入れる前に取る。
    pub fn handle(&self) -> ChromeHandle {
        self.handle.clone()
    }

    fn teardown(&mut self) -> Result<(), E2eError> {
        let mut result = Ok(());
        let live = self.handle.0.lock().ok().and_then(|mut g| g.take());
        if let Some(live) = live
            && live.session.delete().is_err()
        {
            result = Err(err(E2eErrorKind::StopFailed));
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            if child.wait().is_err() {
                result = Err(err(E2eErrorKind::StopFailed));
            }
        }
        result
    }

    fn try_start(&mut self) -> Result<(), E2eError> {
        let fail = || err(E2eErrorKind::StartFailed);
        // 空きポートを取って手放す。取り直しまでの競合は試験の環境では許容する。
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|l| l.local_addr())
            .map_err(|_| fail())?
            .port();
        let child = Command::new(&self.config.driver_program)
            .arg(format!("--port={port}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| fail())?;
        self.child = Some(child);

        let client = WebDriverClient::new(port, self.config.command_timeout);
        let deadline = Instant::now() + self.config.ready_timeout;
        loop {
            if client.is_ready() {
                break;
            }
            let running = matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)));
            if !running || Instant::now() >= deadline {
                return Err(fail());
            }
            sleep(Duration::from_millis(100));
        }
        let (session, browser_version) = client
            .new_session(&new_session_request())
            .map_err(|_| fail())?;
        let mut guard = self.handle.0.lock().map_err(|_| fail())?;
        *guard = Some(Live {
            session,
            browser_version,
        });
        Ok(())
    }
}

impl E2eComponent for ChromeDriverComponent {
    fn kind(&self) -> E2eComponentKind {
        E2eComponentKind::BrowserDriver
    }

    fn start(&mut self) -> Result<(), E2eError> {
        if self.child.is_some() {
            return Err(err(E2eErrorKind::StartFailed));
        }
        let result = self.try_start();
        if result.is_err() {
            // 途中で失敗したらセッション・子プロセスを残さない。
            let _ = self.teardown();
        }
        result
    }

    fn stop(&mut self) -> Result<(), E2eError> {
        self.teardown()
    }
}

impl Drop for ChromeDriverComponent {
    fn drop(&mut self) {
        let _ = self.teardown();
    }
}
