//! Hub の検証用 stub を置く crate（TASK-7）。試験専用で、実物の Hub ではない。
//!
//! 役割: 実物の Hub が Hub の契約の文書・Hub の stub での試験・照合表のとおりに振る舞うという仮定
//! （A-2026-10-28-9）の下で、統合を試す拠り所になる。最小の組込み先（TASK-716）の代わりではなく、
//! 組込み先の背後にいる相手側の模擬である。
//!
//! 本体ではないこと:
//! - `publish = false` で、本体（`core`・`secret`・`db-tests`・`client`）と Plugin（`plugin-oidc`）からは
//!   依存しない。この向きは `scripts/check-crate-deps.sh` の許可表で守る（LIB-4）。
//! - 段階 1 のリリースのメンバーではない（LIB-14）。名前・置き場は仮称で、人が確定する。
//!
//! 設定: Hub の値は設定で受ける（[`config::HubStubConfig`]）。数値の正本は Hub の ID で、ここには
//! 書かない。本番の値として扱わない（A-2026-10-28-1）。環境変数・ファイルは読まない（DPOL-32）。
//!
//! 本人トークンの提示の受け口: [`presentation::IdentityTokenReceiver`]（#60）。

#![forbid(unsafe_code)]

pub mod config;
pub mod presentation;

// TODO(TASK-7): 各模擬・受け口（#61〜#65）、遅れ・拒否の注入（#66）、仮置きの値の明示（#67）、
// 単体試験（#68）、実物との差の文書（#69・#70）は後続の段で足す。
