//! Chrome（chromedriver）の [`E2eComponent`] の実体と、操作の入口 [`ChromeHandle`]。
//!
//! 役割: `BrowserDriver` として chromedriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`ChromeHandle`] 経由（`Box<dyn E2eComponent>` に入れた後は直接操作できないため。it-harness の handle と同じ形）。
//!
//! 安全: 子プロセスは引数の配列で起動しシェルを通さない。stdin・stderr は捨てる。stdout は待ち受けポートを
//! 子自身から受け取るためだけに読む（`--port=0` で子が確保したポートを使い、空きポートの取り違えを避ける。A10）。
//! 環境変数・ファイルは読まない（DPOL-32）。Chrome の flag は `--headless=new` だけで、検証を外す flag を足さない
//! （DPOL-30）。`navigate` は起動前に登録した試験用 origin の配下だけを許す（A10）。
//! 誤り・停止・`Drop` のすべてでセッションの削除と、子のプロセスグループ（chromedriver と Chrome）の終了・wait を行う。

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::component::{E2eComponent, E2eComponentKind};
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;
use crate::webdriver::{
    AuthenticatorId, CredentialSummary, WebDriverClient, WebDriverSession, deadline_after,
};

fn err(kind: E2eErrorKind) -> E2eError {
    E2eError::new(kind, E2eComponentKind::BrowserDriver)
}

/// chromedriver の起動設定。`driver_program` は PATH 上の名前か絶対パスを呼び出し側が渡す（環境から読まない）。
#[derive(Debug, Clone)]
pub struct ChromeDriverConfig {
    driver_program: OsString,
    ready_timeout: Duration,
    command_timeout: Duration,
    allowed_origins: Vec<String>,
}

/// 試験用 origin の形。`http://localhost:<port>` か `http://127.0.0.1:<port>` だけ（path・userinfo を含めない）。
fn validated_origin(origin: &str) -> Result<String, E2eError> {
    let rest = origin
        .strip_prefix("http://localhost:")
        .or_else(|| origin.strip_prefix("http://127.0.0.1:"));
    let ok = rest.is_some_and(|p| {
        !p.is_empty()
            && p.len() <= 5
            && p.bytes().all(|b| b.is_ascii_digit())
            && p.parse::<u16>().is_ok_and(|n| n != 0)
    });
    if ok {
        Ok(origin.to_owned())
    } else {
        Err(err(E2eErrorKind::CommandFailed))
    }
}

/// `url` が許可した origin の配下か。origin の直後は `/`・`?`・`#` だけを許す。制御文字・空白を含む値は拒否する。
fn url_is_allowed(allowed: &[String], url: &str) -> bool {
    if url.bytes().any(|b| b <= 0x20 || b == 0x7f || b == b'\\') {
        return false;
    }
    allowed.iter().any(|o| {
        url.strip_prefix(o.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
    })
}

impl ChromeDriverConfig {
    /// 起動する実行ファイルを決めた設定。時間切れは既定（ハーネスの安全上の値で、要件の値ではない）。
    pub fn new(driver_program: impl Into<OsString>) -> Self {
        Self {
            driver_program: driver_program.into(),
            ready_timeout: Duration::from_secs(30),
            command_timeout: Duration::from_secs(30),
            allowed_origins: Vec::new(),
        }
    }

    /// `navigate` を許す試験用 origin を足す（A10）。足さなければ `navigate` はすべて拒否される。
    /// 形は `http://localhost:<port>` か `http://127.0.0.1:<port>` だけ。
    pub fn with_allowed_origin(mut self, origin: &str) -> Result<Self, E2eError> {
        self.allowed_origins.push(validated_origin(origin)?);
        Ok(self)
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
#[derive(Clone)]
pub struct ChromeHandle {
    live: Arc<Mutex<Option<Live>>>,
    allowed_origins: Arc<Vec<String>>,
}

impl ChromeHandle {
    fn session(&self) -> Result<WebDriverSession, E2eError> {
        let guard = self
            .live
            .lock()
            .map_err(|_| err(E2eErrorKind::NotRunning))?;
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
        let guard = self
            .live
            .lock()
            .map_err(|_| err(E2eErrorKind::NotRunning))?;
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

    /// URL へ移動する。起動前に登録した試験用 origin の配下だけを許し、それ以外は `CommandFailed`（A10）。
    pub fn navigate(&self, url: &str) -> Result<(), E2eError> {
        let session = self.session()?;
        if !url_is_allowed(&self.allowed_origins, url) {
            return Err(err(E2eErrorKind::CommandFailed));
        }
        session.navigate(url)
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        self.session()?.set_script_timeout(timeout)
    }

    /// 非同期スクリプトを実行する。crate 内の定数スクリプトだけが使う（任意のスクリプトを外へ開かない）。
    pub(crate) fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
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
            handle: ChromeHandle {
                live: Arc::new(Mutex::new(None)),
                allowed_origins: Arc::new(config.allowed_origins.clone()),
            },
            config,
            child: None,
        }
    }

    /// 操作の入口。`Box` に入れる前に取る。
    pub fn handle(&self) -> ChromeHandle {
        self.handle.clone()
    }

    fn teardown(&mut self) -> Result<(), E2eError> {
        let mut result = Ok(());
        let live = self.handle.live.lock().ok().and_then(|mut g| g.take());
        // セッションの削除は最善を尽くすだけ。失敗してもプロセスグループの終了へ進む。
        if let Some(live) = live
            && live.session.delete().is_err()
        {
            result = Err(err(E2eErrorKind::StopFailed));
        }
        if let Some(mut child) = self.child.take()
            && terminate_group(&mut child).is_err()
        {
            result = Err(err(E2eErrorKind::StopFailed));
        }
        result
    }

    fn try_start(&mut self) -> Result<(), E2eError> {
        let fail = || err(E2eErrorKind::StartFailed);
        let deadline = deadline_after(self.config.ready_timeout).map_err(|_| fail())?;
        // `--port=0` で子に空きポートを確保させ、子の出力から受け取る。子自身が持つポートなので別のドライバと
        // 取り違えない（A10）。子は新しいプロセスグループに入れ、chromedriver 配下の Chrome ごと後片付けする。
        let mut child = Command::new(&self.config.driver_program)
            .arg("--port=0")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| fail())?;
        let stdout = child.stdout.take();
        self.child = Some(child);
        let stdout = stdout.ok_or_else(fail)?;

        let (tx, rx) = mpsc::channel::<u16>();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if !sent && let Some(port) = parse_started_port(&line) {
                    sent = true;
                    let _ = tx.send(port);
                }
                // 以降も読み捨てて、子が出力で詰まらないようにする。子の終了で EOF になる。
            }
        });
        let wait = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(fail)?;
        let port = rx.recv_timeout(wait).map_err(|_| fail())?;

        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(fail)?;
            // 各 probe は残り時間を超えない。
            let client = WebDriverClient::new(port, self.config.command_timeout.min(left));
            if client.is_ready() && Instant::now() < deadline {
                break;
            }
            let running = matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)));
            let left = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(fail)?;
            if !running {
                return Err(fail());
            }
            sleep(Duration::from_millis(100).min(left));
        }
        let client = WebDriverClient::new(port, self.config.command_timeout);
        let (session, browser_version) = client
            .new_session(&new_session_request())
            .map_err(|_| fail())?;
        let mut guard = self.handle.live.lock().map_err(|_| fail())?;
        *guard = Some(Live {
            session,
            browser_version,
        });
        Ok(())
    }
}

/// chromedriver が起動時に出す `ChromeDriver was started successfully on port N` から N を取る。
/// 最初の行の `on port 0`（要求した値）は拾わない。
fn parse_started_port(line: &str) -> Option<u16> {
    let rest = line.split("was started successfully on port ").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u16>().ok().filter(|p| *p != 0)
}

/// 子のプロセスグループ（chromedriver と配下の Chrome）を終了して wait する。まず SIGTERM で正常終了を促し
/// （一時プロファイルの後始末のため）、時間内に終わらなければ SIGKILL。
fn terminate_group(child: &mut Child) -> std::io::Result<()> {
    let group = format!("-{}", child.id());
    let signal = |sig: &str| {
        let _ = Command::new("kill")
            .args([sig, "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    signal("-TERM");
    let until = Instant::now() + Duration::from_secs(3);
    let mut exited = false;
    while Instant::now() < until {
        if matches!(child.try_wait(), Ok(Some(_))) {
            exited = true;
            break;
        }
        sleep(Duration::from_millis(50));
    }
    // 親が先に終わっても孫が残り得るので、グループへ必ず SIGKILL する。
    signal("-KILL");
    if !exited {
        let _ = child.kill();
    }
    child.wait().map(|_| ())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        vec!["http://localhost:8080".to_owned()]
    }

    #[test]
    fn navigate_is_limited_to_the_registered_origin() {
        let a = allowed();
        assert!(url_is_allowed(&a, "http://localhost:8080"));
        assert!(url_is_allowed(&a, "http://localhost:8080/"));
        assert!(url_is_allowed(&a, "http://localhost:8080/x?y=1#z"));
        for bad in [
            "http://localhost:80801/",
            "http://localhost:8080.evil.example/",
            format!("{}@evil.example/", "http://localhost:8080").as_str(),
            "http://evil.example/",
            "http://localhost:8081/",
            "https://localhost:8080/",
            "file:///etc/passwd",
            "http://localhost:8080/ x",
            format!("{}\\{}", "http://localhost:8080", "evil").as_str(),
            "",
        ] {
            assert!(!url_is_allowed(&a, bad), "{bad:?}");
        }
        assert!(!url_is_allowed(&[], "http://localhost:8080/"));
    }

    #[test]
    fn origin_registration_accepts_only_loopback_origin_form() {
        assert!(validated_origin("http://localhost:8080").is_ok());
        assert!(validated_origin("http://127.0.0.1:1").is_ok());
        for bad in [
            "http://localhost",
            "http://localhost:0",
            "http://localhost:8080/",
            "http://localhost:99999",
            "http://example.com:80",
            "https://localhost:8080",
            format!("{}@evil", "http://localhost:80").as_str(),
        ] {
            assert!(validated_origin(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn started_port_line_is_parsed_and_requested_port_zero_is_ignored() {
        assert_eq!(
            parse_started_port("ChromeDriver was started successfully on port 45385."),
            Some(45385)
        );
        assert_eq!(
            parse_started_port("Starting ChromeDriver 1.0 (x) on port 0"),
            None
        );
        assert_eq!(parse_started_port("other"), None);
    }
}
