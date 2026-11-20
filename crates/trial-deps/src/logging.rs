//! 秘密値を `tracing` で出す試し（SECR-11・SECR-23）。
//!
//! 出す側の口だけを持ち、subscriber は選ばない（`D-2026-11-20-5`）。ログの文言は英語（`D-2026-11-20-13`）。

use fandhe_auth_secret::SecretString;

/// 秘密値を派生 `Debug` の入れ物に入れた試験用の構造体。
/// 派生 `Debug` を付けても値が出ないこと（SECR-13・SECR-30）を確かめるために使う。
///
/// 注記: 秘密値を持つ試験用の型で、SECR-10・SECR-29 の型の一覧には載せない。TASK-512・TASK-514 の走査が
/// 入る前に本 crate を消すか、走査を入れるときに一覧の外の型としての扱いを決める。
#[derive(Debug)]
pub struct TokenHolder {
    /// 入れ物の名前。秘密値ではない。
    pub label: String,
    /// 秘密値。専用の型で包む（SECR-30）。
    pub token: SecretString,
}

/// `SecretString` を `?` 形式の欄として出す（SECR-11・SECR-23）。
/// 期待: 出力に値が現れず、固定文字列だけが現れる。
pub fn log_secret_debug_field(secret: &SecretString) {
    tracing::info!(token = ?secret, "secret field emitted");
}

/// 派生 `Debug` の入れ物を `?` 形式の欄として出す（SECR-13・SECR-23）。
pub fn log_holder_debug_field(holder: &TokenHolder) {
    tracing::info!(holder = ?holder, "holder field emitted");
}
