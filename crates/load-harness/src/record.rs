//! 結果の記録 `fandhe-auth-load-record/1` のファイル全体を組んで書き出す部品（TASK-12 9/10）。
//! 名前・語・上限はすべて仮置きで、人が確認する。合否は付けない（ACPT-7）。
//!
//! 役割: `docs/harness/load.md` の 1・2・6・7 の形式で、メタデータの行・機材と構成（`condition`）・段階（`stage`）・
//! 比べる値（`value:`）・空の `judgment:` を 1 つのファイルにする。呼び出し元は試験ビルドだけ。
//! 呼び出し先は `condition`・`stage` の `Display` と std だけ（通常の依存は 0。it-harness に依存できないため、
//! RFC 3339 の整形は it-harness の `UtcTimestamp` と同じ civil 変換をここに重複して持つ）。
//!
//! 安全: 記録に入る文字列は作る時点で文字種・長さ・語彙を検査し、自由文の欄を作らない。違反は誤りにする
//! （fail-closed。黙って `undecided` に落とさない。DPOL-32）。区分に既定値を置かず、測定値を要件の値へ上げる経路を作らない。
//! `judgment:` は常に空で、ハーネスは合否を書かない（ACPT-7）。ファイル名は整数だけで作り、既存ファイルを上書きしない。
//! 誤りは種類だけを持ち、値・パス・`io::Error` の文言を載せない（SECR-11・SECR-23・SECR-26）。`Debug` は中身を出さない。
//! 他所有者の値は `external` で値なし・出典必須（D-2026-10-15-21）。誤りの文言は英語（D-2026-11-20-13）。

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::condition::MeasurementConditions;
use crate::stage::StagedRun;

/// 形式の識別子。形式を変えたら末尾の版を上げる。
pub const LOAD_RECORD_FORMAT: &str = "fandhe-auth-load-record/1";
/// 1 行の出典の数の上限（仮置き）。
pub const MAX_REFS: usize = 8;
/// 値の行の数の上限（仮置き）。
pub const MAX_VALUES: usize = 64;

/// 記録の誤り。種類だけを持ち、入力の値・パス・元の誤りの文言は載せない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadRecordError {
    InvalidName,
    InvalidRef,
    InvalidRunRecordFile,
    InvalidValue { rule: &'static str },
    TooManyRefs,
    TooManyValues,
    DuplicateValue,
    ClockOrder,
    Format,
    WriteFailed,
}

impl fmt::Display for LoadRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("load record error: invalid name"),
            Self::InvalidRef => f.write_str("load record error: invalid ref"),
            Self::InvalidRunRecordFile => f.write_str("load record error: invalid run record file"),
            Self::InvalidValue { rule } => write!(f, "load record error: invalid value ({rule})"),
            Self::TooManyRefs => f.write_str("load record error: too many refs"),
            Self::TooManyValues => f.write_str("load record error: too many values"),
            Self::DuplicateValue => f.write_str("load record error: duplicate value"),
            Self::ClockOrder => f.write_str("load record error: recorded_at before run_started_at"),
            Self::Format => f.write_str("load record error: format failed"),
            Self::WriteFailed => f.write_str("load record error: write failed"),
        }
    }
}

impl std::error::Error for LoadRecordError {}

/// 値の区分（`load.md` の 2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass {
    Requirement,
    Initial,
    PocParameter,
    TestCondition,
    Measured,
    Undecided,
    External,
}

impl ValueClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requirement => "requirement",
            Self::Initial => "initial",
            Self::PocParameter => "poc_parameter",
            Self::TestCondition => "test_condition",
            Self::Measured => "measured",
            Self::Undecided => "undecided",
            Self::External => "external",
        }
    }
}

/// 単位（`load.md` の 3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueUnit {
    Ns,
    Count,
    PerS,
}

impl ValueUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ns => "ns",
            Self::Count => "count",
            Self::PerS => "per_s",
        }
    }
}

fn is_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 値の名前（`[a-z0-9_]`・1〜64 文字）。
#[derive(Clone, PartialEq, Eq)]
pub struct ValueName(String);

impl ValueName {
    /// # Errors
    /// 規則を満たさないとき [`LoadRecordError::InvalidName`]。
    pub fn new(s: &str) -> Result<Self, LoadRecordError> {
        if is_name(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(LoadRecordError::InvalidName)
        }
    }
}

/// SECR-23・SECR-26: 中身を出さない。
impl fmt::Debug for ValueName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValueName(..)")
    }
}

/// 出典の ID（ASCII 英数字・`-`・`_`・1〜64 文字）。
#[derive(Clone, PartialEq, Eq)]
pub struct RefId(String);

impl RefId {
    /// # Errors
    /// 規則を満たさないとき [`LoadRecordError::InvalidRef`]。
    pub fn new(s: &str) -> Result<Self, LoadRecordError> {
        let ok = (1..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(LoadRecordError::InvalidRef)
        }
    }
}

impl fmt::Debug for RefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefId(..)")
    }
}

/// 実行の記録のファイル名（`it-run-<数字>-<数字>-<数字>.txt`）。
#[derive(Clone, PartialEq, Eq)]
pub struct RunRecordFile(String);

impl RunRecordFile {
    /// # Errors
    /// 形を満たさないとき [`LoadRecordError::InvalidRunRecordFile`]。
    pub fn new(s: &str) -> Result<Self, LoadRecordError> {
        let bad = LoadRecordError::InvalidRunRecordFile;
        if s.len() > 64 {
            return Err(bad);
        }
        let rest = s.strip_prefix("it-run-").ok_or(bad)?;
        let rest = rest.strip_suffix(".txt").ok_or(bad)?;
        let parts: Vec<&str> = rest.split('-').collect();
        let digits = |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        if parts.len() == 3 && parts.iter().all(digits) {
            Ok(Self(s.to_owned()))
        } else {
            Err(bad)
        }
    }
}

impl fmt::Debug for RunRecordFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RunRecordFile(..)")
    }
}

/// 比べる値の 1 行。区分ごとの規則は作る時点で検査する（`load.md` の 2）。
#[derive(Clone, PartialEq, Eq)]
pub struct ComparisonValue {
    name: ValueName,
    class: ValueClass,
    value: Option<u64>,
    unit: ValueUnit,
    refs: Vec<RefId>,
}

impl ComparisonValue {
    /// # Errors
    /// 区分の規則に反するとき [`LoadRecordError::InvalidValue`]、出典が 9 件以上のとき
    /// [`LoadRecordError::TooManyRefs`]、重複のとき [`LoadRecordError::InvalidRef`]。
    pub fn new(
        name: ValueName,
        class: ValueClass,
        value: Option<u64>,
        unit: ValueUnit,
        refs: Vec<RefId>,
    ) -> Result<Self, LoadRecordError> {
        let bad = |rule| LoadRecordError::InvalidValue { rule };
        if refs.len() > MAX_REFS {
            return Err(LoadRecordError::TooManyRefs);
        }
        let uniq: BTreeSet<&str> = refs.iter().map(|r| r.0.as_str()).collect();
        if uniq.len() != refs.len() {
            return Err(LoadRecordError::InvalidRef);
        }
        match class {
            ValueClass::Requirement | ValueClass::Initial | ValueClass::PocParameter => {
                if refs.is_empty() {
                    return Err(bad("refs_required"));
                }
                if value.is_none() {
                    return Err(bad("value_required"));
                }
            }
            ValueClass::Undecided => {
                if value.is_some() {
                    return Err(bad("value_forbidden"));
                }
            }
            ValueClass::External => {
                if value.is_some() {
                    return Err(bad("value_forbidden"));
                }
                if refs.is_empty() {
                    return Err(bad("refs_required"));
                }
            }
            ValueClass::TestCondition | ValueClass::Measured => {
                if value.is_none() {
                    return Err(bad("value_required"));
                }
                if !refs.is_empty() {
                    return Err(bad("refs_forbidden"));
                }
            }
        }
        Ok(Self {
            name,
            class,
            value,
            unit,
            refs,
        })
    }
}

impl fmt::Debug for ComparisonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ComparisonValue(..)")
    }
}

impl fmt::Display for ComparisonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "value: {} class={} value=",
            self.name.0,
            self.class.as_str()
        )?;
        match self.value {
            Some(v) => write!(f, "{v}")?,
            None => f.write_str("none")?,
        }
        write!(f, " unit={} refs=", self.unit.as_str())?;
        match self.class {
            ValueClass::TestCondition => f.write_str("harness_config")?,
            ValueClass::Measured => f.write_str("measured")?,
            _ if self.refs.is_empty() => f.write_str("none")?,
            _ => {
                for (i, r) in self.refs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(&r.0)?;
                }
            }
        }
        Ok(())
    }
}

/// UTC の RFC 3339（秒）。it-harness の `UtcTimestamp` と同じ変換の重複（依存を足さないため）。
fn rfc3339(secs: u64) -> String {
    let z = secs / 86_400 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    let s = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3_600,
        (s % 3_600) / 60,
        s % 60
    )
}

/// 結果の記録のファイル全体。行の順は `load.md` の 7 の見本に合わせる。
#[derive(Clone, PartialEq, Eq)]
pub struct LoadRecord {
    run_started_at: u64,
    text: String,
}

impl LoadRecord {
    /// 作る時点で全体を文字列にして持つ（部分の記録を作らない）。
    ///
    /// # Errors
    /// 時刻の順が逆・値の行が多すぎる・名前の重複・整形の失敗のとき。
    pub fn new(
        run_record_file: &RunRecordFile,
        run_started_at_unix_secs: u64,
        recorded_at_unix_secs: u64,
        conditions: &MeasurementConditions,
        staged: &StagedRun,
        values: Vec<ComparisonValue>,
    ) -> Result<Self, LoadRecordError> {
        if recorded_at_unix_secs < run_started_at_unix_secs {
            return Err(LoadRecordError::ClockOrder);
        }
        if values.len() > MAX_VALUES {
            return Err(LoadRecordError::TooManyValues);
        }
        let names: BTreeSet<&str> = values.iter().map(|v| v.name.0.as_str()).collect();
        if names.len() != values.len() {
            return Err(LoadRecordError::DuplicateValue);
        }
        let fmt_err = |_| LoadRecordError::Format;
        let mut t = String::new();
        writeln!(t, "format: {LOAD_RECORD_FORMAT}").map_err(fmt_err)?;
        writeln!(t, "run_record_file: {}", run_record_file.0).map_err(fmt_err)?;
        writeln!(t, "run_started_at: {}", rfc3339(run_started_at_unix_secs)).map_err(fmt_err)?;
        writeln!(t, "recorded_at: {}", rfc3339(recorded_at_unix_secs)).map_err(fmt_err)?;
        write!(t, "{conditions}").map_err(fmt_err)?;
        write!(t, "{staged}").map_err(fmt_err)?;
        for v in &values {
            writeln!(t, "{v}").map_err(fmt_err)?;
        }
        // ACPT-7: 判定の欄は人が記入する。ハーネスは常に空で出す。
        writeln!(t, "judgment:").map_err(fmt_err)?;
        Ok(Self {
            run_started_at: run_started_at_unix_secs,
            text: t,
        })
    }

    /// 新規のファイルへ書き出す。`dir` は呼び出し元が渡し、無ければ作る。既存のファイルは上書きしない。
    /// 名前は `load-run-<起動時刻の秒>-<pid>-<通し番号>.txt`（乱数を使わない）。
    ///
    /// # Errors
    /// 作成・書き込みに失敗したとき [`LoadRecordError::WriteFailed`]（元の誤りは載せない）。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, LoadRecordError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir).map_err(|_| LoadRecordError::WriteFailed)?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            "load-run-{}-{}-{n}.txt",
            self.run_started_at,
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| LoadRecordError::WriteFailed)?;
        file.write_all(self.text.as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| LoadRecordError::WriteFailed)?;
        Ok(path)
    }
}

impl fmt::Debug for LoadRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoadRecord(..)")
    }
}

impl fmt::Display for LoadRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
