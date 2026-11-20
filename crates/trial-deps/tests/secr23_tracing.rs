//! SECR-23・SECR-11: `tracing` で出した出力に秘密値が現れない。
//!
//! 捕獲用の writer を持つ fmt subscriber を `with_default` で局所的に当てる（グローバルは設定しない）。
//! 陽性対照で、捕獲が値を検出できること（検出力）も確かめる。ここは〔仮想〕の確認で、
//! 本番の試験は TASK-518（SECR-23・SECR-26 の走査）。
//!
//! 注記: `PROBE` は固定のリテラルで、assert のメッセージは捕獲した出力をそのまま出す。
//! 本番版（TASK-513 の probe・TASK-518）では、実行時に作る値を使い、一致の有無だけを出す形にする。
//! 陽性対照（検出力の確認。AAUD-30・AAUD-31）の本番版は TASK-500、入れ物の試験（SECR-13）は TASK-513 が引き取る。

use std::io;
use std::sync::{Arc, Mutex};

use fandhe_auth_secret::SecretString;
use fandhe_auth_trial_deps::logging::{
    TokenHolder, log_holder_debug_field, log_secret_debug_field,
};
use tracing_subscriber::fmt::MakeWriter;

/// 試験用の値。実在の秘密ではない。
const PROBE: &str = "probe-value-0123456789";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

/// `f` を局所の subscriber の下で流し、捕獲した出力を返す。
fn capture(f: impl FnOnce()) -> String {
    let cap = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(cap.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    let bytes = cap.0.lock().unwrap().clone();
    String::from_utf8(bytes).expect("captured output is UTF-8")
}

/// SECR-23・SECR-11: `?` 形式の欄に `SecretString` を出しても値が無い。
#[test]
fn secr23_secret_debug_field_has_no_value() {
    let s = SecretString::new(PROBE);
    let out = capture(|| log_secret_debug_field(&s));
    assert!(out.contains("<redacted>"), "output: {out}");
    assert!(!out.contains(PROBE), "output: {out}");
}

/// SECR-23・SECR-13: 派生 `Debug` の入れ物に入れても値が無い。
#[test]
fn secr23_derived_debug_holder_has_no_value() {
    let holder = TokenHolder {
        label: "holder-label".to_owned(),
        token: SecretString::new(PROBE),
    };
    let out = capture(|| log_holder_debug_field(&holder));
    assert!(out.contains("<redacted>"), "output: {out}");
    assert!(!out.contains(PROBE), "output: {out}");
}

/// 陽性対照: `expose()` の値を出す経路では捕獲が値を検出できる（捕獲の検出力の確認）。
/// 試験の中だけで値を出す。lib には値を出す関数を置かない（SECR-16）。
#[test]
fn secr23_positive_control_detects_exposed_value() {
    // 注記: この expose() は試験専用の呼び出し。SECR-16 の呼び出し箇所の一覧（TASK-515）を作るときは、
    // 試験専用の例外として載せるか、それより前に本 crate を消す。
    let s = SecretString::new(PROBE);
    let out = capture(|| tracing::info!(leaked = s.expose(), "positive control emitted"));
    assert!(out.contains(PROBE), "capture failed to detect: {out}");
}
