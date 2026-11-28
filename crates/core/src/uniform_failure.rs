//! 失敗の一様な表示の型（仮称 `UniformFailure`）の骨組み。
//!
//! 役割: 画面に出す失敗が、理由（未登録・登録済み、使用済み・期限切れ・不明など）によって
//! 変わらないことを型で表す。値は 1 種類しか無く、理由を運ぶ列挙子・フィールド・型引数を持たず、
//! 理由を保つ向きの変換と理由を取り出す口も持たない。
//!
//! 根拠:
//! - 1 種類の表示: `UISF-4`・`UISF-29`・`UISF-31`・`UISF-32`・`UISF-34`
//! - 関連: `UISF-26`（復旧への導線を出し分けない）・`UISF-30`（案内に内部の値を出さない）・
//!   `UISF-33`（上限後の表示が同じ）
//! - サーバー側の応答の差 0: `THRT-3`・`THRT-25`・`THRT-27`
//!
//! 境界: 本体は意味だけを持つ。文言・`data-status` などの E2E の識別子・状態コード・描画は
//! Hub の `ASCR` の担当で、ここには置かない（`D-2026-10-30-1`・`A-2026-10-30-11`）。
//! I/O・時刻・乱数を持たない（`DPOL-1`）。
//!
//! 状態: 型の名前は仮称で、対象の行はすべて検討中である（ACPT-7）。この型があることは
//! 各行を満たしたことを意味しない。
//!
//! 後続: 表示の鍵の決定は #5812、各画面への適用は #5813〜#5816、文言の走査は #5817、
//! 応答時間の確かめ方は #5818（許容値は測って決める。`THRT-29`）、試験の仕様は #5819、結線は #5820。

use core::fmt;

// TODO(#5812): 表示の種類（鍵）を 1 種類として決める。文言の本文は Hub の `ASCR-27` が持つ。
// TODO(#5813): 各画面の内部の理由の型から、理由を捨てて畳む変換を足す（理由を保つ向きは作らない）。

/// 失敗の一様な表示を表す値。
///
/// 値は 1 種類で、どの 2 つの値も区別できない。呼び出し元（各画面の判定）は、失敗の理由に関わらず
/// この値だけを返し、表示側（Hub）はこの値から固定の表示を出す。理由は `new()` に渡せず、
/// `Debug` にも出ない（`UISF-4`・`THRT-25`。`Debug` の固定の形は `SECR-11` に倣う）。
///
/// 非公開の欄で外からの構築を禁じているため、`#[non_exhaustive]` は付けない（`LIB-22`）。
///
/// 外から構築できないこと、`Display` が無いことは次で確かめる。各 `compile_fail` は
/// パスの誤りでも合格になるため、同じパスで `new()` を呼ぶ合格の例を先に置く（陽性対照）。
///
/// ```
/// // 陽性対照: 以下の compile_fail と同じパスが解決できる
/// let _ = fandhe_auth_core::uniform_failure::UniformFailure::new();
/// ```
///
/// ```compile_fail
/// // UISF-4: struct literal で作れない
/// let _ = fandhe_auth_core::uniform_failure::UniformFailure { _private: () };
/// ```
///
/// ```compile_fail
/// // UISF-4: 理由の値からの変換が無い
/// let _ = fandhe_auth_core::uniform_failure::UniformFailure::from("expired");
/// ```
///
/// ```compile_fail
/// // UISF-4: 文言は Hub が持つため Display が無い
/// let _ = format!("{}", fandhe_auth_core::uniform_failure::UniformFailure::new());
/// ```
// TODO(#5813): 理由の型ができたら、理由の型からの変換が無いことの compile_fail を足す。
#[must_use]
// Default は値が 1 種類で理由を運ばないため、clippy の new_without_default に従って足す。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UniformFailure {
    // 外から struct literal で作れないようにする。理由を隠し持たない ZST のまま保つ。
    _private: (),
}

impl UniformFailure {
    /// 失敗の値を作る。引数を取らない（理由を受け取る口を作らない）。
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl fmt::Debug for UniformFailure {
    /// 固定の文字列だけを書く。derive は内部の形に依存するため使わない。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UniformFailure")
    }
}

#[cfg(test)]
mod tests {
    use super::UniformFailure;
    use core::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    fn hash_of(v: &UniformFailure) -> u64 {
        let mut h = DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }

    // UISF-4: 前提 = new() で 2 つ作る / 期待 = 等しく、Hash も等しい（区別できる値が無い）
    // 型の形の確認で、理由を運ぶ欄の検出の主体は uisf_4_carries_no_payload（size）の試験。
    #[test]
    fn uisf_4_all_values_are_equal() {
        let a = UniformFailure::new();
        let b = UniformFailure::new();
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    // UISF-4・THRT-25: Debug は固定の文字列だけ（形は SECR-11 に倣い、{:?} と {:#?} の両方）
    #[test]
    fn uisf_4_debug_is_fixed() {
        assert_eq!(format!("{:?}", UniformFailure::new()), "UniformFailure");
        assert_eq!(format!("{:#?}", UniformFailure::new()), "UniformFailure");
    }

    // UISF-4: 理由を隠し持つ余地が無い（ZST）
    #[test]
    fn uisf_4_carries_no_payload() {
        assert_eq!(core::mem::size_of::<UniformFailure>(), 0);
    }
}
