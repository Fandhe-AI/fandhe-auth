//! 障害注入の記録の形式（TASK-13 の 1/8）。注入した実行と基準線（故障なし）の実行で同じキーを出す。
//!
//! 役割: 「いつ・どの種類を・どのパラメータで注入したか」を読み手が確かめられるように残す。呼び出し元は
//! 結合試験（試験ビルド）だけ。記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。
//!
//! 設計上の約束:
//! - 時刻はハーネスのプロセスの基準の時計で押す。試験対象に注入した、ずれた時計や stub の引数の `now` は使わない
//!   （時計のずれを注入しても記録の時刻はずれない。AVLB-31）。壁時計 `at`（秒）と、実行開始からの単調時計の
//!   経過 `offset_ms` を持つ。hub-stub の `InjectionLog.at`（試験の論理時刻）は
//!   ここへ入れず、`HubStubFaultInjector::call_log` で別に読む（対応は順序だけ。形式の版は変えない）。
//! - 基準線でも同じキーを出す。節を省くと「記録しなかった」と見分けられないため（TASK-10 の `not_started` と同じ）。
//! - 基準線に事象を積むのも、Fault の実行で 0 件のまま記録を作るのも誤り（fail-closed）。
//! - 記録は enum・数値・静的な名前だけで組む。ホスト名・ポート・パス・接続情報・実行時の文字列を持たない
//!   （SECR-11・SECR-23・SECR-26）。環境変数・設定ファイルは読まない（DPOL-32）。
//! - 形式の識別子・キー名・文言は仮置きで、人が確認する。`RunRecord` とは `run_started_at` で結ぶ（結ぶ形も仮置き）。

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

use super::{FaultKind, FaultPhase, FaultTarget};
use crate::record::UtcTimestamp;

/// 記録の形式の識別子（仮置き）。形式を変えたら末尾の版を上げる。
pub const FAULT_RECORD_FORMAT: &str = "fandhe-auth-it-fault-record/1";

/// 実行の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// 故障なしの基準線（F0）。
    Baseline,
    /// 注入ありの実行。
    Fault,
}

/// 誤りの種類。パス・`io::Error` の文言・パラメータの値を載せない（SECR-11・SECR-23）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultRecordError {
    /// 時刻が epoch より前。
    ClockBeforeEpoch,
    /// パラメータの検査に落ちた（0 の遅れ・0 のずれ等）。
    InvalidParam,
    /// 基準線の実行に事象を積もうとした。
    EventsInBaseline,
    /// Fault の実行で事象が 0 件のまま記録を作ろうとした。
    NoInjectionInFaultRun,
    /// 対応する `Applied` が無い `Released`。
    ReleaseWithoutApply,
    /// 解除の無い一回きりの事象（`SIGTERM`・`SIGKILL`）に `Released` を積もうとした。
    ReleaseOfInstantaneous,
    /// `offset_ms` が前の件より小さい。
    OffsetWentBackwards,
    /// ファイルの書き込みに失敗した。
    WriteFailed,
}

impl fmt::Display for FaultRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::ClockBeforeEpoch => "clock is before the epoch",
            Self::InvalidParam => "invalid parameter",
            Self::EventsInBaseline => "events in baseline run",
            Self::NoInjectionInFaultRun => "no injection in fault run",
            Self::ReleaseWithoutApply => "release without apply",
            Self::ReleaseOfInstantaneous => "release of instantaneous fault",
            Self::OffsetWentBackwards => "offset went backwards",
            Self::WriteFailed => "write failed",
        };
        write!(f, "fault record error: {s}")
    }
}

impl std::error::Error for FaultRecordError {}

/// 記録の 1 件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultEvent {
    pub kind: FaultKind,
    pub phase: FaultPhase,
    /// ハーネスの壁時計（秒）。
    pub at: UtcTimestamp,
    /// 実行の開始からの単調時計の経過（ミリ秒）。
    pub offset_ms: u64,
    /// 事象を積んだ時点の、呼び出しの記録（hub-stub の `InjectionLog`）の件数。`None` は呼び出しの記録を持たない種類。
    /// 呼び出し `i`（0 始まり）は、`log_pos <= i` の事象より後・次の事象より前に起きた。
    /// 同じ口へ同じ障害を二度適用・解除しても、どの期間の呼び出しかを照合できる（TASK-13 の 5/8）。
    pub log_pos: Option<usize>,
}

/// 事象を積む入口。`Instant` の原点と開始時刻を持つ。
#[derive(Debug, Clone)]
pub struct FaultTimeline {
    run_id: u64,
    mode: RunMode,
    started_at: UtcTimestamp,
    origin: Instant,
    events: Vec<FaultEvent>,
    open: Vec<FaultKind>,
}

impl FaultTimeline {
    /// 注入ありの実行を始める。`origin` は単調時計の原点、`started` は壁時計の開始時刻。
    pub fn start(origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        Self::new(RunMode::Fault, origin, started)
    }

    /// 故障なしの基準線の実行を始める。事象は積めない。
    pub fn baseline(origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        Self::new(RunMode::Baseline, origin, started)
    }

    fn new(mode: RunMode, origin: Instant, started: SystemTime) -> Result<Self, FaultRecordError> {
        let started_at = UtcTimestamp::from_system_time(started)
            .map_err(|_| FaultRecordError::ClockBeforeEpoch)?;
        static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
        Ok(Self {
            run_id: RUN_SEQ.fetch_add(1, Ordering::Relaxed),
            mode,
            started_at,
            origin,
            events: Vec::new(),
            open: Vec::new(),
        })
    }

    /// この実行の識別（プロセス内で一意）。注入器を単一の実行へ結ぶために使う。
    pub(crate) fn run_id(&self) -> u64 {
        self.run_id
    }

    /// ハーネスの時計で現在の時刻を押して積む。
    pub fn record_now(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
    ) -> Result<(), FaultRecordError> {
        self.record_now_with_log_pos(kind, phase, None)
    }

    /// `record_now` に、呼び出しの記録の件数（`FaultEvent::log_pos`）を添える。
    pub fn record_now_with_log_pos(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        log_pos: Option<usize>,
    ) -> Result<(), FaultRecordError> {
        let at = UtcTimestamp::from_system_time(SystemTime::now())
            .map_err(|_| FaultRecordError::ClockBeforeEpoch)?;
        let ms = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.push(kind, phase, at, ms, log_pos)
    }

    /// 時刻を引数で渡して積む（試験用の決定的な入口）。
    ///
    /// # Errors
    /// 基準線への積み込み・パラメータの検査落ち・`offset_ms` の逆行・対応する `Applied` の無い `Released`・
    /// 一回きりの事象への `Released`。誤りのときは何も積まない。
    pub fn record_at(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        at: UtcTimestamp,
        offset_ms: u64,
    ) -> Result<(), FaultRecordError> {
        self.push(kind, phase, at, offset_ms, None)
    }

    fn push(
        &mut self,
        kind: FaultKind,
        phase: FaultPhase,
        at: UtcTimestamp,
        offset_ms: u64,
        log_pos: Option<usize>,
    ) -> Result<(), FaultRecordError> {
        if self.mode == RunMode::Baseline {
            return Err(FaultRecordError::EventsInBaseline);
        }
        kind.validate()
            .map_err(|_| FaultRecordError::InvalidParam)?;
        if self.events.last().is_some_and(|e| offset_ms < e.offset_ms) {
            return Err(FaultRecordError::OffsetWentBackwards);
        }
        match phase {
            FaultPhase::Applied => {
                if !kind.is_instantaneous() {
                    self.open.push(kind);
                }
            }
            FaultPhase::Released => {
                if kind.is_instantaneous() {
                    return Err(FaultRecordError::ReleaseOfInstantaneous);
                }
                let i = self
                    .open
                    .iter()
                    .position(|k| *k == kind)
                    .ok_or(FaultRecordError::ReleaseWithoutApply)?;
                self.open.remove(i);
            }
        }
        self.events.push(FaultEvent {
            kind,
            phase,
            at,
            offset_ms,
            log_pos,
        });
        Ok(())
    }

    /// 記録を作る。Fault の実行で 0 件なら誤り（何も注入していない故障の試験を作らない）。
    /// 解除しなかった `Applied` は `unreleased_count` に数える（黙って省かない）。
    pub fn finish(self, recorded_at: UtcTimestamp) -> Result<FaultRecord, FaultRecordError> {
        if self.mode == RunMode::Fault && self.events.is_empty() {
            return Err(FaultRecordError::NoInjectionInFaultRun);
        }
        Ok(FaultRecord {
            mode: self.mode,
            started_at: self.started_at,
            recorded_at,
            unreleased: self.open.len(),
            events: self.events,
        })
    }
}

/// 障害注入の記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultRecord {
    mode: RunMode,
    started_at: UtcTimestamp,
    recorded_at: UtcTimestamp,
    unreleased: usize,
    events: Vec<FaultEvent>,
}

impl FaultRecord {
    /// 実行の種別。
    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// 積んだ事象（積んだ順）。
    pub fn events(&self) -> &[FaultEvent] {
        &self.events
    }

    /// 解除しなかった注入の件数。
    pub fn unreleased_count(&self) -> usize {
        self.unreleased
    }

    /// 新規のファイルへ書き出す。`RunRecord::write_new` と同じ約束（無ければ `dir` を作る・上書きしない・
    /// 乱数を使わないファイル名）。
    pub fn write_new(&self, dir: &Path) -> Result<PathBuf, FaultRecordError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir).map_err(|_| FaultRecordError::WriteFailed)?;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "it-fault-{}-{}-{n}.txt",
            self.started_at.unix_seconds(),
            std::process::id()
        );
        let path = dir.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| FaultRecordError::WriteFailed)?;
        file.write_all(self.to_string().as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| FaultRecordError::WriteFailed)?;
        Ok(path)
    }
}

impl fmt::Display for FaultRecord {
    /// 1 行 1 項目の `key: value`（UTF-8・LF）。キーは ASCII 固定。基準線でも同じキーを出す。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            RunMode::Baseline => "baseline",
            RunMode::Fault => "fault",
        };
        writeln!(f, "format: {FAULT_RECORD_FORMAT}")?;
        writeln!(f, "run_mode: {mode}")?;
        writeln!(f, "run_started_at: {}", self.started_at)?;
        writeln!(f, "recorded_at: {}", self.recorded_at)?;
        writeln!(f, "injection_count: {}", self.events.len())?;
        writeln!(f, "unreleased_count: {}", self.unreleased)?;
        for (i, e) in self.events.iter().enumerate() {
            let phase = match e.phase {
                FaultPhase::Applied => "applied",
                FaultPhase::Released => "released",
            };
            write!(
                f,
                "injection: {} {phase} {} target={} at={} offset_ms={}{}",
                i + 1,
                e.kind.category().name(),
                e.kind.target().name(),
                e.at,
                e.offset_ms,
                e.kind
            )?;
            if let Some(n) = e.log_pos {
                write!(f, " log_pos={n}")?;
            }
            writeln!(f)?;
        }
        writeln!(
            f,
            "note: 記録は事実の出力で、合否の判定でも spec の確定でもない（ACPT-7）"
        )?;
        if self
            .events
            .iter()
            .any(|e| e.kind.target() == FaultTarget::HubStub)
        {
            writeln!(
                f,
                "note: stub の合格を実物の合格に数えない（A-2026-10-28-9・ACPT-7）"
            )?;
        }
        Ok(())
    }
}
