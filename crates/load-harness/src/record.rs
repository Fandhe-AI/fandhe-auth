//! 結果の記録 `fandhe-auth-load-record/1` のファイル全体を組んで書き出す部品（TASK-12 9/10）。
//! 名前・語・上限はすべて仮置きで、人が確認する。合否は付けない（ACPT-7）。
//!
//! 役割: `docs/harness/load.md` の 1・2・6・7 の形式で、メタデータの行・機材と構成（`condition`）・段階（`stage`）・
//! 比べる値（`value:`）・空の `judgment:` を 1 つのファイルにする。呼び出し元は試験ビルドだけ。
//! 呼び出し先は `condition`・`stage` の `Display`、段階の実行（[`RunSession::run`] の中の `stage::run_stages`。
//! 時計・対象の型は `meter`・`target`）と std だけ（通常の依存は 0。it-harness に依存できないため、
//! RFC 3339 の整形は it-harness の `UtcTimestamp` と同じ civil 変換をここに重複して持つ）。
//!
//! 安全: 記録に入る文字列は作る時点で文字種・長さ・語彙を検査し、自由文の欄を作らない。違反は誤りにする
//! （fail-closed。黙って `undecided` に落とさない。DPOL-32）。区分に既定値を置かず、ハーネスの中に測定値を要件の値へ上げる経路を作らない
//! （要件の値・初期値は承認一覧との完全一致で限る。一覧の真正性は検査しないため、承認は人がレビューで担保する。[`ApprovedValues`] の信頼の境界）。
//! 段階の結果は [`RunSession::run`] の中でだけ作り、実行の文脈に結び付ける（型で塞げない部分は [`RunSession`] の信頼の境界）。
//! `judgment:` は常に空で、ハーネスは合否を書かない（ACPT-7）。ファイル名は整数だけで作り、既存ファイルを上書きしない。
//! 誤りは種類だけを持ち、値・パス・`io::Error` の文言を載せない（SECR-11・SECR-23）。`Debug` は中身を出さない。
//! 他所有者の値は `external` で値なし・出典必須（D-2026-10-15-21）。誤りの文言は英語（D-2026-11-20-13）。

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::condition::{ComponentState, ConditionComponent, MeasurementConditions};
use crate::meter::{MonotonicClock, StdMonotonicClock};
use crate::stage::{StageError, StagePlan, StagedRun, run_stages};
use crate::target::{CallResult, TargetSpec};

/// 形式の識別子。形式を変えたら末尾の版を上げる。
pub const LOAD_RECORD_FORMAT: &str = "fandhe-auth-load-record/1";
/// 1 行の出典の数の上限（仮置き）。
pub const MAX_REFS: usize = 8;
/// 承認済みの一覧のファイルの大きさの上限（バイト。仮置き）。
const MAX_APPROVED_FILE_BYTES: u64 = 64 * 1024;
/// 値の行の数の上限（仮置き）。
pub const MAX_VALUES: usize = 64;
/// 時刻（UNIX 秒）の上限。9999-12-31T23:59:59Z。RFC 3339 の 4 桁年に収める（仮置き）。
pub const MAX_UNIX_SECS: u64 = 253_402_300_799;
/// 記録の全体・1 行・1 語の長さの上限（仮置き。`is_fixed_shape`）。
/// 1 行の最大は値の行で、名前 64 + 固定の語 + 出典 8 件（各 64 バイトと区切り 7）で 700 バイト前後のため、1024 に置く。
/// 全体の上限は段階の数 × 繰り返し × 区間・失敗の種類の数の行の合計で、`StageLimits` の上限と区間の宣言の数で決まる
/// （1 行 300 バイト前後。超える計画は `Format` で拒否し、呼び出し元が計画を小さくする。fail-closed）。
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024;
const MAX_WORD_BYTES: usize = 64;

/// `refs` 欄で出力側が使う語。出典の ID には使わせない（出典なし・自動取得の印と取り違えない）。
const RESERVED_REFS: [&str; 3] = ["none", "harness_config", "measured"];

/// 記録の誤り。種類だけを持ち、入力の値・パス・元の誤りの文言は載せない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadRecordError {
    InvalidName,
    InvalidRef,
    InvalidRunRecordFile,
    InvalidRunEvidence,
    ApprovedFile,
    InvalidValue(InvalidValueReason),
    RunStartMismatch,
    TimeOutOfRange,
    TooManyRefs,
    TooManyValues,
    DuplicateValue,
    ClockOrder,
    Format,
    WriteFailed,
    /// 段階の実行の誤り（[`RunSession::run`] の中の [`run_stages`] の誤り。種類と整数の番号だけを持つ）。
    Stage(StageError),
}

impl fmt::Display for LoadRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("load record error: invalid name"),
            Self::InvalidRef => f.write_str("load record error: invalid ref"),
            Self::InvalidRunRecordFile => f.write_str("load record error: invalid run record file"),
            Self::InvalidRunEvidence => {
                f.write_str("load record error: run evidence does not match the stages")
            }
            Self::ApprovedFile => f.write_str("load record error: approved list unreadable"),
            Self::InvalidValue(r) => write!(f, "load record error: invalid value ({})", r.as_str()),
            Self::RunStartMismatch => {
                f.write_str("load record error: run record file and run_started_at differ")
            }
            Self::TimeOutOfRange => f.write_str("load record error: time out of range"),
            Self::TooManyRefs => f.write_str("load record error: too many refs"),
            Self::TooManyValues => f.write_str("load record error: too many values"),
            Self::DuplicateValue => f.write_str("load record error: duplicate value"),
            Self::ClockOrder => f.write_str("load record error: recorded_at before run_started_at"),
            Self::Format => f.write_str("load record error: format failed"),
            Self::WriteFailed => f.write_str("load record error: write failed"),
            Self::Stage(_) => f.write_str("load record error: stage run failed"),
        }
    }
}

impl std::error::Error for LoadRecordError {}

/// 値の行が区分の規則に反した理由。固定の列挙で、入力の値を載せない（SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidValueReason {
    RefsRequired,
    RefsForbidden,
    ValueRequired,
    ValueForbidden,
    ClassNotAllowed,
    /// 承認一覧（[`ApprovedValues`]）のどの行とも完全一致しない。一覧の行が人の承認を経たかは見ていない。
    NotInApprovedList,
}

impl InvalidValueReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RefsRequired => "refs_required",
            Self::RefsForbidden => "refs_forbidden",
            Self::ValueRequired => "value_required",
            Self::ValueForbidden => "value_forbidden",
            Self::ClassNotAllowed => "class_not_allowed",
            Self::NotInApprovedList => "not_in_approved_list",
        }
    }
}

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

/// 項目ごとの許容する区分（AUDP-29）。性能の目標は持ち越しで、`undecided`（値なし）以外にしない。
/// ここに無い名前は区分を限らない（区分ごとの規則は別に検査する）。
const NAME_CLASS_RULES: [(&str, &[ValueClass]); 1] = [("p99_target", &[ValueClass::Undecided])];

/// 出典 ID の形（ASCII 英数字・`-`・`_`・1〜64 文字で、予約語でない）。`RefId::new` と
/// `ApprovedValues::parse` が同じ検査を共有する。
fn is_ref_shape(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !RESERVED_REFS.contains(&s)
}

/// 記録の本文が許可した文字・長さだけかを見る（SECR-23・AAUD-30。`write_new` の出力先は追跡ツリーの外で
/// `secret-scan` の対象に入らないため、書く前に中身を限る）。通すのは ASCII の英数字と ` _-.:=,/`・LF だけで、
/// 1 行 1024 バイト・1 語（空白・`=`・`,` 区切り）64 バイト・全体 1 MiB まで。
///
/// この検査が止めるのは、許可文字の外（記号・非 ASCII・制御文字）と、区切り（空白・`=`・`,`）を含まず 64 バイトを超える語、
/// 長すぎる行・全体の回帰だけである。次は通る: `key=<64 バイト以下>`、区切りで連ねた値、`+` を含まない base64、
/// ホスト名・`IP:port`・パス、libpq の `key=value` 形式、全体が 64 バイト以下の語になる JWT の形（`.` は区切りでないため、通常の長さの JWT は語の上限で止まる）。
/// 64 バイト以下の英数字・base64url・16 進の秘密値は形では止められない。実際に止めているのは各欄の語彙・
/// 列挙・整数の検査である。漏れは照合の試験（`tests/trial_run.rs`）で確かめる。これは AAUD-30・AAUD-31 の方法を
/// ハーネスの出力に当てはめたもので、満たしたものではない（実環境の走査は Hub の `AOPS-73`）。対象は stub の設定の
/// 3 欄から負荷の記録へ値が流れないことの回帰の検出と、設定の口（`with_hub_stub_record`）を通した it-run の記録に
/// 登録簿の値が現れないことの確認まで（it-run の記録に入るのは欄名と印だけで、設定の値が届く経路は無い）。
/// 署名鍵の秘密値は対象外で、未確認のまま持ち越し（追跡先は未定。利用者の判断待ち）。
fn is_fixed_shape(text: &str) -> bool {
    text.len() <= MAX_RECORD_BYTES
        && text.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b' ' | b'_' | b'-' | b'.' | b':' | b'=' | b',' | b'/' | b'\n'
                )
        })
        && text.split('\n').all(|l| {
            l.len() <= MAX_LINE_BYTES && l.split([' ', '=', ',']).all(|w| w.len() <= MAX_WORD_BYTES)
        })
}

/// 値の名前（`[a-z0-9_]`・1〜64 文字で、呼び出し元が渡す承認済みの語彙に含まれるもの）。
#[derive(Clone, PartialEq, Eq)]
pub struct ValueName(String);

impl ValueName {
    /// 記録に出してよい名前だけを作る。語彙の外は誤りにする（自由な名前を出さない。SECR-23）。
    ///
    /// # Errors
    /// 文字種・長さの規則を満たさないか、`vocabulary` に無いとき [`LoadRecordError::InvalidName`]。
    pub fn new(s: &str, vocabulary: &[&str]) -> Result<Self, LoadRecordError> {
        if is_name(s) && vocabulary.contains(&s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(LoadRecordError::InvalidName)
        }
    }
}

/// SECR-23: 中身を出さない。
impl fmt::Debug for ValueName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValueName(..)")
    }
}

/// 出典の ID（ASCII 英数字・`-`・`_`・1〜64 文字で、呼び出し元が渡す信頼済みの一覧に完全一致するもの）。
#[derive(Clone, PartialEq, Eq)]
pub struct RefId(String);

impl RefId {
    /// 信頼済みの一覧（spec の ID 等）と完全一致するものだけを受ける。任意の文字列を出典として出さない（SECR-23）。
    ///
    /// # Errors
    /// 規則を満たさない・予約語（`none`・`harness_config`・`measured`）・`trusted` に無いとき [`LoadRecordError::InvalidRef`]。
    pub fn new(s: &str, trusted: &[&str]) -> Result<Self, LoadRecordError> {
        if is_ref_shape(s) && trusted.contains(&s) {
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
            // it-harness の生成元と同じ型（秒 u64・pid u32・通し番号 u64）で解析し、再整形した名前と完全一致を確かめる
            // （先頭ゼロ・型に収まらない数値は生成されない名前として拒否する）。
            let started: u64 = parts[0].parse().map_err(|_| bad)?;
            let pid: u32 = parts[1].parse().map_err(|_| bad)?;
            let seq: u64 = parts[2].parse().map_err(|_| bad)?;
            if started > MAX_UNIX_SECS || format!("it-run-{started}-{pid}-{seq}.txt") != s {
                return Err(bad);
            }
            Ok(Self(s.to_owned()))
        } else {
            Err(bad)
        }
    }
}

impl RunRecordFile {
    /// ファイル名の先頭の数字（実行の起動秒）。
    fn started_unix_secs(&self) -> u64 {
        self.0
            .strip_prefix("it-run-")
            .and_then(|r| r.split('-').next())
            .and_then(|d| d.parse().ok())
            .unwrap_or(u64::MAX)
    }
}

impl fmt::Debug for RunRecordFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RunRecordFile(..)")
    }
}

/// 承認一覧の 1 行（「項目・区分・数値・単位・出典」の組。`load.md` の「値の区別」規則 4）。
/// 要件の値・初期値は、この組と完全に一致するときだけ記録に出せる。フィールドは非公開で、
/// 作るのは [`ApprovedValues::load`] の書式の解析だけ（行が人の承認を経たかは見ない。[`ApprovedValues`] の信頼の境界）。
#[derive(Clone, PartialEq, Eq)]
struct ApprovedValue {
    name: String,
    class: ValueClass,
    value: u64,
    unit: ValueUnit,
    refs: BTreeSet<String>,
}

/// 承認一覧のファイルを読んだ結果。要件の値・初期値の候補（[`ComparisonValue::new`] の引数）を、
/// この一覧の行との完全一致で限る（規則 4）。作る口はファイルから読む [`ApprovedValues::load`] と、
/// 空の一覧（[`Default`]。どの要件の値・初期値も通さない）だけで、本文の文字列から作る公開の口はない。
///
/// 信頼の境界: この型が検査するのは形式（ファイルの種類・大きさ・UTF-8・書式、候補との完全一致）だけで、
/// 一覧の行を人が承認したこと（真正性）は検査しない。ライブラリの中では、ファイルを書いたのが人か、
/// 記録の候補を持つ同じ呼び出し元かを区別できない（呼び出し元が候補から書いたファイルを渡せば、その値は通る）。
/// 真正性は、承認一覧を repo に commit し、人がレビューで承認することで担保する（値の決定・承認は人が行う。ACPT-7）。
/// 一覧のファイルの置き場と形は未定で、人が決める（`docs/harness/load.md` の「未決・人の確認待ち」）。
///
/// 書式（仮置き）: 1 行 1 件 `項目|区分|数値|単位|出典,出典`。区分は `requirement`・`initial` だけ。
/// 単位は `ns`・`count`・`per_s`。空行と `#` で始まる行は読み飛ばす。
/// PoC のパラメータは受けない（規則 5。比べる値として置かない）。
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ApprovedValues(Vec<ApprovedValue>);

/// SECR-23: 中身を出さない。
impl fmt::Debug for ApprovedValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovedValues(..)")
    }
}

impl ApprovedValues {
    /// 承認一覧のファイルを読み込み専用で開き、形式を検査して読む。検査は形式だけで、承認の真正性は見ない
    /// （型の doc の信頼の境界）。渡すのは repo に commit され人がレビューで承認したファイルに限る。記録の候補から
    /// 書いたファイルを渡すと、その値が要件の値・初期値として通る（ACPT-7・load.md 規則 4。呼び出し側の契約）。
    ///
    /// 開くときに止まらない: Unix では `O_NONBLOCK` を付けて開くため、書き手のいない FIFO を指しても `open` で
    /// 待たず、開いたハンドルの種別の検査で拒否する。種別と大きさは開いたハンドルで見る（パスの差し替えに依らない）。
    /// `O_NONBLOCK` の値を確かめていない対象では、開く前にパスの種別も見る（検査と `open` の間の差し替えは防げない）。
    ///
    /// # Errors
    /// 読めない・通常のファイルでない・大きすぎる・UTF-8 でないとき [`LoadRecordError::ApprovedFile`]。
    /// 書式に反する行・未知の区分や単位・数値でない値・出典の欠け・重複のとき [`LoadRecordError::Format`]
    /// （入力の値・パスを誤りに載せない）。
    pub fn load(path: &Path) -> Result<Self, LoadRecordError> {
        let bad = LoadRecordError::ApprovedFile;
        let file = open_without_blocking(path).map_err(|_| bad)?;
        let meta = file.metadata().map_err(|_| bad)?;
        if !meta.is_file() || meta.len() > MAX_APPROVED_FILE_BYTES {
            return Err(bad);
        }
        let mut text = String::new();
        file.take(MAX_APPROVED_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|_| bad)?;
        if text.len() as u64 > MAX_APPROVED_FILE_BYTES {
            return Err(bad);
        }
        Self::parse_text(&text)
    }

    /// 本文の書式を解析する。非公開（本文の文字列を呼び出し元が渡す口にしない）。
    fn parse_text(text: &str) -> Result<Self, LoadRecordError> {
        let bad = LoadRecordError::Format;
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('|').collect();
            let [name, class, value, unit, refs] = f[..] else {
                return Err(bad);
            };
            let class = match class {
                "requirement" => ValueClass::Requirement,
                "initial" => ValueClass::Initial,
                _ => return Err(bad),
            };
            let unit = match unit {
                "ns" => ValueUnit::Ns,
                "count" => ValueUnit::Count,
                "per_s" => ValueUnit::PerS,
                _ => return Err(bad),
            };
            let value: u64 = value.parse().map_err(|_| bad)?;
            if !is_name(name) || refs.is_empty() {
                return Err(bad);
            }
            let list: Vec<&str> = refs.split(',').collect();
            let set: BTreeSet<String> = list.iter().map(|r| (*r).to_owned()).collect();
            if list.iter().any(|r| !is_ref_shape(r))
                || set.len() != list.len()
                || list.len() > MAX_REFS
            {
                return Err(bad);
            }
            out.push(ApprovedValue {
                name: name.to_owned(),
                class,
                value,
                unit,
                refs: set,
            });
        }
        Ok(Self(out))
    }

    fn contains(
        &self,
        name: &str,
        class: ValueClass,
        value: u64,
        unit: ValueUnit,
        refs: &[RefId],
    ) -> bool {
        let got: BTreeSet<&str> = refs.iter().map(|r| r.0.as_str()).collect();
        self.0.iter().any(|a| {
            a.name == name
                && a.class == class
                && a.value == value
                && a.unit == unit
                && a.refs.iter().map(String::as_str).collect::<BTreeSet<_>>() == got
        })
    }
}

/// 開くときに止まらない `O_NONBLOCK` の値。libc 0.2.191 の定義（`src/unix/` の各対象）と突き合わせた対象だけに置く
/// （通常の依存を 0 に保つため libc に依存しない）。ここに無い対象は `None` で、開く前にパスの種別を見る。
/// CI で実地に確かめているのは Linux x86_64 の値だけで、他の対象の値は libc の定義との突き合わせだけ
/// （その対象でのビルド・実行はしていない）。
#[cfg(unix)]
#[allow(unreachable_code)]
const fn nonblock_flag() -> Option<i32> {
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv32",
            target_arch = "riscv64",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "s390x",
            target_arch = "loongarch64"
        )
    ))]
    return Some(0o4000);
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "mips", target_arch = "mips64")
    ))]
    return Some(0o200);
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(target_arch = "sparc", target_arch = "sparc64")
    ))]
    return Some(0x4000);
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    return Some(0x4);
    None
}

/// 読み込み専用で、開くときに止まらないように開く（書き手のいない FIFO で `open` が待たないため）。
/// 種別と大きさの検査は呼び出し元が開いたハンドルで行う。
fn open_without_blocking(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = nonblock_flag() {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.custom_flags(flag);
        return opts.open(path);
    }
    // 値を確かめていない対象: 開く前にパスの種別を見て、通常のファイル以外を開かない
    // （検査と open の間の差し替えは防げない。開いた後のハンドルの検査は呼び出し元が行う）。
    if !std::fs::metadata(path)?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    opts.open(path)
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
    /// 要件の値・初期値は、`approved`（承認一覧のファイルを読んだ結果）の 1 行と項目・区分・数値・単位・出典が
    /// 完全一致するときだけ作る（一致しなければ `NotInApprovedList`。規則 4）。他の区分は `approved` を見ない。
    /// 一致の検査は形式の検査で、一覧の行が人の承認を経たかは見ない（[`ApprovedValues`] の信頼の境界。ACPT-7）。
    ///
    /// # Errors
    /// 区分の規則に反するとき [`LoadRecordError::InvalidValue`]、出典が 9 件以上のとき
    /// [`LoadRecordError::TooManyRefs`]、重複のとき [`LoadRecordError::InvalidRef`]。
    pub fn new(
        name: ValueName,
        class: ValueClass,
        value: Option<u64>,
        unit: ValueUnit,
        refs: Vec<RefId>,
        approved: &ApprovedValues,
    ) -> Result<Self, LoadRecordError> {
        let bad = LoadRecordError::InvalidValue;
        use InvalidValueReason::{
            ClassNotAllowed, NotInApprovedList, RefsForbidden, RefsRequired, ValueForbidden,
            ValueRequired,
        };
        if refs.len() > MAX_REFS {
            return Err(LoadRecordError::TooManyRefs);
        }
        let uniq: BTreeSet<&str> = refs.iter().map(|r| r.0.as_str()).collect();
        if uniq.len() != refs.len() {
            return Err(LoadRecordError::InvalidRef);
        }
        if let Some((_, allowed)) = NAME_CLASS_RULES.iter().find(|(n, _)| *n == name.0.as_str())
            && !allowed.contains(&class)
        {
            return Err(bad(ClassNotAllowed));
        }
        match class {
            // 規則 5: PoC のパラメータの数値は比べる値として置かない（区分を受けない）。
            ValueClass::PocParameter => return Err(bad(ClassNotAllowed)),
            ValueClass::Requirement | ValueClass::Initial => {
                if refs.is_empty() {
                    return Err(bad(RefsRequired));
                }
                let Some(v) = value else {
                    return Err(bad(ValueRequired));
                };
                if !approved.contains(name.0.as_str(), class, v, unit, &refs) {
                    return Err(bad(NotInApprovedList));
                }
            }
            ValueClass::Undecided => {
                if value.is_some() {
                    return Err(bad(ValueForbidden));
                }
            }
            ValueClass::External => {
                if value.is_some() {
                    return Err(bad(ValueForbidden));
                }
                if refs.is_empty() {
                    return Err(bad(RefsRequired));
                }
            }
            ValueClass::TestCondition | ValueClass::Measured => {
                if value.is_none() {
                    return Err(bad(ValueRequired));
                }
                if !refs.is_empty() {
                    return Err(bad(RefsForbidden));
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

/// 1 回の実行の文脈（実行の記録への参照・起動時刻・測定の条件。`load.md` の 1・5）。
/// [`RunSession::run`] で段階をその場で回すと [`RunEvidence`] になる。
///
/// 型で塞いだもの: 段階の結果（[`StagedRun`]）を後から渡して [`RunEvidence`] を作る口は無い。段階の結果は
/// [`RunSession::run`] の中でだけ作られ、同じ文脈（実行の記録・測定の条件）とまとめて [`RunEvidence`] になる。
/// 別の実行の段階の結果を組み合わせる経路は無い。呼び出す構成要素（`exercised`）は回す前に申告させ、測定の条件で
/// 起動していなければ対象を 1 回も呼ばずに拒否する（申告を検査してから回す。申告そのものは記録に残さない）。
/// 時計は [`RunSession::run`] の中で標準の単調な時計（[`StdMonotonicClock`]）を作り、引数で受けない
/// （偽の時計で `class=measured` の時間を作る経路を作らない。決定的な時計が要る試験は [`run_stages`] を直接使い、
/// その結果からは [`RunEvidence`] を作れない）。stub の種類は [`crate::ComponentConditions::new`] が実物として載せられない（`StubMismatch`）ため、
/// stub を申告した実行の記録には stub の起動と `note:` が必ず出る（`A-2026-10-28-9`・ACPT-7。`note:` は条件の `Display`）。
///
/// 信頼の境界（塞げないもの。呼び出し側の契約）: (1) 閉包 `call` が実際に呼ぶ構成要素と `exercised` の一致
/// （閉包の中身は見えない）。(2) `conditions` が実際に起動した構成要素と一致すること（呼び出し元が it-harness の
/// 起動済みの構成から写す）。(3) `run_record_file` が同じ実行の記録であること（見るのは起動秒の一致だけで、
/// 名前の文字列は誰でも作れるため、結び付けの証明ではない）。(4) `conditions` の機材の条件が実際の機材と一致すること
/// （[`crate::MachineCondition::new`] や独自の [`crate::MachineProbe`] で `virtualization`・`build_profile` 等を
/// 呼び出し元が自由に申告できる）。(1)〜(3) は、この crate が通常の依存で it-harness を持たない（どの crate にも
/// 依存しない）ため確かめられない。(4) は呼び出し元が明示する値を受ける設計で、申告の正しさは確かめない。stub の結果を実物の結果として申告しない
/// ことは呼び出し元の試験とレビューで担保する（〔仮想〕〔モック〕の合格を〔実機〕扱いしない。ACPT-7）。
pub struct RunSession {
    run_record_file: RunRecordFile,
    run_started_at: u64,
    conditions: MeasurementConditions,
}

impl fmt::Debug for RunSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RunSession(..)")
    }
}

impl RunSession {
    /// # Errors
    /// 時刻が範囲外のとき [`LoadRecordError::TimeOutOfRange`]、ファイル名の起動秒と違うとき
    /// [`LoadRecordError::RunStartMismatch`]。
    pub fn new(
        run_record_file: RunRecordFile,
        run_started_at_unix_secs: u64,
        conditions: MeasurementConditions,
    ) -> Result<Self, LoadRecordError> {
        if run_started_at_unix_secs > MAX_UNIX_SECS {
            return Err(LoadRecordError::TimeOutOfRange);
        }
        if run_record_file.started_unix_secs() != run_started_at_unix_secs {
            return Err(LoadRecordError::RunStartMismatch);
        }
        Ok(Self {
            run_record_file,
            run_started_at: run_started_at_unix_secs,
            conditions,
        })
    }

    /// `exercised`（閉包 `call` が呼ぶ構成要素。呼ぶ前に申告する）を検査してから、[`run_stages`] で段階を回し、
    /// 結果をこの文脈とまとめた [`RunEvidence`] を返す（申告そのものは記録に残さない）。各回の時計はここで
    /// [`StdMonotonicClock`] を作る（時計は引数で受けない）。文脈の値は消費し、1 つの値で段階を回すのは 1 回だけ
    /// （同じ実行の記録の名前で別の文脈を作ること・作った [`RunEvidence`] の複製は止めない）。
    ///
    /// # Errors
    /// `exercised` が空、または測定の条件で起動していない要素を含むとき [`LoadRecordError::InvalidRunEvidence`]
    /// （対象を 1 回も呼ばない）。段階の実行の誤りは [`LoadRecordError::Stage`]（終わった段階の結果も返さない）。
    pub fn run<F>(
        self,
        exercised: &[ConditionComponent],
        spec: &TargetSpec,
        plan: &StagePlan,
        call: F,
    ) -> Result<RunEvidence, LoadRecordError>
    where
        F: Fn() -> CallResult + Sync,
    {
        if exercised.is_empty()
            || exercised
                .iter()
                .any(|k| self.conditions.components.state(*k) == ComponentState::NotStarted)
        {
            return Err(LoadRecordError::InvalidRunEvidence);
        }
        let new_clock = || -> Box<dyn MonotonicClock> { Box::new(StdMonotonicClock::new()) };
        let staged = run_stages(new_clock, spec, plan, call).map_err(LoadRecordError::Stage)?;
        Ok(RunEvidence {
            run_record_file: self.run_record_file,
            run_started_at: self.run_started_at,
            conditions: self.conditions,
            staged,
        })
    }
}

/// 1 回の実行から得た値のまとめ（実行の記録への参照・起動時刻・測定の条件・段階の結果。`load.md` の 1・5）。
/// 作る口は [`RunSession::run`] だけで、段階の結果を後から渡す公開の口は無い（結び付けと信頼の境界は [`RunSession`]）。
#[derive(Clone, PartialEq, Eq)]
pub struct RunEvidence {
    run_record_file: RunRecordFile,
    run_started_at: u64,
    conditions: MeasurementConditions,
    staged: StagedRun,
}

impl fmt::Debug for RunEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RunEvidence(..)")
    }
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
        evidence: &RunEvidence,
        recorded_at_unix_secs: u64,
        values: Vec<ComparisonValue>,
    ) -> Result<Self, LoadRecordError> {
        let run_started_at_unix_secs = evidence.run_started_at;
        let run_record_file = &evidence.run_record_file;
        let (conditions, staged) = (&evidence.conditions, &evidence.staged);
        if recorded_at_unix_secs > MAX_UNIX_SECS {
            return Err(LoadRecordError::TimeOutOfRange);
        }
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
        if !is_fixed_shape(&t) {
            return Err(LoadRecordError::Format);
        }
        Ok(Self {
            run_started_at: run_started_at_unix_secs,
            text: t,
        })
    }

    /// 新規のファイルへ書き出す。`dir` は呼び出し元が渡し、無ければ作る。既存のファイルは上書きしない。
    /// 出力先のファイルシステムはハードリンクに対応している必要がある（公開に `hard_link` を使う。
    /// 非対応の出力先では [`LoadRecordError::WriteFailed`] になる）。
    /// 名前は `load-run-<起動時刻の秒>-<pid>-<通し番号>.txt`（乱数を使わない）。
    ///
    /// # Errors
    /// 作成・書き込みに失敗したとき [`LoadRecordError::WriteFailed`]（元の誤りは載せない）。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, LoadRecordError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        // 書く直前にも固定の形を確かめる（出力先は secret-scan の対象外。何も作る前に止める）。
        if !is_fixed_shape(&self.text) {
            return Err(LoadRecordError::Format);
        }
        std::fs::create_dir_all(dir).map_err(|_| LoadRecordError::WriteFailed)?;
        self.write_numbered(dir, SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// 通し番号を指定して書き出す本体。名前の衝突を決定的に試験するため分けてある。
    fn write_numbered(&self, dir: &Path, n: u64) -> Result<PathBuf, LoadRecordError> {
        let path = dir.join(format!(
            "load-run-{}-{}-{n}.txt",
            self.run_started_at,
            std::process::id()
        ));
        // 書き込みの途中で失敗した不完全なファイルを完成品の名前で残さない。
        // 隠し名の一時ファイルへ書ききってから、上書きなしの hard_link で最終名へ公開する。
        let tmp = dir.join(format!(
            ".tmp-load-run-{}-{}-{n}",
            self.run_started_at,
            std::process::id()
        ));
        // 一時ファイルを作れた場合だけ後処理で消す。名前の衝突で作れなかったときは、
        // 既存のファイルを消さない（`create_new` の失敗経路で `remove_file` を呼ばない）。
        let mut created = false;
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            created = true;
            file.write_all(self.text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            std::fs::hard_link(&tmp, &path)
        })();
        if created {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|_| LoadRecordError::WriteFailed)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> LoadRecord {
        LoadRecord {
            run_started_at: 0,
            text: "new\n".to_owned(),
        }
    }

    /// 排他的に新規作成したディレクトリ。既存のものは消さず、自分が作ったものだけを Drop で片付ける。
    struct TestDir(PathBuf);

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> TestDir {
        static N: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let d = std::env::temp_dir().join(format!(
                "{name}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            // create_dir は既存なら失敗する。別の名前で作り直し、既存を消さない。
            match std::fs::create_dir(&d) {
                Ok(()) => return TestDir(d),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("create test dir: {e}"),
            }
        }
        panic!("no free test dir name");
    }

    /// 許可文字・長さの網。境界の値（64/65・1024/1025・1 MiB/+1）で、通る側と止まる側の両方を確かめる。
    #[test]
    fn fixed_shape_boundaries() {
        assert!(is_fixed_shape("format: fandhe-auth-load-record/1\n"));
        assert!(is_fixed_shape(
            "run: 1.1 class=measured refs=measured n=100\njudgment:\n"
        ));
        // 語 64 / 65（区切りは空白・`=`・`,`）。
        for sep in [" ", "=", ","] {
            assert!(is_fixed_shape(&format!("x:{sep}{}\n", "a".repeat(64))));
            assert!(!is_fixed_shape(&format!("x:{sep}{}\n", "a".repeat(65))));
        }
        // 1 行 1024 / 1025（語は 64 以下に保つ）。
        let line = |n: usize| format!("{} ", "a".repeat(63)).repeat(n / 64 + 1)[..n].to_owned();
        assert!(is_fixed_shape(&format!("{}\n", line(1024))));
        assert!(!is_fixed_shape(&format!("{}\n", line(1025))));
        // 全体 1 MiB / +1（各行 64 バイト）。
        let total = format!("{}\n", "a".repeat(63)).repeat(MAX_RECORD_BYTES / 64);
        assert_eq!(total.len(), MAX_RECORD_BYTES);
        assert!(is_fixed_shape(&total));
        assert!(!is_fixed_shape(&format!("{total}\n")));
        // JWT の形: 語（`.` は区切りでない）が 64 バイト以下なら網を通る（網は止めない。照合の試験が 0 件を数える）。
        // 64 バイトを超えれば止まる。
        let jwt = |n: usize| format!("note: eyJ{0}.eyJ{0}.{0}\n", "a".repeat(n));
        assert!(is_fixed_shape(&jwt(15)));
        assert!(!is_fixed_shape(&jwt(30)));
        // 許可文字の外（記号・非 ASCII・制御文字）。
        for bad in [
            "x: a\\b\n",
            "x: p@ss\n",
            "x: \u{3042}\n",
            "x: a\tb\n",
            "x: a\rb\n",
            "x: a+b\n",
            "x: a;b\n",
        ] {
            assert!(!is_fixed_shape(bad));
        }
    }

    /// 網の外の本文は、ファイルも作らず Format で拒否する。
    #[test]
    fn write_new_rejects_text_outside_shape() {
        for bad in [format!("x: {}\n", "a".repeat(65)), "x: p@ss\n".to_owned()] {
            let td = dir("load-shape");
            let r = LoadRecord {
                run_started_at: 0,
                text: bad,
            };
            assert_eq!(r.write_new(&td.0), Err(LoadRecordError::Format));
            assert_eq!(std::fs::read_dir(&td.0).unwrap().count(), 0);
        }
    }

    /// 一時名が衝突したら WriteFailed。既存の内容を消さず、最終名も作らない。
    #[test]
    fn tmp_name_collision_fails_and_keeps_existing() {
        let td = dir("load-collision-tmp");
        let d = td.0.clone();
        let pid = std::process::id();
        let tmp = d.join(format!(".tmp-load-run-0-{pid}-7"));
        std::fs::write(&tmp, b"keep").unwrap();
        assert_eq!(
            rec().write_numbered(&d, 7),
            Err(LoadRecordError::WriteFailed)
        );
        assert_eq!(std::fs::read(&tmp).unwrap(), b"keep");
        assert!(!d.join(format!("load-run-0-{pid}-7.txt")).exists());
    }

    /// 最終名が衝突したら WriteFailed。既存の内容は保たれ、一時ファイルは残らない。
    #[test]
    fn final_name_collision_fails_and_keeps_existing() {
        let td = dir("load-collision-final");
        let d = td.0.clone();
        let pid = std::process::id();
        let fin = d.join(format!("load-run-0-{pid}-9.txt"));
        std::fs::write(&fin, b"keep").unwrap();
        assert_eq!(
            rec().write_numbered(&d, 9),
            Err(LoadRecordError::WriteFailed)
        );
        assert_eq!(std::fs::read(&fin).unwrap(), b"keep");
        assert!(!d.join(format!(".tmp-load-run-0-{pid}-9")).exists());
    }
}
