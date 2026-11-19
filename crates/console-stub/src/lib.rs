//! console（`fandhe-dev-console`）の検証用 stub を置く crate（TASK-8）。試験専用で、実物の console ではない。
//!
//! 役割: console の契約は仮定または契約候補のまま進める（A-2026-10-28-12）。その下で、console 側の
//! 呼び出しを模擬して統合を試す拠り所になる。console の実物の数値には根拠がないため、値は設定で受け、
//! stub の値で進める（A-2026-10-28-14・TASK-451）。stub での測定を数値の根拠にしない（TASK-30）。
//!
//! 本体ではないこと:
//! - `publish = false` で、本体（`core`・`secret`・`db-tests`・`client`）と Plugin（`plugin-oidc`）からは
//!   依存しない。この向きは `scripts/check-crate-deps.sh` の許可表で守る（LIB-2・LIB-4）。
//! - 段階 1 のリリースのメンバーではない（LIB-14・LIB-15）。名前・置き場は仮称で、人が確定する。
//! - 本番で console の `aud` を登録しない方針は変えない（OCL-58）。
//!
//! 受け口: introspection の呼び出しの模擬は [`introspection`]（#73）。
//!
//! 設定: console の値は設定で受ける（[`config::ConsoleStubConfig`]）。環境変数・ファイルは読まない
//! （DPOL-32）。

#![forbid(unsafe_code)]

pub mod config;
pub mod introspection;

// TODO(TASK-8): 配送の時点の確認と結果の切り替え（#74）、エージェントの
// 資格情報の発行・失効と呼び出しの記録（#75）、遅延・拒否の注入（#76）、仮定の契約の記録（#77）、
// 単体試験（#78）、実物との差の文書（#79）は後続の段で足す。遅延・拒否の注入は本体の設定に持ち込まない。
