//! E2E の範囲。passkey の登録と認証の 2 操作だけ。
//!
//! 呼び出し元は [`crate::harness::E2eConfig`] と E2E の試験。画面の操作・管理画面（一覧・命名・削除）の変種は
//! 作らない。画面の E2E は Hub の `ASCR` の担当（`TASK-11` の 2026-11-04 の追随）。

/// 対象の passkey の操作（`api-passkey.md`・PKEY の登録と認証）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasskeyOperation {
    /// 登録。
    Registration,
    /// 認証。
    Authentication,
}

impl PasskeyOperation {
    /// 範囲の全件。2 件だけで、増やすときは spec の範囲の変更を先にする。
    pub const ALL: [PasskeyOperation; 2] = [
        PasskeyOperation::Registration,
        PasskeyOperation::Authentication,
    ];
}
