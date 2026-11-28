//! 復旧と招待の画面が出す固定の注記を、種類として表す型（TASK-775 の 1/8。骨組み）。
//!
//! 役割: 復旧の全段と再招待の受け取りの画面に出す固定の注記（`UISF-25`・`UISF-15`）を
//! 「種類」だけで表し、通知の内容の規則を型の形で持つ。呼び出し元は組込み先（Hub）の画面の実行で、
//! 本体はどの注記を出すべきかの判定の入力を型で渡すだけである。
//!
//! 境界:
//! - 文言・表示・要素の識別子・`vault_notice` の応答の出力は組込み先の担当（`LIFE-35` の追記で
//!   Hub `ASCR-58` に分けられた。`A-2026-10-30-11`・`D-2026-10-30-1`）。本体は文言を持たない。
//! - 本人への通知の配送（メールと画面の 2 経路・再送）も組込み先の担当（`LIFE-22` → Hub `AEVT-38`）。
//!   本体は配送せず、I/O・時刻・乱数を持たない（`DPOL-1`）。
//!
//! 通知の内容の規則（設計）:
//! 1. 通知の値は種類だけを持ち、要求ごとに変わる値を持たない。
//! 2. `UISF-11` が名指す 4 種の値（[`RecoverySecretKind`]）は運べない。自由文・バイト列の
//!    フィールドも、それらを受け取るコンストラクタ・変換も持たない。
//! 3. `Debug` は固定で `Display` は実装しない（`SECR-11`・`SECR-12`・`SECR-23`）。
//! 4. 組込み先は未知の列挙子を「注記を出さない」側へ倒さない。注記の欠けを安全側と見なさない
//!    （`UISF-15`）。
//!
//! 状態: 型の名前は仮称で、module 名は提案（人の確認待ち）。`UISF-11`・`UISF-25` の確定の範囲は
//! 旧構成の〔仮想〕に限り、`UISF-15` は検討中。型を置いても各行を満たしたことにはならない
//! （`ACPT-7`）。

/// 復旧と招待の画面が出す固定の注記の種類。
///
/// 値を持たない列挙で、`Display`・`From<&str>`・`FromStr`・`Default` は実装しない
/// （`UISF-11`・`SECR-23`）。`#[non_exhaustive]` のため、組込み先は wildcard の腕が要るが、
/// 未知の列挙子を「注記なし」にしてはならない（`UISF-15`）。
///
/// ```compile_fail
/// use fandhe_auth_core::recovery_notice::RecoveryNotice;
/// let _ = RecoveryNotice::from("free text");
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::recovery_notice::RecoveryNotice;
/// let _ = format!("{}", RecoveryNotice::VaultKeyNotRestored);
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::recovery_notice::RecoveryNotice;
/// let n = RecoveryNotice::VaultKeyNotRestored;
/// match n {
///     RecoveryNotice::VaultKeyNotRestored => {}
/// }
/// ```
// TODO(#5832): 復旧の完了の本人への通知（IRUI-16。配送は AEVT-38）を同じ型に足すかを人が決める。
// TODO(#5832): 本体が UISF-25 の文言を定数で持つかは未決（現状は持たない。表示は ASCR-58）。
// TODO(#5835): 注記を出す画面と段の列挙（UISF-15）との結び付け。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecoveryNotice {
    /// Vault の鍵は復旧しない旨の固定の注記（`UISF-25`・`UISF-15`・`LIFE-35`・Hub `RECOVERY-4`）。
    VaultKeyNotRestored,
}

/// ブラウザ側のストレージにも通知の内容にも入れない値の種類（`UISF-11`・`SECR-23`）。
///
/// 値そのものは持たず、種類の名前だけを表す。秘密値の型は `fandhe-auth-secret` の 1 つだけ
/// （`SECR-31`）。後続の走査・契約テスト（#5836・#5837）が引く一覧として [`Self::ALL`] を置く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecoverySecretKind {
    /// 招待の値。
    InviteValue,
    /// bootstrap の値。
    BootstrapValue,
    /// 復旧コード。
    RecoveryCode,
    /// 再招待の値。
    ReinviteValue,
}

impl RecoverySecretKind {
    /// `UISF-11` が名指す 4 種の一覧。
    pub const ALL: [Self; 4] = [
        Self::InviteValue,
        Self::BootstrapValue,
        Self::RecoveryCode,
        Self::ReinviteValue,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    // UISF-11: 前提=通知の型、操作=大きさを測る、期待=値を隠し持つ余地が無い（フィールド無しの列挙）
    #[test]
    fn uisf_11_notice_carries_no_payload() {
        assert!(core::mem::size_of::<RecoveryNotice>() <= 1);
    }

    // UISF-11: 前提=4 種の一覧、操作=組を比べる、期待=4 種が互いに異なる
    #[test]
    fn uisf_11_secret_kinds_are_four_and_distinct() {
        let all = RecoverySecretKind::ALL;
        assert_eq!(all.len(), 4);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    // SECR-23: 前提=Debug 出力、操作=整形、期待=列挙子の名前だけ
    #[test]
    fn secr_23_debug_is_variant_name_only() {
        assert_eq!(
            format!("{:?}", RecoveryNotice::VaultKeyNotRestored),
            "VaultKeyNotRestored"
        );
    }
}
