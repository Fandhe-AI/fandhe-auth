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
//! 各値は [`config::StubValue`] と [`config::ValueMark`] で「仮置き」の印を必ず持ち、
//! [`config::HubStubConfig::provisional_record`] で記録に残す（#67）。
//!
//! 本人トークンの提示の受け口: [`presentation::IdentityTokenReceiver`]（#60）。
//! `private_key_jwt` の assertion で口を呼ぶ模擬: [`assertion::HubAssertionCaller`]（#61）。
//! 失効の通知（`logout_token`）の受け口: [`logout::LogoutTokenReceiver`]（#62）。
//! 変更のフィードの取得の模擬: [`feed::ChangeFeedFetcher`]（#63）。
//! 生存確認の受け口: [`heartbeat::HeartbeatReceiver`]（#64）。
//! Hub の JWKS を返す模擬: [`jwks::HubJwksEndpoint`]（#65）。
//! Hub が `fandhe-auth` の JWKS を取得する模擬: [`jwks::AuthJwksFetcher`]（#65）。
//! 応答の遅れ・拒否の注入: [`fault::HubFaultInjector`]（#66）。

#![forbid(unsafe_code)]

pub mod assertion;
pub mod config;
pub mod fault;
pub mod feed;
pub mod heartbeat;
pub mod jwks;
pub mod logout;
pub mod presentation;

// TODO(TASK-7): 実物との差の文書（#69・#70）は後続の段で足す。
