//! 結合試験の実行の記録（版・構成・日時）の型と、ファイルへの出力（TASK-10 の 5/6）。
//!
//! 役割: 結合試験が「いつ・どの版で・どの構成で流れたか」を読み手が確かめられるように残す。呼び出し元は
//! 結合試験（試験ビルド）だけ。記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。
//!
//! 設計上の約束:
//! - 値を持たない部品だけで組む: 種類・stub の印・静的な欄名・数値の時刻・検査済みの版の文字列。
//!   設定の値・接続情報・ポート・compose project 名・パス・ホスト名・パスワードの入る経路を作らない
//!   （SECR-11・SECR-23・SECR-26）。
//! - 実行時に外から入る文字列（git の revision・rustc の版）は文字種と長さで検査し、外れたら拒否する
//!   （fail-closed。行の注入を防ぐ）。環境変数・設定ファイルは読まない（DPOL-32）。
//! - 起動していない構成要素も `not_started` で明示し、Host を試したと読まれないようにする
//!   （TASK-716・ACPT-7）。stub を含むときは実物の合格に数えない旨の行を残す（A-2026-10-28-9）。
//! - 形式の識別子・キー名・文言は仮置きで、人が確認する。

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fandhe_auth_console_stub::contract::ContractRecord;
use fandhe_auth_hub_stub::config::ProvisionalRecord;

use crate::component::ComponentKind;
use crate::harness::CompositionRecord;

/// 記録の形式の識別子（仮置き）。形式を変えたら末尾の版を上げる。
pub const RECORD_FORMAT: &str = "fandhe-auth-it-run-record/1";

/// 誤りの種類。パス・コマンドの出力・`io::Error` の文言を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// 時刻が epoch より前。
    ClockBeforeEpoch,
    /// 版を取るコマンドが実行できない・失敗した。
    VersionCommandFailed { field: &'static str },
    /// 版の文字列が文字種・長さの検査を満たさない。
    InvalidVersionField { field: &'static str },
    /// ファイルの書き込みに失敗した。
    WriteFailed,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClockBeforeEpoch => f.write_str("run record error: clock is before the epoch"),
            Self::VersionCommandFailed { field } => {
                write!(f, "run record error: version command failed ({field})")
            }
            Self::InvalidVersionField { field } => {
                write!(f, "run record error: invalid version field ({field})")
            }
            Self::WriteFailed => f.write_str("run record error: write failed"),
        }
    }
}

impl std::error::Error for RecordError {}

/// 秒単位の UTC 時刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcTimestamp(u64);

impl UtcTimestamp {
    /// epoch からの秒数から作る。
    pub fn from_unix_seconds(secs: u64) -> Self {
        Self(secs)
    }

    /// システム時計から作る。epoch より前は拒否する。
    pub fn from_system_time(t: SystemTime) -> Result<Self, RecordError> {
        t.duration_since(UNIX_EPOCH)
            .map(|d| Self(d.as_secs()))
            .map_err(|_| RecordError::ClockBeforeEpoch)
    }

    /// epoch からの秒数。
    pub fn unix_seconds(self) -> u64 {
        self.0
    }

    /// 日数（1970-01-01 起点）から (年, 月, 日) へ。proleptic gregorian の civil 変換。
    fn civil(days: u64) -> (u64, u64, u64) {
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + u64::from(m <= 2);
        (y, m, d)
    }
}

impl fmt::Display for UtcTimestamp {
    /// RFC 3339 の `YYYY-MM-DDTHH:MM:SSZ`。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = Self::civil(self.0 / 86_400);
        let s = self.0 % 86_400;
        write!(
            f,
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            s / 3_600,
            (s % 3_600) / 60,
            s % 60
        )
    }
}

/// 試験した版。欄の選び方は書き手の読みで、人が確認する（spec に「版」の中身の規定は見当たらない）。
/// 値はすべて検査済み。パス・ファイル名・ホスト名・利用者名を持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    harness_version: String,
    source_revision: String,
    source_dirty: bool,
    spec_revision: String,
    rustc: String,
}

fn is_hex_revision(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_plain_text(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .()+-_".contains(&b))
}

fn invalid(field: &'static str) -> RecordError {
    RecordError::InvalidVersionField { field }
}

impl VersionInfo {
    /// 検査を通して作る。決定的な入力（試験）と `detect` の共通の入口。
    pub fn new(
        harness_version: &str,
        source_revision: &str,
        source_dirty: bool,
        spec_revision: &str,
        rustc: &str,
    ) -> Result<Self, RecordError> {
        if !is_plain_text(harness_version, 64) {
            return Err(invalid("harness_version"));
        }
        if !is_hex_revision(source_revision) {
            return Err(invalid("source_revision"));
        }
        if !is_hex_revision(spec_revision) {
            return Err(invalid("spec_revision"));
        }
        if !is_plain_text(rustc, 128) {
            return Err(invalid("rustc"));
        }
        Ok(Self {
            harness_version: harness_version.to_owned(),
            source_revision: source_revision.to_owned(),
            source_dirty,
            spec_revision: spec_revision.to_owned(),
            rustc: rustc.to_owned(),
        })
    }

    /// `git`・`rustc` を子プロセスで呼んで取る。シェルを通さず固定の引数だけを渡す。
    /// 失敗・検査外は誤り（黙って省かない）。リポジトリのパスは `-C` の引数にだけ使い、記録・誤りに載せない。
    pub fn detect() -> Result<Self, RecordError> {
        let repo = env!("CARGO_MANIFEST_DIR");
        let rev = run("source_revision", "git", &["-C", repo, "rev-parse", "HEAD"])?;
        let status = run(
            "source_dirty",
            "git",
            &["-C", repo, "status", "--porcelain"],
        )?;
        // ファイル名は記録しない。汚れの有無だけ。
        let dirty = !status.trim().is_empty();
        let spec = run(
            "spec_revision",
            "git",
            &["-C", repo, "rev-parse", "HEAD:docs/spec"],
        )?;
        let rustc = run("rustc", "rustc", &["--version"])?;
        Self::new(
            env!("CARGO_PKG_VERSION"),
            rev.trim(),
            dirty,
            spec.trim(),
            rustc.trim(),
        )
    }
}

/// 子プロセスを実行して標準出力を返す。出力・エラーの文言は誤りに載せない。
fn run(field: &'static str, program: &str, args: &[&str]) -> Result<String, RecordError> {
    let failed = || RecordError::VersionCommandFailed { field };
    let out = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|_| failed())?;
    if !out.status.success() {
        return Err(failed());
    }
    String::from_utf8(out.stdout).map_err(|_| failed())
}

/// 実行の記録。値を持たない部品だけで組む（モジュールの doc）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    started_at: UtcTimestamp,
    recorded_at: UtcTimestamp,
    version: VersionInfo,
    composition: CompositionRecord,
    hub_stub_lines: Vec<String>,
    console_contract_lines: Vec<String>,
}

impl RunRecord {
    /// 起動時刻・記録時刻・版・構成から作る。
    pub fn new(
        started_at: UtcTimestamp,
        recorded_at: UtcTimestamp,
        version: VersionInfo,
        composition: CompositionRecord,
    ) -> Self {
        Self {
            started_at,
            recorded_at,
            version,
            composition,
            hub_stub_lines: Vec::new(),
            console_contract_lines: Vec::new(),
        }
    }

    /// Hub の stub の「仮置き」の印の記録を取り込む。欄名と印だけで、値は無い。
    #[must_use]
    pub fn with_hub_stub_record(mut self, r: &ProvisionalRecord) -> Self {
        self.hub_stub_lines = r
            .entries
            .iter()
            .map(|e| format!("hub_stub_field: {} {}", e.field, e.mark))
            .collect();
        self
    }

    /// console の stub の契約の記録を取り込む。項目名・印・根拠 ID だけで、値は無い。
    #[must_use]
    pub fn with_console_contract(mut self, r: &ContractRecord) -> Self {
        self.console_contract_lines = r
            .entries
            .iter()
            .map(|e| {
                format!(
                    "console_contract: {} {} [{}]",
                    e.item.name(),
                    e.mark,
                    e.refs.join(",")
                )
            })
            .collect();
        self
    }

    /// 起動時刻。
    pub fn started_at(&self) -> UtcTimestamp {
        self.started_at
    }

    /// 新規のファイルへ書き出す。`dir` は呼び出し元が渡し、無ければ作る。既存のファイルは上書きしない。
    /// ファイル名は起動時刻・pid・通し番号から作る（乱数を使わない）。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, RecordError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir).map_err(|_| RecordError::WriteFailed)?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "it-run-{}-{}-{n}.txt",
            self.started_at.unix_seconds(),
            std::process::id()
        );
        let path = dir.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| RecordError::WriteFailed)?;
        file.write_all(self.to_string().as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| RecordError::WriteFailed)?;
        Ok(path)
    }
}

impl fmt::Display for RunRecord {
    /// 1 行 1 項目の `key: value`（UTF-8・LF）。キーは ASCII 固定。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "format: {RECORD_FORMAT}")?;
        writeln!(f, "started_at: {}", self.started_at)?;
        writeln!(f, "recorded_at: {}", self.recorded_at)?;
        writeln!(f, "harness_version: {}", self.version.harness_version)?;
        writeln!(f, "source_revision: {}", self.version.source_revision)?;
        writeln!(f, "source_dirty: {}", self.version.source_dirty)?;
        writeln!(f, "spec_revision: {}", self.version.spec_revision)?;
        writeln!(f, "rustc: {}", self.version.rustc)?;
        let mut any_stub = false;
        for kind in ComponentKind::START_ORDER {
            match self.composition.entries.iter().find(|e| e.kind == kind) {
                Some(e) if e.stub => {
                    any_stub = true;
                    writeln!(f, "component: {kind:?} started stub")?;
                }
                Some(_) => writeln!(f, "component: {kind:?} started real")?,
                None => writeln!(f, "component: {kind:?} not_started")?,
            }
        }
        if any_stub {
            writeln!(
                f,
                "note: stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）"
            )?;
        }
        for l in self
            .hub_stub_lines
            .iter()
            .chain(&self.console_contract_lines)
        {
            writeln!(f, "{l}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::CompositionEntry;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    fn ver() -> VersionInfo {
        VersionInfo::new("0.1.0", REV, false, REV, "rustc 1.90.0 (abc 2026-01-01)").unwrap()
    }

    fn comp(kinds: &[ComponentKind]) -> CompositionRecord {
        CompositionRecord {
            entries: kinds
                .iter()
                .map(|k| CompositionEntry {
                    kind: *k,
                    stub: k.is_stub(),
                })
                .collect(),
        }
    }

    fn ts(s: u64) -> UtcTimestamp {
        UtcTimestamp::from_unix_seconds(s)
    }

    #[test]
    fn timestamp_known_values() {
        assert_eq!(ts(0).to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(ts(951_782_400).to_string(), "2000-02-29T00:00:00Z");
        assert_eq!(ts(951_868_799).to_string(), "2000-02-29T23:59:59Z");
        assert_eq!(ts(4_107_542_400).to_string(), "2100-03-01T00:00:00Z");
        assert_eq!(ts(1_700_000_000).to_string(), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn timestamp_rejects_before_epoch() {
        let t = UNIX_EPOCH - std::time::Duration::from_secs(1);
        assert_eq!(
            UtcTimestamp::from_system_time(t),
            Err(RecordError::ClockBeforeEpoch)
        );
    }

    #[test]
    fn version_rejects_bad_fields() {
        let ok = "rustc 1.90.0";
        assert!(VersionInfo::new("0.1.0", "xyz", false, REV, ok).is_err());
        assert!(VersionInfo::new("0.1.0", REV, false, "", ok).is_err());
        assert!(VersionInfo::new("0.1.0", &format!("{REV}\n"), false, REV, ok).is_err());
        assert!(VersionInfo::new("0.1.0", REV, false, REV, "rustc\nformat: x").is_err());
        assert!(VersionInfo::new("0.1.0", REV, false, REV, &"a".repeat(129)).is_err());
        assert!(VersionInfo::new("", REV, false, REV, ok).is_err());
        assert!(VersionInfo::new("0.1.0", REV, false, REV, ok).is_ok());
    }

    #[test]
    fn display_is_deterministic_and_marks_not_started() {
        let r = RunRecord::new(
            ts(0),
            ts(60),
            ver(),
            comp(&[ComponentKind::HubStub, ComponentKind::ConsoleStub]),
        );
        let expect = [
            format!("format: {RECORD_FORMAT}"),
            "started_at: 1970-01-01T00:00:00Z".to_owned(),
            "recorded_at: 1970-01-01T00:01:00Z".to_owned(),
            "harness_version: 0.1.0".to_owned(),
            format!("source_revision: {REV}"),
            "source_dirty: false".to_owned(),
            format!("spec_revision: {REV}"),
            "rustc: rustc 1.90.0 (abc 2026-01-01)".to_owned(),
            "component: Database not_started".to_owned(),
            "component: Host not_started".to_owned(),
            "component: HubStub started stub".to_owned(),
            "component: ConsoleStub started stub".to_owned(),
            "note: stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）".to_owned(),
        ]
        .join("\n")
            + "\n";
        assert_eq!(r.to_string(), expect);
    }

    #[test]
    fn no_note_without_stub() {
        let r = RunRecord::new(ts(0), ts(0), ver(), comp(&[ComponentKind::Database]));
        let s = r.to_string();
        assert!(s.contains("component: Database started real\n"));
        assert!(!s.contains("note:"));
    }
}
