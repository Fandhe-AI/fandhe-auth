//! 画面が守る安全条件（区分「画面」）のうち、HTML の出力の型 `SafeHtml` を持つ。
//!
//! 役割: HTML として外へ出してよい値を型で区別する。エスケープ前の文字列から `SafeHtml` を作る
//! 公開の道を置かず、作る道は [`SafeHtml::escape_text`] と [`html_literal!`] の 2 つだけにする
//! （UISF-2・UISF-18・UISF-19 の型の部分。LGUI-14）。
//!
//! 限界: `html_literal!` が内部で呼ぶ `__from_literal` は `pub` で、`&'static str` を渡せるため
//! （`Box::leak` 等で）実行時の文字列からも作れてしまう。型の上では塞げず、直接呼ぶ箇所は
//! ソース走査で担保する前提とする。走査は本 crate の型では未実装。
//!
//! 持たないもの: 描画・テンプレート・HTTP 応答・CSP の付与（LIB-1・LIB-3・A-2026-10-30-11。実行は Hub の
//! `ASCR`）。I/O・時刻・乱数も持たない（DPOL-1）。
//!
//! 呼ばれ方: 組込み先（Hub）の画面の描画は `SafeHtml` を受け、`String` から HTML を組み立てない。
//! 外へ渡す入口は [`SafeHtml::as_str`]・[`SafeHtml::into_string`] だけにする。
//!
//! 名前 `SafeHtml` は spec 上の仮称で、module 名 `ui_safety`・マクロ名 `html_literal!` は提案（人の確認待ち）。
//! UISF-2 は検討中のまま。この型は「申告値のエスケープ」の型の部分に沿うだけで、SSR のソース検査・
//! ブラウザでの確認は別の作業で残る。
//!
//! # エスケープが有効な文脈
//!
//! 有効なのは要素のテキストと、引用符で囲んだ属性値だけ。引用符なしの属性値・URL（`href`・`src` の
//! scheme）・`<script>`/`<style>` の中・イベントハンドラ属性・コメントに利用者の値を入れることは、
//! この型では守れない。

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

    /// [`html_literal!`] だけが呼ぶ。直接呼ばない。`&'static str` を受けるため、型の上では実行時の
    /// 文字列（`Box::leak` 等）も渡せる。守りは `#[doc(hidden)]` ではなくソース走査に依る。
    #[doc(hidden)]
    pub const fn __from_literal(markup: &'static str) -> SafeHtml {
        SafeHtml {
            inner: Cow::Borrowed(markup),
        }
    }
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
        const MARKUP: &str = $markup;
        $crate::ui_safety::SafeHtml::__from_literal(MARKUP)
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
