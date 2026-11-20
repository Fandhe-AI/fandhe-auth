//! 秘密値を `tracing` で出す試し（SECR-11・SECR-23）。
//!
//! 出す側の口だけを持ち、subscriber は選ばない（`D-2026-11-20-5`）。ログの文言は英語（`D-2026-11-20-13`）。

use fandhe_auth_secret::SecretString;

/// `SecretString` を `?` 形式の欄として出す（SECR-11・SECR-23）。
/// 期待: 出力に値が現れず、固定文字列だけが現れる。
pub fn log_secret_debug_field(secret: &SecretString) {
    tracing::info!(token = ?secret, "secret field emitted");
}

/// std の入れ物（tuple・`Option`・slice）に入れた `SecretString` を `?` 形式の欄として出す（SECR-13・SECR-23）。
/// std の派生 `Debug` は中身の `Debug` に委ねるため、期待は値が現れず固定文字列だけが現れること。
///
/// ラベルは関数の中で固定し、呼び出し元から任意の `&str` を受けない（SECR-23。受けた文字列は
/// マスクされずに記録されるため）。
///
/// 注記: 秘密値を持つ名前付きの型をここで新しく定義しない。SECR-10・SECR-29 の型の一覧（TASK-512 で作る）が
/// 無い間に、一覧の外の型を増やさないためである。自前の構造体に `#[derive(Debug)]` を付けた入れ物の試験は
/// TASK-513 の範囲である。
pub fn log_container_debug_fields(
    secret: &SecretString,
    maybe: &Option<SecretString>,
    list: &[SecretString],
) {
    let pair = ("label", secret);
    tracing::info!(pair = ?pair, maybe = ?maybe, list = ?list, "container fields emitted");
}
