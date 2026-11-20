//! 測定の条件（機材・構成・負荷の型）を、人の手入力なしで記録の行に出す部品（TASK-12 7/10）。
//!
//! 役割: `docs/harness/load.md` の「結果の記録の形式」の 5 が定めた `condition_machine`・`condition_component`・
//! stub の `note:`・`condition_loop` の行を、型と `Display` で出す。PoC の値と要件の値を混同しないよう、
//! 測定の条件を結果の値から分けて記録に残すための部品である。
//! 呼び出し元はハーネスの試験（試験ビルド）だけで、記録のファイル出力を担う後段（担当の段は未決）が使う想定。
//! 条件は「記録の単位」で 1 つ持ち、`TargetRun`・`LoadRun` には入れない（段階を変える実行 8/10 が重複しないため）。
//!
//! 取得: 機材は [`MachineProbe`] から取る。[`StdMachineProbe`] は std と Linux の `/proc/meminfo`（固定パス・
//! 上限付き・整数 1 つだけ取り出す）だけを読む。ホスト名・機材の名前・利用者名・パス・アドレスは取得も記録もしない
//! （SECR-11・SECR-23・SECR-26、04-behavior/README.md の (d)）。`arch`・`os_family` は固定の許可語彙だけを受け、
//! 語彙の外の値（識別情報・制御文字入りを含む）は記録を作らずエラーで返す。取れない（`None`）値だけが `unknown` になる。
//! 構成は呼び出し元が「起動したもの」を明示で渡す。既定値は置かない。
//!
//! 範囲外: 記録のファイル全体の出力・`stage` 行（並行数は 8/10 とファイル出力の段）・仮想化の判別。合否は付けない（`ACPT-7`）。
//! 名前・語・値はすべて仮置きで、人が確認する。

use std::fmt;

use crate::generate::LoopModel;

/// 出力する各行の末尾の固定部分（区分と出典。形式の 2 に従う）。
const TAIL: &str = "class=test_condition refs=harness_config";

/// 条件の組み立ての誤り。種類だけを持ち、値・名前の中身・パスを載せない（SECR-11・SECR-23）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionError {
    /// 同じ構成要素を 2 度渡した。
    DuplicateComponent,
    /// 構成要素の stub の印が、その種類の stub かどうかと食い違う（A-2026-10-28-9）。
    StubMismatch,
    /// 機材の文字列（`arch`・`os_family`）が許可語彙の外。値は載せない。
    InvalidMachineValue,
}

impl fmt::Display for ConditionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateComponent => f.write_str("duplicate component"),
            Self::StubMismatch => f.write_str("stub mark does not match the component kind"),
            Self::InvalidMachineValue => f.write_str("invalid machine value"),
        }
    }
}

impl std::error::Error for ConditionError {}

/// 仮想化の区別。判別の証拠が無いときは [`Virtualization::Unknown`]（`none` と読まない）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Virtualization {
    None,
    Vm,
    Container,
    Unknown,
}

impl Virtualization {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Vm => "vm",
            Self::Container => "container",
            Self::Unknown => "unknown",
        }
    }
}

/// ビルドの種別。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildProfile {
    Debug,
    Release,
    Unknown,
}

impl BuildProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
            Self::Unknown => "unknown",
        }
    }
}

/// 機材の生の値の取得元。環境から入る値なので、許可語彙の検査は [`MachineCondition::detect_with`] が行う。
/// 試験では偽の実装を渡して決定的にする。
pub trait MachineProbe {
    fn cpu_count(&self) -> Option<u64>;
    fn memory_mib(&self) -> Option<u64>;
    fn arch(&self) -> Option<String>;
    fn os_family(&self) -> Option<String>;
    fn virtualization(&self) -> Virtualization;
    fn build_profile(&self) -> BuildProfile;
}

/// std による取得元。
///
/// - `cpu_count`: `available_parallelism`。affinity・cgroup の上限が効く「使える並列数」で、物理 CPU 数ではない。
/// - `memory_mib`: Linux だけ `/proc/meminfo` の `MemTotal`。コンテナでは cgroup の上限ではなくホストの総量が出る。
/// - `arch`・`os_family`: `std::env::consts` の `ARCH`・`OS`。許可語彙の外（対象外のプラットフォーム）は誤りにせず
///   `None`（未取得）にして、他の測定条件の記録を妨げない。外から注入された値の拒否は [`in_vocab`] が維持する。
/// - `virtualization`: 判別の方法が未決のため常に `Unknown`。
/// - `build_profile`: 実行時にプロファイルを確かめる手段が無い（`debug_assertions` はプロファイルの設定で上書きされ誤記録になる）ため
///   常に `Unknown`。確かな値は呼び出し元が [`MachineCondition::new`] で明示する。
#[derive(Clone, Copy, Debug, Default)]
pub struct StdMachineProbe;

impl MachineProbe for StdMachineProbe {
    fn cpu_count(&self) -> Option<u64> {
        std::thread::available_parallelism()
            .ok()
            .and_then(|n| u64::try_from(n.get()).ok())
    }

    fn memory_mib(&self) -> Option<u64> {
        read_mem_total_mib()
    }

    fn arch(&self) -> Option<String> {
        std_value_in_vocab(std::env::consts::ARCH, ARCH_VOCAB)
    }

    fn os_family(&self) -> Option<String> {
        std_value_in_vocab(std::env::consts::OS, OS_VOCAB)
    }

    fn virtualization(&self) -> Virtualization {
        Virtualization::Unknown
    }

    fn build_profile(&self) -> BuildProfile {
        BuildProfile::Unknown
    }
}

/// `/proc/meminfo` から読む量の上限（仮置き。読みすぎない・中身を抱えない）。
#[cfg(target_os = "linux")]
const MEMINFO_READ_LIMIT: u64 = 64 * 1024;

#[cfg(target_os = "linux")]
fn read_mem_total_mib() -> Option<u64> {
    use std::io::Read;
    let file = std::fs::File::open("/proc/meminfo").ok()?;
    let mut buf = String::new();
    file.take(MEMINFO_READ_LIMIT)
        .read_to_string(&mut buf)
        .ok()?;
    parse_mem_total_mib(&buf)
}

#[cfg(not(target_os = "linux"))]
fn read_mem_total_mib() -> Option<u64> {
    None
}

/// `MemTotal:` の行の kB を MiB（整数除算）にする。形の外れはすべて `None`。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_mem_total_mib(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let mut it = line["MemTotal:".len()..].split_whitespace();
    let kb: u64 = it.next()?.parse().ok()?;
    match it.next() {
        Some("kB") if it.next().is_none() => Some(kb / 1024),
        _ => None,
    }
}

/// 機材の 6 属性。数値は整数、文字列は検査済みの値だけを持つ。取れないものは `None`（出力は `unknown`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineCondition {
    cpu_count: Option<u64>,
    memory_mib: Option<u64>,
    arch: Option<&'static str>,
    os_family: Option<&'static str>,
    virtualization: Virtualization,
    build_profile: BuildProfile,
}

/// `std::env::consts::ARCH` の語彙（仮置き。人が確認する）。
const ARCH_VOCAB: &[&str] = &[
    "x86",
    "x86_64",
    "arm",
    "aarch64",
    "loongarch64",
    "m68k",
    "csky",
    "mips",
    "mips64",
    "powerpc",
    "powerpc64",
    "riscv32",
    "riscv64",
    "s390x",
    "sparc",
    "sparc64",
    "hexagon",
    "wasm32",
    "wasm64",
    "mips32r6",
    "mips64r6",
    "powerpc64le",
    "xtensa",
    "avr",
    "msp430",
    "bpf",
    "nvptx64",
    "amdgpu",
];

/// `std::env::consts::OS` の語彙（仮置き。人が確認する）。
const OS_VOCAB: &[&str] = &[
    "linux",
    "macos",
    "windows",
    "freebsd",
    "netbsd",
    "openbsd",
    "dragonfly",
    "solaris",
    "illumos",
    "android",
    "ios",
    "fuchsia",
    "redox",
    "haiku",
    "wasi",
    "tvos",
    "visionos",
    "watchos",
    "cygwin",
    "aix",
    "hurd",
    "nto",
    "vxworks",
    "espidf",
    "horizon",
    "vita",
    "l4re",
    "nuttx",
    "emscripten",
];

/// 標準の取得元の値を許可語彙に照らす。語彙の外は `None`（未取得）。注入値の検査（[`in_vocab`]）とは別で、
/// 標準の取得元が対象外のプラットフォームで他の測定条件を巻き込まないための入口。
fn std_value_in_vocab(v: &str, vocab: &'static [&'static str]) -> Option<String> {
    vocab.iter().find(|w| **w == v).map(|w| (*w).to_owned())
}

/// 許可語彙の中の値だけを通す。外れは値を載せずに誤りにする（取れない `None` とは区別する）。
fn in_vocab(
    v: Option<String>,
    vocab: &'static [&'static str],
) -> Result<Option<&'static str>, ConditionError> {
    match v {
        None => Ok(None),
        Some(s) => vocab
            .iter()
            .find(|w| **w == s)
            .map(|w| Some(*w))
            .ok_or(ConditionError::InvalidMachineValue),
    }
}

impl MachineCondition {
    /// 取得元から作る。取れない（`None`）値は `unknown`。値が返ったのに許可語彙の外なら、`unknown` に
    /// 置き換えず誤りにする（制御文字・識別情報を含む値を記録に入れず、取得不能とも混同しない）。
    ///
    /// # Errors
    /// `arch`・`os_family` が許可語彙の外のとき [`ConditionError::InvalidMachineValue`]。
    pub fn detect_with(probe: &dyn MachineProbe) -> Result<Self, ConditionError> {
        Ok(Self {
            cpu_count: probe.cpu_count(),
            memory_mib: probe.memory_mib(),
            arch: in_vocab(probe.arch(), ARCH_VOCAB)?,
            os_family: in_vocab(probe.os_family(), OS_VOCAB)?,
            virtualization: probe.virtualization(),
            build_profile: probe.build_profile(),
        })
    }

    /// 実機の std の取得元で作る。
    ///
    /// # Errors
    /// [`MachineCondition::detect_with`] と同じ。
    pub fn detect() -> Result<Self, ConditionError> {
        Self::detect_with(&StdMachineProbe)
    }

    /// 呼び出し元が明示で値を渡す入口。許可語彙の外の文字列は fail-closed で拒否する。
    ///
    /// # Errors
    /// `arch`・`os_family` が許可語彙の外のとき [`ConditionError::InvalidMachineValue`]。
    pub fn new(
        cpu_count: Option<u64>,
        memory_mib: Option<u64>,
        arch: Option<String>,
        os_family: Option<String>,
        virtualization: Virtualization,
        build_profile: BuildProfile,
    ) -> Result<Self, ConditionError> {
        Ok(Self {
            cpu_count,
            memory_mib,
            arch: in_vocab(arch, ARCH_VOCAB)?,
            os_family: in_vocab(os_family, OS_VOCAB)?,
            virtualization,
            build_profile,
        })
    }
}

fn num(v: Option<u64>) -> String {
    v.map_or_else(|| "unknown".to_owned(), |n| n.to_string())
}

fn text(v: Option<&'static str>) -> &'static str {
    v.unwrap_or("unknown")
}

impl fmt::Display for MachineCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows: [(&str, String); 6] = [
            ("cpu_count", num(self.cpu_count)),
            ("memory_mib", num(self.memory_mib)),
            ("arch", text(self.arch).to_owned()),
            ("os_family", text(self.os_family).to_owned()),
            ("virtualization", self.virtualization.as_str().to_owned()),
            ("build_profile", self.build_profile.as_str().to_owned()),
        ];
        for (k, v) in rows {
            writeln!(f, "condition_machine: {k} {v} {TAIL}")?;
        }
        Ok(())
    }
}

/// 構成要素の種類。語彙と並びは it-harness の実行の記録の `component` 行と同じ（試験で突き合わせる）。
/// it-harness は試験（dev）からしか見えないため、ここで自前に持つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionComponent {
    Database,
    Host,
    HubStub,
    ConsoleStub,
}

impl ConditionComponent {
    /// 出力の並び（起動順）。
    pub const ORDER: [ConditionComponent; 4] =
        [Self::Database, Self::Host, Self::HubStub, Self::ConsoleStub];

    /// stub（実物ではない構成要素）か（A-2026-10-28-9・ACPT-7）。網羅の `match` で書く。
    pub fn is_stub(self) -> bool {
        match self {
            Self::HubStub | Self::ConsoleStub => true,
            Self::Database | Self::Host => false,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "Database",
            Self::Host => "Host",
            Self::HubStub => "HubStub",
            Self::ConsoleStub => "ConsoleStub",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Database => 0,
            Self::Host => 1,
            Self::HubStub => 2,
            Self::ConsoleStub => 3,
        }
    }
}

/// 構成要素の起動状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentState {
    StartedReal,
    StartedStub,
    NotStarted,
}

impl ComponentState {
    fn as_str(self) -> &'static str {
        match self {
            Self::StartedReal => "started real",
            Self::StartedStub => "started stub",
            Self::NotStarted => "not_started",
        }
    }
}

/// 4 種すべての起動状態。渡されなかった種類は `NotStarted` として明示で出す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentConditions {
    states: [ComponentState; 4],
}

impl ComponentConditions {
    /// 起動したもの（種類と stub の印）から作る。
    ///
    /// # Errors
    /// 重複は [`ConditionError::DuplicateComponent`]、stub の印が種類と食い違えば [`ConditionError::StubMismatch`]。
    // TODO(TASK-716): Host の実体ができるまで、Host が StartedReal になる経路は試験に無い。
    pub fn new(started: &[(ConditionComponent, bool)]) -> Result<Self, ConditionError> {
        let mut states = [ComponentState::NotStarted; 4];
        for (i, (kind, stub)) in started.iter().enumerate() {
            if started[..i].iter().any(|(k, _)| k == kind) {
                return Err(ConditionError::DuplicateComponent);
            }
            if *stub != kind.is_stub() {
                return Err(ConditionError::StubMismatch);
            }
            states[kind.index()] = if *stub {
                ComponentState::StartedStub
            } else {
                ComponentState::StartedReal
            };
        }
        Ok(Self { states })
    }

    /// stub が 1 つでも起動しているか。
    pub fn has_stub(&self) -> bool {
        self.states.contains(&ComponentState::StartedStub)
    }

    /// 種類ごとの状態。
    pub fn state(&self, kind: ConditionComponent) -> ComponentState {
        self.states[kind.index()]
    }
}

/// 記録の単位で 1 つ持つ測定の条件（機材・構成・負荷の型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasurementConditions {
    pub machine: MachineCondition,
    pub components: ComponentConditions,
    pub loop_model: LoopModel,
}

impl MeasurementConditions {
    pub fn new(
        machine: MachineCondition,
        components: ComponentConditions,
        loop_model: LoopModel,
    ) -> Self {
        Self {
            machine,
            components,
            loop_model,
        }
    }

    /// 機材を自動で取得して作る。測定の条件が人の手入力なしで記録に入る入口。
    ///
    /// # Errors
    /// [`MachineCondition::detect`] と同じ。
    pub fn detect(
        components: ComponentConditions,
        loop_model: LoopModel,
    ) -> Result<Self, ConditionError> {
        Ok(Self::new(
            MachineCondition::detect()?,
            components,
            loop_model,
        ))
    }
}

impl fmt::Display for MeasurementConditions {
    /// 機材 6 行・構成 4 行・（stub があれば）`note:`・`condition_loop` の順。各行は LF で終える。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.machine)?;
        for kind in ConditionComponent::ORDER {
            writeln!(
                f,
                "condition_component: {} {} {TAIL}",
                kind.as_str(),
                self.components.state(kind).as_str()
            )?;
        }
        if self.components.has_stub() {
            writeln!(f, "note: stub results are not counted as real results")?;
        }
        writeln!(f, "condition_loop: {} {TAIL}", self.loop_model.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ARCH_VOCAB, BuildProfile, ConditionError, MachineCondition, OS_VOCAB, Virtualization,
        parse_mem_total_mib, std_value_in_vocab,
    };

    #[test]
    fn std_probe_value_outside_vocab_is_unobtained() {
        assert_eq!(std_value_in_vocab("someos", OS_VOCAB), None);
        assert_eq!(std_value_in_vocab("somearch", ARCH_VOCAB), None);
        assert_eq!(
            std_value_in_vocab("tvos", OS_VOCAB).as_deref(),
            Some("tvos")
        );
        assert_eq!(
            std_value_in_vocab("mips32r6", ARCH_VOCAB).as_deref(),
            Some("mips32r6")
        );
    }

    #[test]
    fn injected_value_outside_vocab_stays_rejected() {
        let r = MachineCondition::new(
            None,
            None,
            Some("somearch".to_owned()),
            None,
            Virtualization::Unknown,
            BuildProfile::Unknown,
        );
        assert!(matches!(r, Err(ConditionError::InvalidMachineValue)));
    }

    #[test]
    fn detect_succeeds_on_this_target() {
        assert!(MachineCondition::detect().is_ok());
    }

    #[test]
    fn parses_mem_total() {
        assert_eq!(
            parse_mem_total_mib("Foo: 1 kB\nMemTotal:       16384 kB\nBar: 2 kB\n"),
            Some(16)
        );
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_mem_total_mib("MemFree: 10 kB\n"), None);
        assert_eq!(parse_mem_total_mib(""), None);
        assert_eq!(parse_mem_total_mib("MemTotal: abc kB\n"), None);
        assert_eq!(
            parse_mem_total_mib("MemTotal: 99999999999999999999999 kB\n"),
            None
        );
        assert_eq!(parse_mem_total_mib("MemTotal: 10 MB\n"), None);
        assert_eq!(parse_mem_total_mib("MemTotal: 10\n"), None);
        assert_eq!(parse_mem_total_mib("MemTotal: 10 kB extra\n"), None);
    }
}
