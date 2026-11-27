//! 計測・集計・負荷の生成の部品を置く crate（TASK-12 3/10〜8/10）。試験専用で、本番のビルドには入らない。
//!
//! 役割: 1 回の「回」の中で、試行ごとに区間の時間を単調な時計で測り、標本・件数・実測の最大同時実行数を
//! 集める。集めた結果（[`RunSamples`]）は [`aggregate`]（4/10）が百分位・最大・最悪の回に集計する。
//! [`generate`]（5/10）は閉ループで対象を並行に繰り返し呼ぶ。呼び出し元は負荷の対象を呼ぶ
//! ハーネスの試験（試験ビルド）だけ。指標の定義は `docs/harness/load.md`。
//! [`condition`]（7/10）は測定の条件（機材・構成・負荷の型）を記録の行に出す。
//! [`stage`]（8/10）は並行数を段階的に変えて回し、結果を段階ごとに分けて記録の行（`stage`・`run`・`worst_run` 等）に出す。
//!
//! [`record`]（9/10）は記録のファイル全体（`format:`・`run_record_file:`・`value:`・`judgment:` を含む）を組んで新規ファイルへ書き出す
//! （仮置き。人が確認する）。
//!
//! 範囲外: 障害注入（`TASK-13`）、負荷の見積もり（`TASK-15`）、
//! 合格の値の導出（`TASK-21`・`AUDP-29`）、結合試験（`TASK-10`）。合否は付けない（`ACPT-7`）。
//!
//! 依存の向き: どの crate もこの crate に依存しない（LIB-2・LIB-4）。段階 1 のリリースのメンバーではない
//! （LIB-14・LIB-15）。名前・置き場は仮称で、人が確定する。
//! 通常の依存は 0（std だけ）。試験（dev）だけ it-harness・hub-stub・secret・ring に依存する（5/10。許可表の行は人の確認待ち）。
//!
//! 安全: 検証を外す意味の分岐・feature は作らない（DPOL-30〜32）。環境変数は読まず、設定ファイルを自分では探さない（DPOL-32）。
//! 自分で読むファイルは Linux の `/proc/meminfo`（固定パス・上限付き・整数だけ取り出す。設定ではなく OS の情報）だけ。
//! 書くのは `write_new` が呼び出し元の渡したディレクトリへ書く記録だけ（ディレクトリの作成・一時ファイル・`hard_link` による公開を含む）。
//! 例外として、[`ApprovedValues::load`] は呼び出し元が渡したパスのファイル（承認一覧）を形式の検査つきで読む
//! （現在は照合に使わない。TODO(TASK-12): 承認一覧の置き場・形・承認の手順が決まったら照合を戻す。#111）。
//! 記録に出す名前・出典・区間名の語彙は crate の固定の定数で、呼び出し元は足せない（#6038）。
//! 誤りの値に名前の中身・パスを載せない（SECR-11・SECR-23）。

#![forbid(unsafe_code)]

pub mod aggregate;
pub mod condition;
pub mod generate;
pub mod meter;
pub mod record;
pub mod stage;
pub mod target;

pub use aggregate::{
    AggregateError, IntervalStats, MetricKey, RunSummary, summarize_interval, summarize_run,
    worst_run,
};

pub use condition::{
    BuildProfile, ComponentConditions, ComponentState, ConditionComponent, ConditionError,
    MachineCondition, MachineProbe, MeasurementConditions, StdMachineProbe, Virtualization,
};

pub use generate::{LoadConfig, LoadError, LoadRun, LoopModel, MAX_CONCURRENCY, run_closed_loop};

pub use meter::{
    FailureKind, IntervalName, MeterError, MonotonicClock, Outcome, RunMeter, RunSamples, Sample,
    StdMonotonicClock, Trial,
};

pub use record::{
    ApprovedValues, ComparisonValue, InvalidValueReason, LOAD_RECORD_FORMAT, LoadRecord,
    LoadRecordError, MeasuredSource, REF_IDS, RefId, RunEvidence, RunRecordFile, RunSession,
    VALUE_NAMES, ValueClass, ValueName, ValueUnit,
};

pub use stage::{
    RECORD_INTERVALS, StageConfig, StageError, StageLimits, StagePlan, StageResult, StagedRun,
    run_stages,
};

pub use target::{CallResult, TargetError, TargetName, TargetRun, TargetSpec, run_target};
