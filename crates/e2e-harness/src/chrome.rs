//! Chrome（chromedriver）の [`E2eComponent`] の実体と、操作の入口 [`ChromeHandle`]。
//!
//! 役割: `BrowserDriver` として chromedriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`ChromeHandle`] 経由（`Box<dyn E2eComponent>` に入れた後は直接操作できないため。it-harness の handle と同じ形）。
//!
//! 安全: 子プロセスは引数の配列で起動しシェルを通さない。stdin・stdout・stderr は捨てる。環境変数・ファイルは読まない
//! （DPOL-32）。Chrome の flag は `--headless=new` だけで、検証を外す flag を足さない（DPOL-30）。
//! 誤り・停止・`Drop` のすべてで、セッションの削除（最善）→ 子のプロセスグループへ `SIGKILL` → 期限付きの wait を行う。
//! セッションの削除に失敗しても、セッション ID が無くても、配下の Chrome ごとグループで止める。
//!
//! unix 限定（`process_group` と `kill -KILL -<pgid>` を使う）。Linux・macOS で同じ手順で動く。
//!
//! 受け入れた制限（試験の環境で許容する。人の確認待ち）: 空きポートを取って手放してから子に渡すため、その間に別の
//! プロセスが同じポートを取ると取り違え得る。ブラウザの送信先は制限しない（試験用ページは loopback の試験用 origin だけを開く）。

use std::ffi::OsString;
use std::net::{Ipv4Addr, TcpListener};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::component::{E2eComponent, E2eComponentKind};
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;
use crate::webdriver::{AuthenticatorId, CredentialSummary, WebDriverClient, WebDriverSession};

/// 停止で子の終了を待つ期限。ハーネスの安全上の値で、要件の値ではない。
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

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
            // 回収の前にグループへ送る（未回収の子がグループ ID を保つので、別のプロセスを宛先にしない）。
            // chromedriver が配下の Chrome を残しても、同じグループごと止める。
            let _ = kill_group(child.id());
            let _ = child.kill();
            if wait_until(&mut child, STOP_TIMEOUT).is_err() {
                // 終了を確かめられない子は手放さず、stop()・Drop で再試行できるようにする。
                self.child = Some(child);
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
            // 新しいプロセスグループ（グループ ID = 子の PID）。停止で配下の Chrome ごと止めるため。
            .process_group(0)
            .spawn()
            .map_err(|_| fail())?;
        self.child = Some(child);

        let client = WebDriverClient::new(port, self.config.command_timeout);
        let deadline = Instant::now()
            .checked_add(self.config.ready_timeout)
            .ok_or_else(fail)?;
        loop {
            // probe も起動待ちの残り時間で打ち切り、期限後の ready は受理しない。
            let left = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(fail)?;
            let probe = WebDriverClient::new(port, left.min(self.config.command_timeout));
            if probe.is_ready() && Instant::now() < deadline {
                break;
            }
            let running = matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)));
            if !running {
                return Err(fail());
            }
            sleep(Duration::from_millis(100).min(left));
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

/// プロセスグループ `pgid` へ `SIGKILL` を送る。`-KILL -<pgid>` は GNU・BSD の `kill` の両方で通る
/// （BSD の `kill` は `--` を pid として拒否するので使わない）。
fn kill_group(pgid: u32) -> std::io::Result<()> {
    let status = Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("kill failed"))
    }
}

/// 子の終了を `timeout` まで待って回収する。期限内に終わらなければ誤り（子は回収しない）。
fn wait_until(child: &mut Child, timeout: Duration) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::other("child did not exit"));
        }
        sleep(Duration::from_millis(20));
    }
}
