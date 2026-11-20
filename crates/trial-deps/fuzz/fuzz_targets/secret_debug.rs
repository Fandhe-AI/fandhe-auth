#![forbid(unsafe_code)]
#![no_main]

//! SECR-11: 任意のバイト列を UTF-8 として読めたら `SecretString` にし、
//! `Debug` が固定であること、`expose()` が往復することを確かめる。
//! K8 の基準（ACPT-37・ACPT-38）の形を試す。対象の F-n ではない（`SecretString` は対象に入っていない）。

use fandhe_auth_secret::SecretString;
use libfuzzer_sys::fuzz_target;

const FIXED: &str = "SecretString(<redacted>)";

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let secret = SecretString::new(s);
    assert_eq!(format!("{secret:?}"), FIXED);
    assert_eq!(format!("{secret:#?}"), FIXED);
    assert_eq!(secret.expose(), s);
});
