//! SECR-11: 任意の文字列から作った `SecretString` の `Debug` は常に固定文字列。
//!
//! 例数は proptest の既定のまま。10,000 例の基準は本番の TASK-116 で扱う（この段は最小形）。
//! K8 の基準（ACPT-37・ACPT-38）の形を試す。対象の P-n・F-n ではない（`SecretString` は対象に入っていない）。
//! ここは〔仮想〕の確認。
//!
//! SECR-23・SECR-26: 失敗時にも値を出さない。次の 3 点で守る。
//! - 戦略が `SecretString` を直接作る。ランナーが入力を表示しても固定の `Debug` しか出ない。
//! - assert は固定のメッセージの `prop_assert!` だけで、値・出力を書式に含めない。
//! - `failure_persistence: None` で回帰の記録ファイルを切る（記録は生成した値をディスクに残すため）。

use fandhe_auth_secret::SecretString;
use proptest::prelude::*;

const FIXED: &str = "SecretString(<redacted>)";

fn secret_strategy() -> impl Strategy<Value = SecretString> {
    any::<String>().prop_map(SecretString::new)
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// SECR-11: `{:?}`・`{:#?}` が固定文字列になる。
    #[test]
    fn secr11_debug_is_fixed_for_any_string(secret in secret_strategy()) {
        let compact_ok = format!("{secret:?}") == FIXED;
        let pretty_ok = format!("{secret:#?}") == FIXED;
        prop_assert!(compact_ok, "compact Debug is not the fixed string");
        prop_assert!(pretty_ok, "pretty Debug is not the fixed string");
    }

    /// SECR-11: 固定文字列と値が重ならない限り、出力に値が含まれない。
    /// 値が空、または固定文字列の部分文字列になる入力は「含まれない」の対象外とする。
    #[test]
    fn secr11_debug_does_not_contain_value(secret in secret_strategy()) {
        let value = secret.expose();
        prop_assume!(!value.is_empty() && !FIXED.contains(value));
        let leaked = format!("{secret:?}").contains(value);
        prop_assert!(!leaked, "Debug output contains the value");
    }

    /// `expose()` が入力を往復する（作り直した `SecretString` の `expose()` が元と一致する）。
    #[test]
    fn expose_roundtrips(secret in secret_strategy()) {
        let rebuilt = SecretString::new(secret.expose());
        let same = rebuilt.expose() == secret.expose();
        prop_assert!(same, "expose() did not round-trip");
    }
}
