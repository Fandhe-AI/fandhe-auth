//! SECR-11: 任意の文字列から作った `SecretString` の `Debug` は常に固定文字列。
//!
//! 例数は proptest の既定のまま。10,000 例の基準は本番の TASK-116 で扱う（この段は最小形）。
//! K8 の基準（ACPT-37・ACPT-38）の形を試す。対象の P-n・F-n ではない（`SecretString` は対象に入っていない）。
//! ここは〔仮想〕の確認。
//!
//! SECR-23・SECR-26: 失敗時にも値を出さない。次の 3 点で守る。
//! - 戦略は試験専用の包み型 `Input` を出す。`Input` の `Debug` は固定文字列で、SECR-11 のマスクが壊れた退行の
//!   失敗でも、ランナーが表示する最小の反例に生成した値が出ない（`SecretString` の `Debug` に頼らない）。
//! - assert は固定のメッセージの `prop_assert!` だけで、値・出力を書式に含めない。
//! - `failure_persistence: None` で回帰の記録ファイルを切る（記録は生成した値をディスクに残すため）。

use fandhe_auth_secret::SecretString;
use proptest::prelude::*;

const FIXED: &str = "SecretString(<redacted>)";

/// 生成した試験の値（秘密値ではない）を、proptest の失敗の表示から隠すための試験専用の型。
///
/// `Debug` は手書きで固定文字列を返す。欄名は秘密値らしい名前にしない（SECR-21 の走査の対象の外）。
struct Input(String);

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Input(<generated>)")
    }
}

fn input_strategy() -> impl Strategy<Value = Input> {
    any::<String>().prop_map(Input)
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// SECR-11: `{:?}`・`{:#?}` が固定文字列になる。
    #[test]
    fn secr11_debug_is_fixed_for_any_string(input in input_strategy()) {
        let secret = SecretString::new(input.0.clone());
        let compact_ok = format!("{secret:?}") == FIXED;
        let pretty_ok = format!("{secret:#?}") == FIXED;
        prop_assert!(compact_ok, "compact Debug is not the fixed string");
        prop_assert!(pretty_ok, "pretty Debug is not the fixed string");
    }

    /// SECR-11: 固定文字列と値が重ならない限り、出力に値が含まれない。
    /// 値が空、または固定文字列の部分文字列になる入力は「含まれない」の対象外とする。
    #[test]
    fn secr11_debug_does_not_contain_value(input in input_strategy()) {
        let secret = SecretString::new(input.0.clone());
        let value = input.0.as_str();
        prop_assume!(!value.is_empty() && !FIXED.contains(value));
        let leaked = format!("{secret:?}").contains(value);
        prop_assert!(!leaked, "Debug output contains the value");
    }

    /// `expose()` が生成時の元の文字列を往復する。
    #[test]
    fn expose_roundtrips(input in input_strategy()) {
        let secret = SecretString::new(input.0.clone());
        let same = secret.expose() == input.0;
        prop_assert!(same, "expose() did not round-trip");
    }
}
