//! 認証コア。I/O を持たないドメインの判定だけを担う crate。
//!
//! 担うもの: 本人・credential・認証セッション・グラントの状態遷移とポリシー（DPOL-1）。
//!
//! 依存しないもの: 非同期ランタイム・DB ドライバ・HTTP サーバー・`fandhe-backend`・
//! `fandhe-frontend`・`web-sys`。現在時刻・乱数・ファイル・ネットワークを直接読まない（DPOL-1）。
//!
//! 時刻・乱数・ID は呼び出し元（server・試験）から引数で受け、同じ入力で同じ出力にする（DPOL-3）。
//! 受け渡しの型・trait の名前は未決で、ここでは定義しない。
//!
//! 本人確認・復旧・権限付与・管理者承認の判定はプログラムが行い、LLM・検索を経由する経路を置かない
//! （HAPR-1・SECR-28）。

#![forbid(unsafe_code)]

pub mod screen_access;
pub mod uniform_failure;

// TODO(TASK-659): 状態遷移とポリシーの実装、時刻・乱数・ID の受け渡しの型を決めて置く。
