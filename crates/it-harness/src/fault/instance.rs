//! インスタンスの停止（`SIGTERM`・`SIGKILL`）の注入（TASK-13 の 3/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: ハーネスが起動した子プロセスへ signal を送り、送る直前に 1/8 の [`FaultTimeline`] へ `Applied` を記録する。
//! 停止の時刻は、この `Applied` の `at`（壁時計）と `offset_ms`（単調時計）である。呼び出し元は結合試験（試験ビルド）
//! だけ。呼び出し先は [`super::record`]（記録）。AVLB-33 の「再起動の後に二重の発行と取りこぼしが 0」の試験は
//! Hub `ARUN-76` が持ち、ここには無い。in-process の Host（TASK-716）への当て方も無い
//! （TODO(TASK-716): Host がプロセスとして立つ形が決まったら当て方を決める）。
//!
//! 設計上の約束:
//! - 対象はハーネスが所有する未回収の [`Child`] だけ。生の pid・プロセス名を受ける口を作らない。回収した後は送らない
//!   （pid の再利用による誤爆を防ぐ）。
//! - `SIGKILL` は `Child::kill`。`SIGTERM` は固定の絶対パス `/bin/kill` を、シェルも PATH も経由せず、環境を空にして
//!   起動して送る（`unsafe` と依存の追加を避けるため）。`/bin/kill` が無いときは記録の前に止まる（fail-closed）。
//! - 記録が成功してから送る。基準線の `FaultTimeline` では記録が拒否し、signal も送らない。
//!   記録の後に送信が失敗したら [`InstanceFaultError::SignalFailedAfterRecord`] を返す。そのときの記録は
//!   その実行の正しい記録として使わない（`FaultTimeline` に事象を取り消す口は無い）。
//! - 注入の呼び出しは待たない。終了の観察は [`InstanceFaultHandle::wait_exit`] で、記録しない。
//! - 再起動は `Released` にしない（一回きりの事象）。呼び出し元が新しいハンドルを作る。
//! - 誤りと `Debug` に pid・コマンド行・環境変数・`io::Error` の文言を載せない（SECR-11・SECR-23・SECR-26）。
//! - 環境変数・設定ファイル・feature で切り替える経路を作らない（DPOL-30〜32）。
//! - 刻み 10 ms は開発の値で要件の値ではない（THRT-29）。
//!
//! Unix 限定。名前・形・`/bin/kill` を使う方式は仮置きで、人が確認する。

use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{FaultKind, FaultPhase, FaultRecordError, FaultTimeline};

/// 終了を見る刻み（開発の値）。
const WAIT_TICK: Duration = Duration::from_millis(10);
/// `SIGTERM` を送る固定のパス。
const KILL_BIN: &str = "/bin/kill";

/// 誤りの種類。pid・コマンド・`io::Error` の文言を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceFaultError {
    /// 子プロセスの起動に失敗した。
    SpawnFailed,
    /// すでに終了している（回収済み）。記録も送信もしていない。
    NotRunning,
    /// `SIGTERM` を送る手段（`/bin/kill`）が無い。記録も送信もしていない。
    SignalUnavailable,
    /// 記録の後に送信が失敗した。この記録をその実行の正しい記録として使わない。
    SignalFailedAfterRecord,
    /// 上限までに終了しなかった。
    ExitTimeout,
    /// 終了の確認に失敗した。
    WaitFailed,
    /// 記録が拒否した（基準線である等）。signal は送っていない。
    Record(FaultRecordError),
}

impl fmt::Display for InstanceFaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnFailed => f.write_str("instance fault error: spawn failed"),
            Self::NotRunning => f.write_str("instance fault error: not running"),
            Self::SignalUnavailable => f.write_str("instance fault error: signal unavailable"),
            Self::SignalFailedAfterRecord => {
                f.write_str("instance fault error: signal failed after record")
            }
            Self::ExitTimeout => f.write_str("instance fault error: exit timeout"),
            Self::WaitFailed => f.write_str("instance fault error: wait failed"),
            Self::Record(e) => write!(f, "instance fault error: {e}"),
        }
    }
}

impl std::error::Error for InstanceFaultError {}

/// 終了の観察。記録には入れない（ACPT-7: 事実の出力で判定ではない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceExit {
    /// signal で終わったときの signal 番号。
    pub signal: Option<i32>,
    /// 通常終了のときの終了コード。
    pub code: Option<i32>,
    /// 最後に signal を送ってからの経過。送っていなければ 0。
    pub since_signal: Duration,
}

/// ハーネスが所有する子プロセスへの停止の注入口。`Drop` で未回収なら kill して回収する（注入ではないので記録しない）。
pub struct InstanceFaultHandle {
    child: Child,
    exited: bool,
    signaled_at: Option<Instant>,
}

impl fmt::Debug for InstanceFaultHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstanceFaultHandle")
            .field("exited", &self.exited)
            .finish()
    }
}

impl InstanceFaultHandle {
    /// 子プロセスを起動して所有する。`stdin` は閉じる。
    pub fn spawn(cmd: &mut Command) -> Result<Self, InstanceFaultError> {
        let child = cmd
            .stdin(Stdio::null())
            .spawn()
            .map_err(|_| InstanceFaultError::SpawnFailed)?;
        Ok(Self::adopt(child))
    }

    /// 起動済みの子プロセスの所有を受ける。
    pub fn adopt(child: Child) -> Self {
        Self {
            child,
            exited: false,
            signaled_at: None,
        }
    }

    /// 未回収かを確かめる。回収済みなら状態を移して `NotRunning`。
    fn ensure_running(&mut self) -> Result<(), InstanceFaultError> {
        if self.exited {
            return Err(InstanceFaultError::NotRunning);
        }
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(_)) => {
                self.exited = true;
                Err(InstanceFaultError::NotRunning)
            }
            Err(_) => Err(InstanceFaultError::WaitFailed),
        }
    }

    /// `SIGTERM` を送る。待たない。
    pub fn sigterm(&mut self, timeline: &mut FaultTimeline) -> Result<(), InstanceFaultError> {
        self.ensure_running()?;
        if !Path::new(KILL_BIN).is_file() {
            return Err(InstanceFaultError::SignalUnavailable);
        }
        timeline
            .record_now(FaultKind::InstanceSigterm, FaultPhase::Applied)
            .map_err(InstanceFaultError::Record)?;
        self.signaled_at = Some(Instant::now());
        let ok = Command::new(KILL_BIN)
            .env_clear()
            .args(["-s", "TERM", &self.child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            Ok(())
        } else {
            Err(InstanceFaultError::SignalFailedAfterRecord)
        }
    }

    /// `SIGKILL` を送る。待たない。
    pub fn sigkill(&mut self, timeline: &mut FaultTimeline) -> Result<(), InstanceFaultError> {
        self.ensure_running()?;
        timeline
            .record_now(FaultKind::InstanceSigkill, FaultPhase::Applied)
            .map_err(InstanceFaultError::Record)?;
        self.signaled_at = Some(Instant::now());
        self.child
            .kill()
            .map_err(|_| InstanceFaultError::SignalFailedAfterRecord)
    }

    /// 終了を `timeout` まで待って観察を返す。記録しない。
    pub fn wait_exit(&mut self, timeout: Duration) -> Result<InstanceExit, InstanceFaultError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exited = true;
                    return Ok(InstanceExit {
                        signal: status.signal(),
                        code: status.code(),
                        since_signal: self.signaled_at.map_or(Duration::ZERO, |t| t.elapsed()),
                    });
                }
                Ok(None) => {}
                Err(_) => return Err(InstanceFaultError::WaitFailed),
            }
            if Instant::now() >= deadline {
                return Err(InstanceFaultError::ExitTimeout);
            }
            thread::sleep(WAIT_TICK);
        }
    }

    /// 後始末で止める（記録しない。注入ではない）。
    pub fn shutdown(self) {
        drop(self);
    }
}

impl Drop for InstanceFaultHandle {
    fn drop(&mut self) {
        if !self.exited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_shows_only_state() {
        let h = InstanceFaultHandle::spawn(Command::new("sleep").arg("30")).unwrap();
        assert_eq!(format!("{h:?}"), "InstanceFaultHandle { exited: false }");
    }

    #[test]
    fn error_display_is_static() {
        let s = InstanceFaultError::Record(FaultRecordError::EventsInBaseline).to_string();
        assert!(s.starts_with("instance fault error:"));
    }
}
