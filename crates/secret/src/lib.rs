//! 秘密値の型を置く crate。秘密値の型は本 crate に 1 つだけ置く（SECR-31・SECR-15）。
//!
//! - `{:?}`・`{:#?}` は固定文字列を出し、値・長さ・ハッシュ・一部を出さない（SECR-11）。
//!   `Display` は実装しない（SECR-12）。
//! - 値の取り出しは [`SecretString::expose`] だけに限る（SECR-16）。`Deref`・`AsRef`・`From<_> for String` は実装しない。
//! - HTTP・DB・非同期ランタイムに依存しない小さな crate とし、他の crate から広く使われる（DPOL-41）。
//!
//! 現状は文字列の秘密値（トークン・assertion 等）だけ。バイト列・`zeroize`（SECR-22）は後続（TASK-511）。

#![forbid(unsafe_code)]

/// 文字列の秘密値。`Debug` は固定文字列で、値・長さ・ハッシュを出さない（SECR-11）。
/// `Display`・`Serialize` は実装しない（SECR-12・SECR-17）。`PartialEq` も持たない。
#[derive(Clone)]
pub struct SecretString {
    inner: String,
}

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            inner: value.into(),
        }
    }

    /// 値を明示的に取り出す唯一の経路（SECR-16）。ハッシュ化・署名・外への送信の直前だけで呼び、
    /// 戻り値をログ・誤り・記録に渡さない。
    pub fn expose(&self) -> &str {
        &self.inner
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::SecretString;

    #[derive(Debug)]
    struct Holder {
        _s: SecretString,
    }

    // SECR-11・SECR-13: Debug（通常・整形）に値・長さが出ない。派生 Debug の入れ物でも同じ。
    #[test]
    fn debug_is_fixed() {
        let s = SecretString::new("value-0123456789");
        assert_eq!(format!("{s:?}"), "SecretString(<redacted>)");
        assert_eq!(format!("{s:#?}"), "SecretString(<redacted>)");
        let h = Holder { _s: s.clone() };
        assert!(!format!("{h:?}").contains("0123"));
        assert_eq!(s.expose(), "value-0123456789");
    }
}
