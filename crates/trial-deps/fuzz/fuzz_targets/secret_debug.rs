#![forbid(unsafe_code)]
#![no_main]

//! SECR-11: 任意のバイト列を UTF-8 として読めたら `SecretString` にし、
//! `Debug` が固定であること、`expose()` が往復することを確かめる。
//!
//! SECR-23・SECR-26: `assert_eq!` は失敗時に値を含む整形結果を panic のメッセージへ出すため、
//! 固定のメッセージの `assert!` だけを使う。値の出口は次の 2 つ。
//! - libFuzzer は crash の入力を `fuzz/artifacts/` に保存する。このディレクトリは `.gitignore` 済みで、コミットしない。
//! - crash のとき、libFuzzer は入力を 16 進・ASCII・Base64 で stderr に出す。
//!   この出力を PR・Issue・記録に貼らない。
//! K8 の基準（ACPT-37・ACPT-38）の形を試す。対象の F-n ではない（`SecretString` は対象に入っていない）。

use fandhe_auth_secret::SecretString;
use libfuzzer_sys::fuzz_target;

const FIXED: &str = "SecretString(<redacted>)";

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let secret = SecretString::new(s);
    assert!(
        format!("{secret:?}") == FIXED,
        "compact Debug is not the fixed string"
    );
    assert!(
        format!("{secret:#?}") == FIXED,
        "pretty Debug is not the fixed string"
    );
    // 注記: この expose() は試験専用の取り出しで、比較だけに使い外へ出さない。
    // SECR-16 の呼び出し箇所の一覧（TASK-515）の対象。
    assert!(secret.expose() == s, "expose() did not round-trip");
});
