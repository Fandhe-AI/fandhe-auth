//! 構成要素の組み立て・起動・停止の入口。
//!
//! 呼び出し元は結合試験。起動に失敗したら起動済みを逆順に停止してから誤りを返す（fail-closed の後始末）。
//! 環境変数・ファイルは読まない（接続情報の受け方は 2/6 で決める。DPOL-32）。

use std::fmt;
use std::time::SystemTime;

use crate::component::{Component, ComponentKind};
use crate::record::{RecordError, RunRecord, UtcTimestamp, VersionInfo};

/// 誤りの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    /// 同じ種類の構成要素が 2 つ以上渡された（複数インスタンスを作らない）。
    Duplicate,
    /// 起動に失敗した。
    StartFailed,
    /// 停止に失敗した。
    StopFailed,
}

/// ハーネスの誤り。種類と構成要素の種類だけを持ち、秘密値・接続文字列を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessError {
    kind: HarnessErrorKind,
    component: ComponentKind,
}

impl HarnessError {
    /// 誤りを作る。
    pub fn new(kind: HarnessErrorKind, component: ComponentKind) -> Self {
        Self { kind, component }
    }

    /// 誤りの種類。
    pub fn kind(&self) -> HarnessErrorKind {
        self.kind
    }

    /// 対象の構成要素の種類。
    pub fn component(&self) -> ComponentKind {
        self.component
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "harness error: {:?} ({:?})", self.kind, self.component)
    }
}

impl std::error::Error for HarnessError {}

/// 起動する構成要素の並び。既定で構成要素を足さない。
#[derive(Default)]
pub struct HarnessConfig {
    components: Vec<Box<dyn Component>>,
}

impl HarnessConfig {
    /// 空の構成。
    pub fn new() -> Self {
        Self::default()
    }

    /// 構成要素を足す。順序は渡した順ではなく [`ComponentKind::START_ORDER`] で決まる。
    #[must_use]
    pub fn with(mut self, component: Box<dyn Component>) -> Self {
        self.components.push(component);
        self
    }
}

/// 構成の記録の 1 項目。種類と、stub（実物ではない）かの印だけを持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionEntry {
    pub kind: ComponentKind,
    /// stub なら `true`。stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）。
    pub stub: bool,
}

/// 起動した構成の記録（起動順）。設定の値・接続情報・鍵を載せない（SECR-11・SECR-23）。
/// 版・日時・ファイルへの出力は [`crate::record::RunRecord`]（5/6。#94）が持つ。文言は仮置きで人が確認する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionRecord {
    pub entries: Vec<CompositionEntry>,
}

impl fmt::Display for CompositionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "結合試験の構成（stub の合格を実物の合格に数えない。A-2026-10-28-9・ACPT-7）"
        )?;
        for e in &self.entries {
            if e.stub {
                writeln!(f, "{:?}: stub（実物ではない）", e.kind)?;
            } else {
                writeln!(f, "{:?}: 構成要素", e.kind)?;
            }
        }
        Ok(())
    }
}

/// 起動の入口。
pub struct Harness;

impl Harness {
    /// 決まった順で起動する。重複は起動前に拒否し、途中で失敗したら起動済みを逆順に停止してから誤りを返す。
    pub fn start(config: HarnessConfig) -> Result<RunningHarness, HarnessError> {
        let mut components = config.components;
        for (i, c) in components.iter().enumerate() {
            if components[..i].iter().any(|p| p.kind() == c.kind()) {
                return Err(HarnessError::new(HarnessErrorKind::Duplicate, c.kind()));
            }
        }
        components.sort_by_key(|c| c.kind().rank());

        let mut running = RunningHarness {
            components: Vec::new(),
            stopped: false,
            started_at: SystemTime::now(),
        };
        for mut c in components {
            if let Err(e) = c.start() {
                // 失敗した要素は起動済みに数えない。後始末の失敗より起動の誤りを返す。
                let _ = running.stop_all();
                return Err(e);
            }
            running.components.push(c);
        }
        Ok(running)
    }
}

/// 起動済みのハーネス。`Drop` でも停止を試みる（試験の panic で取り残さない）。
pub struct RunningHarness {
    components: Vec<Box<dyn Component>>,
    stopped: bool,
    /// 起動の開始時刻（実行の記録の `started_at`）。
    started_at: SystemTime,
}

impl RunningHarness {
    /// 起動している構成要素の種類（起動順）。
    pub fn kinds(&self) -> Vec<ComponentKind> {
        self.components.iter().map(|c| c.kind()).collect()
    }

    /// 起動した構成の記録（起動順）。stub は `stub == true` の印で残る（A-2026-10-28-9・ACPT-7）。
    pub fn composition_record(&self) -> CompositionRecord {
        CompositionRecord {
            entries: self
                .components
                .iter()
                .map(|c| CompositionEntry {
                    kind: c.kind(),
                    stub: c.kind().is_stub(),
                })
                .collect(),
        }
    }

    /// 実行の記録（版・構成・日時）を作る。stub の記録は `RunRecord::with_*` で取り込む（5/6）。
    pub fn run_record(&self, version: VersionInfo) -> Result<RunRecord, RecordError> {
        Ok(RunRecord::new(
            UtcTimestamp::from_system_time(self.started_at)?,
            UtcTimestamp::from_system_time(SystemTime::now())?,
            version,
            self.composition_record(),
        ))
    }

    /// 逆順に停止する。最初の失敗を返しつつ、残りも停止を試みる。
    pub fn stop(mut self) -> Result<(), HarnessError> {
        self.stop_all()
    }

    fn stop_all(&mut self) -> Result<(), HarnessError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        let mut first = None;
        for c in self.components.iter_mut().rev() {
            if let Err(e) = c.stop() {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

impl Drop for RunningHarness {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}
