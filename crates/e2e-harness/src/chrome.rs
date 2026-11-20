//! Chrome（chromedriver）の [`E2eComponent`] の実体と、操作の入口 [`ChromeHandle`]。
//!
//! 役割: `BrowserDriver` として chromedriver を子プロセスで起動し、ヘッドレスのセッションを 1 つ作る。
//! 呼び出し元は E2E の試験で、起動・停止は [`crate::harness::E2eHarness`] 経由、操作は起動前に取った
//! [`ChromeHandle`] 経由（`Box<dyn E2eComponent>` に入れた後は直接操作できないため。it-harness の handle と同じ形）。
//!
//! 安全: 子プロセスは引数の配列で起動しシェルを通さない。stdin・stderr は捨てる。stdout は待ち受けポートを
//! 子自身から受け取るためだけに読む（`--port=0` で子が確保したポートを使い、空きポートの取り違えを避ける。A10）。
//! 環境変数・ファイルは読まない（DPOL-32）。Chrome の flag は `--headless=new` と外部への通信を禁じる 3 つ（名前の解決禁止・
//! 解決できない proxy・登録 origin だけの bypass list）だけで、検証を外す flag を足さない（DPOL-30）。`navigate` は起動前に登録した試験用 origin の配下だけを許し、移動後の
//! URL（リダイレクト先）も同じ許可リストで確かめる。サブリソースと許可外のループバックのポートへの通信は、proxy の bypass list が登録 origin だけを直接接続にするため送信前に失敗する（A10）。
//! unix 限定（プロセスグループで起動する。`it-harness` の `fault::instance` と同じ）。
//! A10 の保証範囲: proxy 設定が効くのは Chrome の HTTP(S) の送信先だけ。WebRTC の直接 UDP など proxy を通らない
//! 経路はこの flag では制限できず、保証しない（試験用ページはそれらを使わない。ネットワーク層の隔離は実行環境の担当で、ここでは主張しない）。
//! 誤り・停止・`Drop` のすべてでセッションの削除と、子のプロセスグループ（chromedriver と Chrome）の終了・wait を行う。

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
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
/// ポートは先頭ゼロなしの 1〜65535 で、既定ポート 80 は正規形でないため拒否する。
fn validated_origin(origin: &str) -> Result<String, E2eError> {
    let rest = origin
        .strip_prefix("http://localhost:")
        .or_else(|| origin.strip_prefix("http://127.0.0.1:"));
    let ok = rest.is_some_and(|p| {
        !p.is_empty()
            && p.len() <= 5
            && p.bytes().all(|b| b.is_ascii_digit())
            // 先頭ゼロ・既定ポート 80 は origin の正規形でないので設定時に拒否する（比較を文字列の完全一致に保つ）。
            && !p.starts_with('0')
            && p.parse::<u16>().is_ok_and(|n| n != 0 && n != 80)
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

/// 外部名の解決を禁じる flag（`localhost` 以外の名前は引けない）。試験用ページが外部へ出るサブリソース・
/// リダイレクトを辿れないようにする制限で、検証を外す flag ではない（A10・DPOL-30）。
pub const HOST_RESOLVER_RULES_ARG: &str =
    "--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE localhost";

/// すべての通信を解決できない proxy へ向ける flag。名前の解決は上の規則で必ず失敗するため、proxy 経由の通信は成立しない。
/// 登録した origin だけを下の bypass list で直接接続にする（許可外のループバックのポートへ送信させない。A10）。
pub const PROXY_SERVER_ARG: &str = "--proxy-server=http://proxy.invalid:9";

/// `--proxy-bypass-list` の flag。`<-loopback>` で loopback の暗黙の除外を外し、登録した `host:port` だけを直接接続にする。
/// 各規則は `http://host:port` のスキーム付き（同じ host:port の HTTPS を直接接続にしない）。
/// 検証済みの origin だけを入れる（ワイルドカード・区切り文字を含む値は黙って捨てる。fail closed）。
fn proxy_bypass_arg(allowed_origins: &[String]) -> String {
    let mut list = String::from("--proxy-bypass-list=<-loopback>");
    for o in allowed_origins {
        if let Ok(valid) = validated_origin(o) {
            list.push(';');
            list.push_str(&valid);
        }
    }
    list
}

/// `POST /session` の本文。Chrome の flag は `--headless=new` と、外部への通信を禁じる 3 つだけ（DPOL-30。実起動ではこれに `--user-data-dir` が加わる）。
/// `allowed_origins` に登録した origin 以外へは、初回の移動・リダイレクト・サブリソースとも送信前に失敗する（A10）。
pub fn new_session_request_for(allowed_origins: &[String]) -> Value {
    new_session_request_with_profile(allowed_origins, None)
}

/// 実際の起動（`try_start`）と同じ入力で本文を作る入口。
/// `profile_dir` があれば `--user-data-dir` で Chrome の一時領域をハーネスが所有する場所に固定する（後片付けはハーネスが行う）。
pub fn new_session_request_with_profile(
    allowed_origins: &[String],
    profile_dir: Option<&Path>,
) -> Value {
    let mut args = vec![
        "--headless=new".to_owned(),
        HOST_RESOLVER_RULES_ARG.to_owned(),
        PROXY_SERVER_ARG.to_owned(),
        proxy_bypass_arg(allowed_origins),
    ];
    if let Some(dir) = profile_dir {
        args.push(format!("--user-data-dir={}", dir.display()));
    }
    json!({
        "capabilities": {
            "alwaysMatch": {
                "browserName": "chrome",
                "goog:chromeOptions": { "args": args }
            }
        }
    })
}

/// 許可 origin なしの `POST /session` の本文（すべての通信が失敗する）。
pub fn new_session_request() -> Value {
    new_session_request_for(&[])
}

struct Live {
    session: WebDriverSession,
    browser_version: String,
    /// 移動の結果を確定できなかった。以後の操作は拒否し、停止処理で回収する。
    unusable: bool,
}

/// ブラウザの操作の入口。起動前・停止後は `NotRunning`。
#[derive(Clone)]
pub struct ChromeHandle {
    live: Arc<Mutex<Option<Live>>>,
    allowed_origins: Arc<Vec<String>>,
}

impl ChromeHandle {
    /// セッションの操作を、ロックを握ったまま実行する。停止（`teardown`）は同じロックを取るので、
    /// 実行中の命令が終わるまで待ち、停止・回収後の古いポートへ命令を送れない。命令は時間切れで必ず終わる。
    fn with_live<T>(
        &self,
        f: impl FnOnce(&mut Live) -> Result<T, E2eError>,
    ) -> Result<T, E2eError> {
        let mut guard = self
            .live
            .lock()
            .map_err(|_| err(E2eErrorKind::NotRunning))?;
        let live = guard
            .as_mut()
            .ok_or_else(|| err(E2eErrorKind::NotRunning))?;
        if live.unusable {
            return Err(err(E2eErrorKind::CommandFailed));
        }
        f(live)
    }

    /// `origin` が起動前に登録した試験用 origin のどれかと完全一致するか。
    pub(crate) fn origin_is_registered(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == origin)
    }

    /// 動いているか（使用不可にされたセッションは動いていないものとして扱う）。
    pub fn is_running(&self) -> bool {
        self.live
            .lock()
            .is_ok_and(|g| g.as_ref().is_some_and(|l| !l.unusable))
    }

    /// ブラウザの版（実行の記録用。記録の形は 5/6）。
    pub fn browser_version(&self) -> Result<String, E2eError> {
        self.with_live(|l| Ok(l.browser_version.clone()))
    }

    /// 仮想 authenticator を足す。
    pub fn add_virtual_authenticator(
        &self,
        options: &VirtualAuthenticatorOptions,
    ) -> Result<AuthenticatorId, E2eError> {
        self.with_live(|l| l.session.add_virtual_authenticator(options))
    }

    /// URL へ移動する。起動前に登録した試験用 origin の配下だけを許し、それ以外は `CommandFailed`（A10）。
    /// 移動後の URL が許可外（リダイレクト）でも `CommandFailed`。移動の結果を確定できない（命令の失敗・時間切れ・
    /// 移動後の URL を取れない・空白ページへ戻せない）ときは、セッションを使用不可にし、停止処理で回収する。
    pub fn navigate(&self, url: &str) -> Result<(), E2eError> {
        if !url_is_allowed(&self.allowed_origins, url) {
            // セッションの有無の確認（起動前・停止後は NotRunning）だけ行う。
            return self.with_live(|_| Err(err(E2eErrorKind::CommandFailed)));
        }
        self.with_live(|l| {
            if l.session.navigate(url).is_err() {
                l.unusable = true;
                return Err(err(E2eErrorKind::CommandFailed));
            }
            // リダイレクト先も許可リストで確かめる。外れていたら空白ページへ戻して誤りにする。
            match l.session.current_url() {
                Ok(u) if url_is_allowed(&self.allowed_origins, &u) => Ok(()),
                Ok(_) => {
                    if l.session.navigate("about:blank").is_err() {
                        l.unusable = true;
                    }
                    Err(err(E2eErrorKind::CommandFailed))
                }
                // 移動後の URL を取れない場合は着地先を確定できないので、空白ページへの復帰を試みず使用不可にする。
                Err(_) => {
                    l.unusable = true;
                    Err(err(E2eErrorKind::CommandFailed))
                }
            }
        })
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        self.with_live(|l| l.session.set_script_timeout(timeout))
    }

    /// 非同期スクリプトを実行する。crate 内の定数スクリプトだけが使う（任意のスクリプトを外へ開かない）。
    pub(crate) fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
        self.with_live(|l| {
            let r = l.session.execute_async(script, args);
            if r.is_err() {
                // 通信の失敗は結果不明（実行済みかもしれない）。同じセッションで続けず、停止処理で回収する。
                l.unusable = true;
            }
            r
        })
    }

    /// 仮想 authenticator の credential の要約。
    pub fn credentials(&self, id: &AuthenticatorId) -> Result<Vec<CredentialSummary>, E2eError> {
        self.with_live(|l| l.session.credentials(id))
    }
}

/// chromedriver の構成要素。
pub struct ChromeDriverComponent {
    config: ChromeDriverConfig,
    handle: ChromeHandle,
    child: Option<Child>,
    /// Chrome の一時領域（ハーネスが作り、プロセスの回収後にセッション ID に依らず消す）。
    profile_dir: Option<PathBuf>,
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
            profile_dir: None,
        }
    }

    /// 操作の入口。`Box` に入れる前に取る。
    pub fn handle(&self) -> ChromeHandle {
        self.handle.clone()
    }

    fn teardown(&mut self) -> Result<(), E2eError> {
        let mut result = Ok(());
        // ロックを取るので、実行中の命令が終わってからセッションを取り出す（命令と停止の同期）。
        let live = match self.handle.live.lock() {
            Ok(mut g) => g.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        // セッションの削除は最善を尽くすだけ。失敗してもプロセスグループの終了へ進む。
        if let Some(live) = live
            && live.session.delete().is_err()
        {
            result = Err(err(E2eErrorKind::StopFailed));
        }
        if let Some(mut child) = self.child.take()
            && kill_group_and_reap(&mut child).is_err()
        {
            result = Err(err(E2eErrorKind::StopFailed));
        }
        // プロセスの回収後に一時領域を消す（セッションを確立できなかった場合も同じ）。
        // 削除の成功か NotFound を確かめるまでパスを保持し、stop()・Drop で再試行できるようにする。
        if let Some(dir) = self.profile_dir.as_ref() {
            match std::fs::remove_dir_all(dir) {
                Ok(()) => self.profile_dir = None,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.profile_dir = None,
                Err(_) => result = Err(err(E2eErrorKind::StopFailed)),
            }
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
        let profile = make_profile_dir().map_err(|_| fail())?;
        self.profile_dir = Some(profile.clone());
        let stdout = stdout.ok_or_else(fail)?;

        let (tx, rx) = mpsc::channel::<u16>();
        std::thread::spawn(move || read_driver_output(stdout, &tx));
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
            // 子が先に終わっていたら、ここで回収済みになる。セッション前なので Chrome は居らず、
            // グループへの signal は不要（回収後の PID を signal の宛先にしない）。
            match self.child.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                // 回収済み。宛先にする PID が無いので子を手放す。
                Some(Ok(Some(_))) => {
                    self.child = None;
                    return Err(fail());
                }
                // 状態を取れない。子は残し、呼び出し元の teardown に任せる。
                _ => return Err(fail()),
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(fail)?;
            sleep(Duration::from_millis(100).min(left));
        }
        let client = WebDriverClient::new(port, self.config.command_timeout);
        let (session, browser_version) = client
            .new_session(&new_session_request_with_profile(
                &self.config.allowed_origins,
                Some(&profile),
            ))
            .map_err(|_| fail())?;
        let mut guard = self.handle.live.lock().map_err(|_| fail())?;
        *guard = Some(Live {
            session,
            browser_version,
            unusable: false,
        });
        Ok(())
    }
}

static PROFILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Chrome の一時領域を `/tmp` 直下に所有者だけの権限（0700）で作る。名前は pid・連番・時刻で一意にする（環境変数は読まない。DPOL-32）。
fn make_profile_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = PathBuf::from(format!(
        "/tmp/fandhe-e2e-chrome-{}-{}-{nanos}",
        std::process::id(),
        PROFILE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

/// chromedriver が起動時に出す `ChromeDriver was started successfully on port N` から N を取る。
/// 最初の行の `on port 0`（要求した値）は拾わない。
fn parse_started_port(line: &str) -> Option<u16> {
    let rest = line.split("was started successfully on port ").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u16>().ok().filter(|p| *p != 0)
}

/// 1 行の上限。ポート通知の行は短いので、超える行は読み捨てる（メモリを無制限に使わない）。
const MAX_LINE_BYTES: usize = 1024;

/// ドライバの標準出力を固定サイズの buffer で読み、`started successfully on port N` の行から N を `tx` へ送る。
/// 改行の無い出力を読んでも保持するのは `MAX_LINE_BYTES` までで、超えた行は次の改行まで捨てる。EOF まで読み捨てる。
fn read_driver_output(mut stdout: impl Read, tx: &mpsc::Sender<u16>) {
    let mut chunk = [0u8; 4096];
    let mut line: Vec<u8> = Vec::with_capacity(MAX_LINE_BYTES);
    let mut overflow = false;
    let mut sent = false;
    loop {
        let n = match stdout.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &chunk[..n] {
            if b == b'\n' {
                if !sent
                    && !overflow
                    && let Ok(text) = std::str::from_utf8(&line)
                    && let Some(port) = parse_started_port(text)
                {
                    sent = true;
                    let _ = tx.send(port);
                }
                line.clear();
                overflow = false;
            } else if !overflow {
                if line.len() < MAX_LINE_BYTES {
                    line.push(b);
                } else {
                    overflow = true;
                    line.clear();
                }
            }
        }
    }
}

/// `kill` を 1 回実行する。起動できない・成功以外の終了は誤りにする（結果を捨てない）。
fn send_signal(sig: &str, group: &str) -> std::io::Result<()> {
    let status = Command::new("kill")
        // BSD の kill は `--` を pid として拒否するので使わない。`-KILL -<pgid>` は GNU・BSD の両方で通る。
        .args([sig, group])
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

/// 子のプロセスグループ（chromedriver と配下の Chrome）へ SIGKILL を送り、そのあとで子を wait する。
/// 子を回収する前に signal を送るので、グループ ID（= 子の PID）は signal を送り終えるまで再利用されない
/// （回収済みの PID を宛先にしない）。Chrome の一時領域は回収後にハーネスが消すため、SIGTERM の猶予は置かない。
/// signal の失敗は子を回収したうえで誤りとして返す。
fn kill_group_and_reap(child: &mut Child) -> std::io::Result<()> {
    let group = format!("-{}", child.id());
    let sent = send_signal("-KILL", &group);
    // グループ全体が送れなくても、子自身は止めて回収する。
    let _ = child.kill();
    let waited = child.wait().map(|_| ());
    sent.and(waited)
}

impl E2eComponent for ChromeDriverComponent {
    fn kind(&self) -> E2eComponentKind {
        E2eComponentKind::BrowserDriver
    }

    fn start(&mut self) -> Result<(), E2eError> {
        if self.child.is_some() {
            return Err(err(E2eErrorKind::StartFailed));
        }
        // 前回の一時領域の削除に失敗して残っていれば、再試行して消えるまで起動しない（パスを上書きして取りこぼさない）。
        if self.profile_dir.is_some() && self.teardown().is_err() {
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
            "http://localhost:80",
            "http://localhost:08080",
            "http://127.0.0.1:080",
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
    fn driver_output_without_newline_is_bounded_and_port_line_is_found() {
        let (tx, rx) = mpsc::channel();
        // 改行の無い大きな出力のあとに、通知の行が来る。
        let mut data = vec![b'x'; 5 * 1024 * 1024];
        data.extend_from_slice(b"\nChromeDriver was started successfully on port 4242.\n");
        read_driver_output(std::io::Cursor::new(data), &tx);
        assert_eq!(rx.try_recv().ok(), Some(4242));
        // 上限を超えた行の中の通知は拾わない。
        let (tx, rx) = mpsc::channel();
        let mut long = b"ChromeDriver was started successfully on port 1.".to_vec();
        long.extend(vec![b'y'; MAX_LINE_BYTES]);
        long.push(b'\n');
        read_driver_output(std::io::Cursor::new(long), &tx);
        assert!(rx.try_recv().is_err());
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
