//! 実行の記録の形式（TASK-11 5/6）。ブラウザの版・authenticator の種類・実行の回数・「仮想は実機・Safari に数えない」印を出す。
//!
//! 役割: E2E の試験（呼び出し元。`tests/support/passkey.rs` の `RecordSink`）が 1 つのブラウザのセッションで行った
//! 登録・認証の結果を、決定的な 1 行 1 項目の `key: value`（UTF-8・LF）にして出す。it-harness の `RunRecord` と同じ作りで、
//! 形式の識別子に版を持つ。呼び出し先は無い（ファイルを書くのは [`E2eRunRecord::write_new`] だけ）。
//!
//! 記録に出すのは列挙の名前・数値・定数だけ。URL・ポート・セッション ID・authenticator ID・credential ID・署名・パスが
//! 入る経路は作らない（SECR-11・SECR-23）。ブラウザの版は検査した主番号の数値だけで、ドライバが返した文字列は保持しない。
//! 記録は実行の事実の出力で、合否の判定でも spec の「確定」でもない。結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。
//! 形式の識別子・キー名・文言は仮置きで、人が確認する。

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::browser::{AuthenticatorKind, Browser, Evidence};
use crate::harness::{E2eError, E2eErrorKind};
use crate::scope::PasskeyOperation;
use crate::webauthn_error::WebAuthnErrorName;

/// 形式の識別子。形式を変えるときは末尾の版を上げる。
pub const RECORD_FORMAT: &str = "fandhe-auth-e2e-run-record/1";

/// 1 つの記録に入れる実行の件数の上限。ハーネスの安全上の値で、要件の値ではない。
pub const MAX_RUNS: usize = 10_000;

/// ブラウザの版。記録に出すのは主番号だけで、ドライバが返した文字列は保持しない。
///
/// 数字だけで最大 4 桁の値なので、アドレスにもセッション ID にもならない（SECR-23）。
/// 解釈できない値は [`BrowserVersion::Unknown`] に落とす（fail-closed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserVersion {
    /// 主番号（1 以上 9999 以下）。
    Known {
        /// 主番号。
        major: u16,
    },
    /// 取得できない、または解釈できない。
    Unknown,
}

impl BrowserVersion {
    /// ドライバが返した `browserVersion` を解釈する。呼び出し元は `WebDriverClient::new_session` で、
    /// セッション ID が手元にあるここで検査する。
    ///
    /// 次のどれかなら `Unknown`: 空・32 バイト超・`[0-9a-z.]` 以外を含む・先頭が「1〜4 桁の数字 + `.`」でない・
    /// 主番号が 0・全体または主番号の文字列がセッション ID と等しい。
    pub(crate) fn from_driver(raw: &str, session_id: &str) -> Self {
        if raw.is_empty()
            || raw.len() > 32
            || !raw
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'.')
        {
            return Self::Unknown;
        }
        let Some((head, _)) = raw.split_once('.') else {
            return Self::Unknown;
        };
        if head.is_empty() || head.len() > 4 || !head.bytes().all(|b| b.is_ascii_digit()) {
            return Self::Unknown;
        }
        if raw == session_id || head == session_id {
            return Self::Unknown;
        }
        match head.parse::<u16>() {
            Ok(major) if major > 0 => Self::Known { major },
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for BrowserVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known { major } => write!(f, "{major}"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// 1 回の実行の結果。種類だけを持ち、ドライバの文字列・ID・署名を持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    /// 成功。
    Succeeded,
    /// ブラウザが `DOMException` の名前つきで拒否した。
    Rejected(WebAuthnErrorName),
    /// 成功ではなく、失敗の名前も無い（ページ内の検査が成り立たなかった）。
    CheckFailed,
    /// ハーネス側の誤り（ドライバの命令の失敗等）。
    HarnessError(E2eErrorKind),
}

impl RunResult {
    fn classify(succeeded: bool, name: Option<WebAuthnErrorName>) -> Self {
        if succeeded {
            Self::Succeeded
        } else if let Some(n) = name {
            Self::Rejected(n)
        } else {
            Self::CheckFailed
        }
    }

    /// 登録の呼び出しの結果から作る。誤りは種類だけを取る。
    #[cfg(unix)]
    pub fn from_registration(
        r: &Result<crate::registration::RegistrationOutcome, E2eError>,
    ) -> Self {
        match r {
            Ok(o) => Self::classify(o.succeeded(), o.error_name()),
            Err(e) => Self::HarnessError(e.kind()),
        }
    }

    /// 認証の呼び出しの結果から作る。
    #[cfg(unix)]
    pub fn from_authentication(
        r: &Result<crate::authentication::AuthenticationOutcome, E2eError>,
    ) -> Self {
        match r {
            Ok(o) => Self::classify(o.succeeded(), o.error_name()),
            Err(e) => Self::HarnessError(e.kind()),
        }
    }
}

impl fmt::Display for RunResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Succeeded => f.write_str("succeeded"),
            Self::Rejected(n) => write!(f, "rejected:{n:?}"),
            Self::CheckFailed => f.write_str("check_failed"),
            Self::HarnessError(k) => write!(f, "harness_error:{k:?}"),
        }
    }
}

/// 記録の誤り。種類だけを持ち、パス・`io::Error` の文言を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// 書き込みに失敗した（出力先の作成・ファイルの作成・書き込み。既存のファイルがあっても失敗）。
    WriteFailed,
    /// 時計が UNIX epoch より前だった。
    ClockBeforeEpoch,
    /// 実行の件数が上限を超えた。
    TooManyRuns,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WriteFailed => "run record write failed",
            Self::ClockBeforeEpoch => "clock is before the unix epoch",
            Self::TooManyRuns => "too many runs in one record",
        })
    }
}

impl std::error::Error for RecordError {}

/// 1 つのブラウザのセッション（`E2eHarness::start` 1 回）の実行の記録。
///
/// 回数の単位はこのセッションで `register_once`・`authenticate_once` を呼んだ回数で、(操作, 結果) ごとに数える。
/// 試験をまたぐ合算はこの型では行わない（読み手が行う）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eRunRecord {
    browser: Browser,
    version: BrowserVersion,
    authenticator: AuthenticatorKind,
    target_operation: PasskeyOperation,
    completed: bool,
    runs: Vec<(PasskeyOperation, RunResult)>,
}

impl E2eRunRecord {
    /// 記録を作る。`completed` は `false` で始まる（到達しなかった実行を完了と読ませない）。
    pub fn new(
        browser: Browser,
        version: BrowserVersion,
        authenticator: AuthenticatorKind,
        target_operation: PasskeyOperation,
    ) -> Self {
        Self {
            browser,
            version,
            authenticator,
            target_operation,
            completed: false,
            runs: Vec::new(),
        }
    }

    /// 実行 1 件の結果を足す。上限を超えたら `TooManyRuns`。
    pub fn record(&mut self, op: PasskeyOperation, result: RunResult) -> Result<(), RecordError> {
        if self.runs.len() >= MAX_RUNS {
            return Err(RecordError::TooManyRuns);
        }
        self.runs.push((op, result));
        Ok(())
    }

    /// 試験が最後まで到達したことを記す。
    pub fn mark_completed(&mut self) {
        self.completed = true;
    }

    /// 記録した実行の件数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 証拠の種類。authenticator の種類から導き、外から上書きする口は無い（ACPT-7・ACPT-8）。
    pub fn evidence(&self) -> Evidence {
        self.authenticator.evidence()
    }

    /// 新しいファイルとして `dir` に書き、書いたパスを返す。既存のファイルは上書きしない。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, RecordError> {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RecordError::ClockBeforeEpoch)?
            .as_secs();
        fs::create_dir_all(dir).map_err(|_| RecordError::WriteFailed)?;
        let name = format!(
            "e2e-run-{}-{}-{}-{}.txt",
            browser_key(self.browser),
            secs,
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = dir.join(name);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| RecordError::WriteFailed)?;
        f.write_all(self.to_string().as_bytes())
            .map_err(|_| RecordError::WriteFailed)?;
        Ok(path)
    }
}

fn browser_key(b: Browser) -> &'static str {
    match b {
        Browser::Chrome => "chrome",
        Browser::Firefox => "firefox",
    }
}

fn op_key(o: PasskeyOperation) -> &'static str {
    match o {
        PasskeyOperation::Registration => "Registration",
        PasskeyOperation::Authentication => "Authentication",
    }
}

impl fmt::Display for E2eRunRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ev = self.authenticator.evidence();
        writeln!(f, "format: {RECORD_FORMAT}")?;
        writeln!(f, "harness_version: {}", env!("CARGO_PKG_VERSION"))?;
        writeln!(f, "browser: {:?}", self.browser)?;
        writeln!(f, "browser_version_major: {}", self.version)?;
        writeln!(f, "authenticator: {:?}", self.authenticator)?;
        writeln!(f, "evidence: {ev:?}")?;
        writeln!(f, "counts_as_real_device: {}", ev.counts_as_real_device())?;
        writeln!(f, "counts_as_safari: {}", ev.counts_as_safari())?;
        writeln!(f, "target_operation: {}", op_key(self.target_operation))?;
        writeln!(f, "completed: {}", self.completed)?;
        for (i, (op, r)) in self.runs.iter().enumerate() {
            writeln!(f, "run: {} {} {}", i + 1, op_key(*op), r)?;
        }
        for op in PasskeyOperation::ALL {
            let of = |pred: fn(&RunResult) -> bool| {
                self.runs
                    .iter()
                    .filter(|(o, r)| *o == op && pred(r))
                    .count()
            };
            let total = self.runs.iter().filter(|(o, _)| *o == op).count();
            writeln!(
                f,
                "count: {} runs={} succeeded={} rejected={} check_failed={} harness_error={}",
                op_key(op),
                total,
                of(|r| matches!(r, RunResult::Succeeded)),
                of(|r| matches!(r, RunResult::Rejected(_))),
                of(|r| matches!(r, RunResult::CheckFailed)),
                of(|r| matches!(r, RunResult::HarnessError(_))),
            )?;
        }
        writeln!(
            f,
            "note: virtual authenticator results; not counted as real-device or Safari results (ACPT-7, ACPT-8)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str, sid: &str) -> BrowserVersion {
        BrowserVersion::from_driver(raw, sid)
    }

    #[test]
    fn version_table() {
        let k = |major| BrowserVersion::Known { major };
        assert_eq!(v("157.0.7234.5", "s"), k(157));
        assert_eq!(v("158.0", "s"), k(158));
        assert_eq!(v("158.0a1", "s"), k(158));
        assert_eq!(v("128.5.0esr", "s"), k(128));
        assert_eq!(v("1.2.3", "s"), k(1));
        assert_eq!(v("10.0.0.5", "s"), k(10));
        for bad in [
            "",
            "1234567890",
            "a/../b",
            "0.1",
            "12345.0",
            "abc.1",
            "1",
            ".1",
            "1.2 ",
            "1.2\n3",
            "157.0.7234.5.1.1.1.1.1.1.1.1.1.1.1.1",
        ] {
            assert_eq!(v(bad, "s"), BrowserVersion::Unknown, "{bad:?}");
        }
        assert_eq!(v("1234567890", "1234567890"), BrowserVersion::Unknown);
        assert_eq!(v("1234.5", "1234"), BrowserVersion::Unknown);
        assert_eq!(v("1234.5", "1234.5"), BrowserVersion::Unknown);
    }

    #[test]
    fn version_display_has_no_dotted_quad() {
        assert_eq!(v("10.0.0.5", "s").to_string(), "10");
        assert_eq!(format!("{:?}", v("10.0.0.5", "s")), "Known { major: 10 }");
        assert_eq!(BrowserVersion::Unknown.to_string(), "unknown");
    }
}
