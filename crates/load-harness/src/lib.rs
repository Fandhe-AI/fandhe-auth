//! 計測・負荷のハーネスの計測の部品を置く crate（TASK-12 3/10）。試験専用で、本番のビルドには入らない。
//!
//! 役割: 1 回の「回」の中で、試行ごとに区間の時間を単調な時計で測り、標本・件数・実測の最大同時実行数を
//! 集める。集めた結果（[`RunSamples`]）は 4/10 以降の集計と記録の出力が受け取る。呼び出し元は負荷の対象を呼ぶ
//! ハーネスの試験（試験ビルド）だけ。指標の定義は `docs/harness/load.md`。
//!
//! 範囲外: 百分位・最悪の回の計算と記録の出力（4/10 以降）、障害注入（`TASK-13`）、負荷の見積もり（`TASK-15`）、
//! 合格の値の導出（`TASK-21`・`AUDP-29`）、結合試験（`TASK-10`）。合否は付けない（`ACPT-7`）。
//!
//! 依存の向き: どの crate もこの crate に依存しない（LIB-2・LIB-4）。段階 1 のリリースのメンバーではない
//! （LIB-14・LIB-15）。名前・置き場は仮称で、人が確定する。依存は 0。
//!
//! 安全: 検証を外す意味の分岐・feature は作らない（DPOL-30〜32）。環境変数・設定ファイルを読まない（DPOL-32）。
//! 誤りの値に名前の中身・パスを載せない（SECR-11・SECR-23）。

#![forbid(unsafe_code)]

pub mod meter;

pub use meter::{
    FailureKind, IntervalName, MeterError, MonotonicClock, Outcome, RunMeter, RunSamples, Sample,
    StdMonotonicClock, Trial,
};
