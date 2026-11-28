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
//! 2. [`RecoverySecretKind`] の 4 種の値は運べない。自由文・バイト列のフィールドも、それらを
//!    受け取るコンストラクタ・変換も持たない。これは TASK-775 の設計（〔提案〕）で、
//!    `UISF-11` の原文（4 種の値をブラウザ側のストレージに置かない）とは別の規則である。
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
/// （通知の内容の規則 2。`SECR-23`）。
///
/// # 組込み先への要求
///
/// `#[non_exhaustive]`（`LIB-22`）のため、組込み先の `match` は wildcard の腕が要る。
/// その腕で注記を出さない側に倒してはならない。未知の列挙子は注記を出す側に倒す
/// （`UISF-15`。規則 4）。外すかどうかは人の判断で、ここでは外さない。
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
/// let _ = "x".parse::<RecoveryNotice>();
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::recovery_notice::RecoveryNotice;
/// let _ = RecoveryNotice::default();
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
// TODO(#5837): wildcard の腕で注記を出さない側に倒さないこと（UISF-15）を契約テストで確かめる。
// TODO(#5835): 注記を出す画面と段の列挙（UISF-15）との結び付け。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecoveryNotice {
    /// Vault の鍵は復旧しない旨の固定の注記（`UISF-25`・`UISF-15`・`LIFE-35`・Hub `RECOVERY-4`）。
    VaultKeyNotRestored,
}

/// 通知の内容に入れない値の種類。`UISF-11` が名指す 4 種の値（ブラウザ側のストレージに置かない）と
/// 同じ 4 種を、通知の内容の規則 2（設計。〔提案〕）でも運ばない値として一覧にする（`SECR-23`）。
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
    /// 4 種の一覧。列挙子を足したら、ここと試験の網羅の `match` を同じ変更で更新する。
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

    // 通知の内容の規則 1・2（設計）: 単位の形の列挙子だけであることを、wildcard なしの match で固定する。
    // 列挙子を足す・値を持つ形にすると、この match がコンパイルで落ちる。
    #[test]
    fn notice_variants_are_unit_shaped() {
        fn shape(n: RecoveryNotice) {
            match n {
                RecoveryNotice::VaultKeyNotRestored => {}
            }
        }
        shape(RecoveryNotice::VaultKeyNotRestored);
    }

    // 通知の内容の規則 2（設計）: 網羅の match で列挙子ごとの番号を持ち、ALL が 0..len を重複なく
    // 覆うことを確かめる。列挙子を足すと match がコンパイルで落ち、番号の更新で ALL の漏れも落ちる。
    #[test]
    fn secret_kinds_all_matches_exhaustive_variants() {
        const fn ordinal(k: RecoverySecretKind) -> usize {
            match k {
                RecoverySecretKind::InviteValue => 0,
                RecoverySecretKind::BootstrapValue => 1,
                RecoverySecretKind::RecoveryCode => 2,
                RecoverySecretKind::ReinviteValue => 3,
            }
        }
        // 最後の番号 + 1。列挙子を足したらここも更新し、ALL の要素数と食い違えば落ちる。
        const COUNT: usize = ordinal(RecoverySecretKind::ReinviteValue) + 1;
        let all = RecoverySecretKind::ALL;
        assert_eq!(all.len(), COUNT);
        let mut seen = [false; COUNT];
        for k in all {
            let o = ordinal(k);
            assert!(o < COUNT && !seen[o]);
            seen[o] = true;
        }
        assert!(seen.iter().all(|s| *s));
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
