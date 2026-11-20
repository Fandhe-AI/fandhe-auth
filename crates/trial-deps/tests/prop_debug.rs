//! SECR-11: 任意の `String` から作った `SecretString` の `Debug` は常に固定文字列。
//!
//! 例数は proptest の既定のまま。10,000 例の基準は本番の TASK-116 で扱う（この段は最小形）。
//! K8 の基準（ACPT-37・ACPT-38）の形を試す。対象の P-n・F-n ではない（`SecretString` は対象に入っていない）。
//! ここは〔仮想〕の確認。

use fandhe_auth_secret::SecretString;
use proptest::prelude::*;

const FIXED: &str = "SecretString(<redacted>)";

proptest! {
    /// SECR-11: `{:?}`・`{:#?}` が固定文字列になる。
    #[test]
    fn secr11_debug_is_fixed_for_any_string(s in any::<String>()) {
        let secret = SecretString::new(s.clone());
        let compact = format!("{secret:?}");
        let pretty = format!("{secret:#?}");
        prop_assert_eq!(&compact, FIXED);
        prop_assert_eq!(&pretty, FIXED);
    }

    /// SECR-11: 固定文字列と値が重ならない限り、出力に値が含まれない。
    /// 値が空、または固定文字列の部分文字列になる入力は「含まれない」の対象外とする。
    #[test]
    fn secr11_debug_does_not_contain_value(s in any::<String>()) {
        prop_assume!(!s.is_empty() && !FIXED.contains(s.as_str()));
        let out = format!("{:?}", SecretString::new(s.clone()));
        prop_assert!(!out.contains(s.as_str()));
    }

    /// `expose()` が入力を往復する。
    #[test]
    fn expose_roundtrips(s in any::<String>()) {
        let secret = SecretString::new(s.clone());
        prop_assert_eq!(secret.expose(), s.as_str());
    }
}
