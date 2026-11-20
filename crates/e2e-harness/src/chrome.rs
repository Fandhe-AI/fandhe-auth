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
//! unix 限定（`process_group` と `kill -s KILL -- -<pgid>`（`sh` の組み込み）を使う）。Linux・macOS で同じ手順で動く。
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

    /// 非同期スクリプトを実行する。応答は生の `Value` のため crate の外へ公開しない。
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
            // 送信に失敗したら配下の終了を確かめられないので、`StopFailed` にする（グループが既に無い場合は成功）。
            if kill_group(child.id()).is_err() {
                // グループへ送れなかった。子（グループの先頭）は回収せず所有を保ち、stop()・Drop で再試行できるようにする
                // （回収するとグループ ID を失い、残った配下を止められないまま次の stop() が Ok を返してしまう）。
                // 先頭は止めておく（止めるだけで回収しない）。
                let _ = child.kill();
                self.child = Some(child);
                return Err(err(E2eErrorKind::StopFailed));
            }
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
            let ready = probe.is_ready();
            // ready を受理する前に子の生存を確かめる。子が終了していたら、応答したのは別のプロセスの可能性があり、
            // 起動の失敗にする（ポートの所有の完全な照合ではない。試験環境の上の攻撃者の仮定は P2 以下）。
            // 状態の確認（`ps`）にも起動待ちの期限を渡す。
            let state_limit = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::ZERO);
            match self.child.as_mut().map(|c| child_state(c, state_limit)) {
                Some(ChildState::Alive) => {
                    // ready を受け付ける直前にも期限を確かめる。
                    if ready && Instant::now() < deadline {
                        break;
                    }
                }
                // 終了しているが未回収（zombie）。所有を保ったまま返し、呼び出し元（`start`）の teardown が
                // 回収の前にグループへ送って止める。止められなければ所有を保ち、後の stop・Drop で再試行できる。
                Some(ChildState::ExitedUnreaped) => return Err(fail()),
                // `ps` で状態を取れない。子は回収しないまま返し、teardown がグループを止めてから回収する。
                Some(ChildState::Unknown) | None => return Err(fail()),
            }
            // 残り時間は probe・try_wait の後で計算し直す（古い `left` でスリープして期限を超えない）。
            let Some(left_now) = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
            else {
                return Err(fail());
            };
            sleep(Duration::from_millis(100).min(left_now));
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

/// 子の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildState {
    /// 動いている。
    Alive,
    /// 終了したが回収していない（zombie）。グループ ID はまだ保たれている。
    ExitedUnreaped,
    /// `ps` で見分けられない（期限切れ・実行失敗）。子は回収していない。
    Unknown,
}

/// 子を回収せずに状態を見る。`ps` の状態（`Z` は zombie）で見分け、見分けられなければ `Unknown`（回収しない）。
/// 回収の前にグループへ送れるようにするため（回収すると、再利用され得る PGID へ送ることになる）。
/// `limit` は `ps` に許す時間の上限（呼び出し元の期限の残りと `KILL_TIMEOUT` の短い方）。
fn child_state(child: &mut Child, limit: Duration) -> ChildState {
    match ps_stat(child.id(), limit.min(KILL_TIMEOUT)) {
        Some(stat) if stat.starts_with('Z') => ChildState::ExitedUnreaped,
        Some(stat) if !stat.is_empty() => ChildState::Alive,
        _ => ChildState::Unknown,
    }
}

/// `ps -o stat= -p <pid>` の出力（期限付き。無期限に待たない）。
fn ps_stat(pid: u32, limit: Duration) -> Option<String> {
    if limit.is_zero() {
        return None;
    }
    let mut child = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = wait_until(&mut child, KILL_REAP_TIMEOUT);
                return None;
            }
        }
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut text).ok()?;
    Some(text.trim().to_owned())
}

/// `kill` の実行時間の上限。ハーネスの安全上の値で、要件の値ではない。
const KILL_TIMEOUT: Duration = Duration::from_secs(5);

/// 期限切れの `kill` を止めたあと、回収を待つ上限。ハーネスの安全上の値で、要件の値ではない。
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(2);

/// `kill` の結果の分類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillOutcome {
    /// 送れた。
    Sent,
    /// グループが既に無い（ESRCH 相当）。止まっているので成功として扱う。
    Gone,
    /// 送れなかった（権限・実行失敗・期限切れ等）。配下の終了を確かめられない。
    Failed,
}

/// `kill` の終了状態と stderr から結果を分ける。ロケールは `LC_ALL=C` で固定して呼ぶ。
fn classify_kill(success: bool, stderr: &str) -> KillOutcome {
    if success {
        KillOutcome::Sent
    } else if stderr.to_ascii_lowercase().contains("no such process") {
        KillOutcome::Gone
    } else {
        KillOutcome::Failed
    }
}

/// プロセスグループ `pgid` へ `SIGKILL` を送る（`kill -s KILL -- -<pgid>`）。
/// `sh`（`kill`）は spawn して期限付きで待つ（無期限に待たない）。期限を過ぎたら `kill` 自体を止めて失敗にする。
fn kill_group(pgid: u32) -> Result<(), E2eError> {
    match run_kill(pgid) {
        KillOutcome::Sent | KillOutcome::Gone => Ok(()),
        KillOutcome::Failed => Err(err(E2eErrorKind::StopFailed)),
    }
}

fn run_kill(pgid: u32) -> KillOutcome {
    // 外部の `kill` コマンドは実装により終了コードが一定でない（procps は成功でも 1、存在しないグループで 0 を返す）。
    // `sh` の組み込みの `kill` を固定のスクリプトで呼び、pgid は引数で渡す（A03。dash・bash で終了コードと文言が揃う）。
    let Ok(mut child) = Command::new("sh")
        .args(["-c", "kill -s KILL -- \"-$1\"", "sh", &pgid.to_string()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return KillOutcome::Failed;
    };
    let deadline = Instant::now() + KILL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => sleep(Duration::from_millis(10)),
            _ => {
                // 回収にも期限を設ける（`wait()` で無期限に待たない）。回収できなくても失敗として返す。
                let _ = child.kill();
                let _ = wait_until(&mut child, KILL_REAP_TIMEOUT);
                return KillOutcome::Failed;
            }
        }
    };
    // 終了後なので stderr は読み切れる（出力は 1 行程度）。
    let mut text = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut e, &mut text);
    }
    classify_kill(status.success(), &text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_outcome_separates_gone_from_failed() {
        assert_eq!(classify_kill(true, ""), KillOutcome::Sent);
        assert_eq!(
            classify_kill(false, "kill: (123) - No such process"),
            KillOutcome::Gone
        );
        assert_eq!(
            classify_kill(false, "kill: (123) - Operation not permitted"),
            KillOutcome::Failed
        );
        assert_eq!(classify_kill(false, ""), KillOutcome::Failed);
    }

    #[test]
    fn exited_leader_is_seen_unreaped_so_the_group_can_be_killed_before_reaping() {
        // 親が子孫を残して終了する。回収せずに終了を見分け、回収の前にグループへ送って子孫を止められる。
        let mut leader = Command::new("sh")
            .args(["-c", "sleep 30 & exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while child_state(&mut leader, KILL_TIMEOUT) == ChildState::Alive
            && Instant::now() < deadline
        {
            sleep(Duration::from_millis(20));
        }
        assert_eq!(
            child_state(&mut leader, KILL_TIMEOUT),
            ChildState::ExitedUnreaped
        );
        // 未回収なので PGID はまだ自分の子のもの。回収の前に送る。
        assert!(kill_group(leader.id()).is_ok());
        assert!(wait_until(&mut leader, Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn unreadable_state_is_unknown_and_the_child_is_not_reaped() {
        // `ps` に時間を与えない（取れない）場合は Unknown で、子を回収しない（`try_wait` に落とさない）。
        let mut c = Command::new("sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        sleep(Duration::from_millis(200));
        assert_eq!(child_state(&mut c, Duration::ZERO), ChildState::Unknown);
        // 回収していなければ、続く判定で zombie として見える。
        assert_eq!(
            child_state(&mut c, KILL_TIMEOUT),
            ChildState::ExitedUnreaped
        );
        assert!(wait_until(&mut c, Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn kill_group_kills_a_live_group_that_this_test_owns() {
        // 回収済みの PGID へは送らない（PGID の再利用で無関係なグループを止めない）。消失の分岐は
        // `classify_kill` の試験で確かめ、実シグナルは所有中の子のグループだけに送る。
        let mut live = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        assert!(kill_group(live.id()).is_ok());
        assert!(wait_until(&mut live, Duration::from_secs(5)).is_ok());
    }
}
