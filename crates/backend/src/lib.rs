//! `fandhe-backend` に依る部分を持つ crate。いまは TASK-9 (2/8) の差し替え点の枠（`interim` module）だけを持つ。
//!
//! 役割: `fandhe-backend` の部品は提供されるまで自前の暫定の実装で代え、提供版は版を固定して差し替える
//! （`A-2026-10-16-5`）。その差し替えの書き換え箇所を `interim` module に集める。暫定の実装の中身は
//! `TASK-523`（CSRF の Gate）ほか各 TASK が作る。ここでは枠だけを置き、中身は持たない。
//!
//! 段階: spec 上は段階 2 である（LIB-14）。LIB-15 の判定の記録はまだ無く、判定は人が行う
//! （`docs/interim/README.md`）。crate 名は `D-2026-10-27-8`・`D-2026-10-29-2` による。
//! module の構成は暫定で、人が確定する。
//!
//! 担わないもの: Gate の登録・実行、HTTP の受付・待ち受け、環境変数・秘密値の読み込み。これらは組込み先の
//! Hub の担当である（`D-2026-10-30-1`）。設定・秘密値は引数で受ける（DPOL-31・DPOL-32）。
//!
//! 依存の向き: 現時点では workspace 内の crate にも外部の crate にも依存しない。`fandhe-backend` への依存と
//! 版の固定は、差し替えの TASK と版の固定の記録（#85）の形式で行う。

#![forbid(unsafe_code)]

pub mod interim;
