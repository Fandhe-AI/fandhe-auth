//! 故障なしの基準線と注入ありの実行を組で流す入口（TASK-13 の 7/8）。試験専用で、本番のビルドには入らない。
//!
//! 役割: 同じシナリオの本体を、モードで分岐させずに基準線・注入ありの順で 2 回流し、`run_mode` の違う 2 つの記録を
//! 組で返す。呼び出し元は結合試験（試験ビルド）だけ。呼び出し先は [`super::record`]。基準線の記録は注入ありと同じキーを
//! 出し、`injection_count: 0` になる（形式の版は変えない）。記録は事実の出力で、合否の判定でも spec の「確定」でもない（ACPT-7）。
//!
//! 設計上の約束:
//! - 基準線を先に流す。注入の後の状態（共有のずれ・有効な規則）を基準線へ持ち越さないため。
//! - 本体は実行ごとにタイムラインを受け、確定した記録を返す。確定の経路は注入器の結合で決まり、本体だけが知っている。
//!   注入器は本体の中で実行ごとに作る（注入器を実行をまたいで使わない約束を保つ）。
//! - 本体は `FaultTimeline::inject` で注入の段を通す。基準線では操作を呼ばず段だけを残し、注入ありでは操作が積んだ事象と
//!   照合した段を残す。2 つの記録の段の並びが違えば `ShapeMismatch`（fail-closed。本体がモードで分岐した等）。
//! - 返った記録が渡したタイムラインの実行のものでなければ `RunMismatch`（モードの取り違えもここで捕まる）。
//! - 誤りに値・パス・ポートを載せない（SECR-11・SECR-23〜27）。環境変数・設定ファイルは読まない（DPOL-32）。
//!   検証を外す分岐・feature を作らない（DPOL-30〜32）。
//!
//! 名前・形は仮置きで、人が確認する。

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use super::{FaultRecord, FaultRecordError, FaultTimeline, RunMode};

/// 基準線と注入ありの組。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedRecords {
    baseline: FaultRecord,
    fault: FaultRecord,
}

impl PairedRecords {
    /// 故障なしの基準線の記録。
    pub fn baseline(&self) -> &FaultRecord {
        &self.baseline
    }

    /// 注入ありの記録。
    pub fn fault(&self) -> &FaultRecord {
        &self.fault
    }

    /// 2 つを別々の新規ファイルへ書き出す（基準線、注入ありの順）。名前に実行の種別が入るので分かれる。
    pub fn write_new(&self, dir: &Path) -> Result<(PathBuf, PathBuf), FaultRecordError> {
        let b = self.baseline.write_new(dir)?;
        let f = self.fault.write_new(dir)?;
        Ok((b, f))
    }
}

/// [`FaultPair::run`] の誤り。`Body` の `E` は本体の型で、本体が値を載せない責任を持つ。
pub enum PairError<E> {
    /// 本体が失敗した（どちらの実行か）。
    Body { mode: RunMode, error: E },
    /// タイムラインを作れなかった（時計が epoch より前）。
    Start(FaultRecordError),
    /// 返った記録が渡したタイムラインの実行のものでない。
    RunMismatch { mode: RunMode },
    /// 2 つの記録の注入の段の並びが一致しない。読み手が確かめられるよう両方を返す。
    ShapeMismatch(Box<PairedRecords>),
}

impl<E> fmt::Debug for PairError<E> {
    /// 種類だけを出す（本体の誤りの値は出さない。SECR-23〜27）。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body { mode, .. } => f
                .debug_struct("Body")
                .field("mode", mode)
                .finish_non_exhaustive(),
            Self::Start(e) => f.debug_tuple("Start").field(e).finish(),
            Self::RunMismatch { mode } => {
                f.debug_struct("RunMismatch").field("mode", mode).finish()
            }
            Self::ShapeMismatch(_) => f.write_str("ShapeMismatch"),
        }
    }
}

impl<E> fmt::Display for PairError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body { .. } => f.write_str("fault pair error: body failed"),
            Self::Start(e) => write!(f, "fault pair error: {e}"),
            Self::RunMismatch { .. } => f.write_str("fault pair error: run mismatch"),
            Self::ShapeMismatch(_) => f.write_str("fault pair error: injection points differ"),
        }
    }
}

/// 組の入口。
pub struct FaultPair;

impl FaultPair {
    /// `body` を基準線、注入ありの順に流し、記録の組を返す。各実行のタイムラインはハーネスの時計で新しく作る。
    ///
    /// # Errors
    /// [`PairError`] を参照。基準線で失敗したら注入ありは流さない。
    pub fn run<E>(
        mut body: impl FnMut(FaultTimeline) -> Result<FaultRecord, E>,
    ) -> Result<PairedRecords, PairError<E>> {
        let baseline = Self::one(RunMode::Baseline, &mut body)?;
        let fault = Self::one(RunMode::Fault, &mut body)?;
        if baseline.injection_points() != fault.injection_points() {
            return Err(PairError::ShapeMismatch(Box::new(PairedRecords {
                baseline,
                fault,
            })));
        }
        // 注入ありの全事象が照合済みの段と一対一で対応すること（`inject` を経ない直接記録を通さない）。
        let ev = fault.events();
        let pts = fault.injection_points();
        if ev.len() != pts.len()
            || !ev
                .iter()
                .zip(pts)
                .all(|(e, p)| e.kind == p.kind && e.phase == p.phase)
        {
            return Err(PairError::ShapeMismatch(Box::new(PairedRecords {
                baseline,
                fault,
            })));
        }
        Ok(PairedRecords { baseline, fault })
    }

    fn one<E>(
        mode: RunMode,
        body: &mut impl FnMut(FaultTimeline) -> Result<FaultRecord, E>,
    ) -> Result<FaultRecord, PairError<E>> {
        let origin = Instant::now();
        let started = SystemTime::now();
        let tl = match mode {
            RunMode::Baseline => FaultTimeline::baseline(origin, started),
            RunMode::Fault => FaultTimeline::start(origin, started),
        }
        .map_err(PairError::Start)?;
        let run_id = tl.run_id();
        let record = body(tl).map_err(|error| PairError::Body { mode, error })?;
        if record.run_id() != run_id || record.mode() != mode {
            return Err(PairError::RunMismatch { mode });
        }
        Ok(record)
    }
}
