//! 画面が守る安全条件（区分「画面」）のうち、HTML の出力の型 `SafeHtml` を持つ。
//!
//! 役割: HTML として外へ出してよい値を型で区別する。エスケープ前の実行時の文字列から `SafeHtml` を作る
//! 公開の道を置かず、実行時の値が入る道は [`SafeHtml::escape_text`]・[`SafeHtml::push_text`]（どちらも
//! エスケープする）だけにする。コンパイル時の定数が入る道は [`html_literal!`] を想定する。
//! 根拠は UISF-2・UISF-18 の型の部分（申告値・利用者の入力のエスケープ）。UISF-19 はスクリプトが
//! `textContent` で描く形で、この型の対象外。LGUI-14 は将来の利用先（誤りの画面）で、ここでは扱わない。
//!
//! 型で閉じているのは実行時の値だけである。コンパイル時の定数は、`html_literal!` の外の手書きの
//! `impl Markup`・`SafeHtml::__from_markup`（どちらも `doc(hidden)` の公開項目）の直接の呼び出しからも入る。
//! そのため「作る道は 2 つだけ」とは言えず、`html_literal!` 以外の入口が使われていないことは後続の走査で見る
//! （TODO(#5796): TASK-771 (2/8) の走査で `Markup` の実装と `__from_markup` の呼び出しを数える）。
//!
//! 実行時の文字列を包む道は置かない。`SafeHtml::__from_markup` は型引数の関連定数（`Markup::TEXT`）からだけ
//! 値を取る。関連定数はコンパイル時に決まる値しか持てず、実行時の文字列（`Box::leak` 等）は渡せない
//! （`&'static str` を直接受ける関数は置かない）。
//!
//! 持たないもの: 描画・テンプレート・HTTP 応答・CSP の付与（LIB-1・LIB-3・A-2026-10-30-11。実行は Hub の
//! `ASCR`）。I/O・時刻・乱数も持たない（DPOL-1）。
//!
//! 呼ばれ方: 組込み先（Hub）の画面の描画は `SafeHtml` を受け、`String` から HTML を組み立てない。
//! 外へ渡す入口は [`SafeHtml::as_str`]・[`SafeHtml::into_string`] だけにする。
//!
//! 名前 `SafeHtml` は spec 上の仮称で、置き場・module 名 `ui_safety`・マクロ名 `html_literal!` は提案（人の確認待ち）。
//! UISF-2 は検討中のまま。この型は「申告値のエスケープ」の型の部分に沿うだけで、SSR のソース検査・
//! ブラウザでの確認は別の作業で残る。
//!
//! # エスケープが有効な文脈
//!
//! `SafeHtml` は HTML のテキストと、引用符で囲んだ属性値の文脈だけのエスケープを保証する。
//! `<script>`/`<style>` の中・引用符なしの属性値・URL（`href`・`src` の scheme）・イベントハンドラ属性・
//! コメントの文脈の安全は型では保証しない。引用符で囲んでも、`style` 属性（中身は CSS の文脈）や
//! `srcdoc` 等の値が HTML・CSS・JS として解釈される属性は、エスケープでは守れない。型は出力の文脈を区別しないため、組込み先は型だけを根拠に
//! それらの文脈へ利用者の値を出さない。
//!
//! 開発者がリテラルで書くインラインスクリプトは、型では止められない。止めるのは走査と CSP である
//! （走査は TODO(#5799): TASK-771 (5/8)。CSP の付与の実行は Hub の `ASCR`。要求値は CSRF-28）。

use std::borrow::Cow;
use std::fmt;

/// HTML として出してよい値。フィールドは非公開で、`From<String>`・`FromStr`・`Default`・`Deref`・
/// `Display` を実装しない（エスケープ前の文字列を直接出す道を型の上で置かない。UISF-2）。
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let _ = SafeHtml::from(String::new());
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let _ = "x".parse::<SafeHtml>();
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let _ = SafeHtml { inner: std::borrow::Cow::Borrowed("x") };
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::html_literal;
/// let s = String::from("<p>");
/// let _ = html_literal!(s);
/// ```
///
/// 実装しない trait の陰性対照。`Display`・`Deref`・`Default`・`From<&'static str>`・`Into<String>`。
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let x = SafeHtml::escape_text("a");
/// let _ = format!("{}", x);
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let x = SafeHtml::escape_text("a");
/// let _ = x.to_string();
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let x = SafeHtml::escape_text("a");
/// let _: &str = &*x;
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let _ = SafeHtml::default();
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let _: SafeHtml = "<p>".into();
/// ```
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let x = SafeHtml::escape_text("a");
/// let _: String = x.into();
/// ```
///
/// 陽性対照: 同じ入口（`escape_text`・`as_str`・`into_string`）は使える。
///
/// ```
/// use fandhe_auth_core::ui_safety::SafeHtml;
/// let x = SafeHtml::escape_text("<a>");
/// assert_eq!(x.as_str(), "&lt;a&gt;");
/// assert_eq!(x.into_string(), "&lt;a&gt;");
/// ```
///
/// 関連定数に実行時の値は入れられない。
///
/// ```compile_fail
/// use fandhe_auth_core::ui_safety::{Markup, SafeHtml};
/// fn f(input: String) -> SafeHtml {
///     struct M;
///     impl Markup for M {
///         const TEXT: &'static str = input.leak();
///     }
///     SafeHtml::__from_markup::<M>()
/// }
/// ```
#[must_use]
#[derive(Clone, PartialEq, Eq)]
pub struct SafeHtml {
    inner: Cow<'static, str>,
}

impl SafeHtml {
    /// 利用者の入力・攻撃者が選べる値を HTML エスケープして包む（UISF-2・UISF-18）。
    ///
    /// `&` `<` `>` `"` `'` を実体参照にする。1 文字ずつ書き出すため二重置換は起きない。
    ///
    /// ```
    /// use fandhe_auth_core::ui_safety::SafeHtml;
    /// assert_eq!(SafeHtml::escape_text("<b>x</b>").as_str(), "&lt;b&gt;x&lt;/b&gt;");
    /// ```
    pub fn escape_text(text: &str) -> SafeHtml {
        let mut out = SafeHtml {
            inner: Cow::Owned(String::with_capacity(text.len())),
        };
        out.push_text(text);
        out
    }

    /// `SafeHtml` 同士の連結。
    pub fn push(&mut self, other: SafeHtml) {
        self.inner.to_mut().push_str(&other.inner);
    }

    /// エスケープして足す。[`SafeHtml::escape_text`] と同じ規則。
    pub fn push_text(&mut self, text: &str) {
        let buf = self.inner.to_mut();
        for c in text.chars() {
            match c {
                '&' => buf.push_str("&amp;"),
                '<' => buf.push_str("&lt;"),
                '>' => buf.push_str("&gt;"),
                '"' => buf.push_str("&quot;"),
                '\'' => buf.push_str("&#x27;"),
                other => buf.push(other),
            }
        }
    }

    /// HTML を外（組込み先の応答の本文）へ渡す入口。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// HTML を外へ渡す入口（所有権を渡す形）。
    #[must_use]
    pub fn into_string(self) -> String {
        self.inner.into_owned()
    }

    /// [`html_literal!`] が呼ぶ。マークアップは型引数 `M` の関連定数から取るため、
    /// コンパイル時に決まる値だけが入る（実行時の文字列は関連定数に入れられない）。
    /// `doc(hidden)` だが公開で、直接呼ぶ道は型では塞げない（走査で見る。TODO(#5796)）。
    #[doc(hidden)]
    pub const fn __from_markup<M: Markup>() -> SafeHtml {
        SafeHtml {
            inner: Cow::Borrowed(M::TEXT),
        }
    }
}

/// [`html_literal!`] が生成する型が実装する。関連定数でコンパイル時の文字列だけを受ける。
#[doc(hidden)]
pub trait Markup {
    /// コンパイル時に決まるマークアップ。
    const TEXT: &'static str;
}

impl fmt::Debug for SafeHtml {
    /// 中身に利用者の入力が入るため、固定文字列にする。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SafeHtml(..)")
    }
}

/// 開発者が書いた固定のマークアップから `SafeHtml` を作る。引数は文字列リテラルだけで、
/// 変数・`format!`・実行時の値はコンパイルできない（UISF-2）。利用者の値は入れない。
///
/// ```
/// use fandhe_auth_core::html_literal;
/// assert_eq!(html_literal!("<p>").as_str(), "<p>");
/// ```
#[macro_export]
macro_rules! html_literal {
    ($markup:literal) => {{
        struct M;
        impl $crate::ui_safety::Markup for M {
            const TEXT: &'static str = $markup;
        }
        $crate::ui_safety::SafeHtml::__from_markup::<M>()
    }};
}

#[cfg(test)]
mod tests {
    use super::SafeHtml;

    fn unescape(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&amp;", "&")
    }

    // UISF-18: 5 文字がそれぞれ置き換わる
    #[test]
    fn escapes_five_chars() {
        assert_eq!(
            SafeHtml::escape_text("&<>\"'").as_str(),
            "&amp;&lt;&gt;&quot;&#x27;"
        );
    }

    // UISF-2: 申告値の形が生のまま残らず、逆変換で元に戻る
    #[test]
    fn declared_value_has_no_raw_markup() {
        let raw = "<img src=x onerror=alert(1)><script>a</script>\"'&";
        let out = SafeHtml::escape_text(raw);
        let s = out.as_str();
        assert!(!s.contains(['<', '>', '"', '\'']));
        assert_eq!(unescape(s), raw);
    }

    #[test]
    fn no_double_escape_of_ampersand() {
        let out = SafeHtml::escape_text("&amp;");
        assert_eq!(out.as_str(), "&amp;amp;");
        assert_eq!(unescape(out.as_str()), "&amp;");
    }

    #[test]
    fn push_and_push_text() {
        let mut a = html_literal!("<p>");
        a.push_text("<x>");
        a.push(html_literal!("</p>"));
        assert_eq!(a.as_str(), "<p>&lt;x&gt;</p>");
        assert_eq!(a.into_string(), "<p>&lt;x&gt;</p>");
        let mut b = SafeHtml::escape_text("");
        b.push_text("a&b");
        assert_eq!(b, SafeHtml::escape_text("a&b"));
        // 連結の結果でも Eq が成り立つ
        let mut c = SafeHtml::escape_text("a");
        c.push(SafeHtml::escape_text("<"));
        assert_eq!(c, SafeHtml::escape_text("a<"));
    }

    #[test]
    fn empty_and_non_ascii() {
        assert_eq!(SafeHtml::escape_text("").as_str(), "");
        assert_eq!(
            SafeHtml::escape_text("日本語😀<é>").as_str(),
            "日本語😀&lt;é&gt;"
        );
    }

    #[test]
    fn debug_hides_content() {
        let d = format!("{:?}", SafeHtml::escape_text("secret-handle"));
        assert_eq!(d, "SafeHtml(..)");
    }
}
