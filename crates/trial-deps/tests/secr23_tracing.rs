//! SECR-23・SECR-11: `tracing` で出した出力に秘密値が現れない。
//!
//! 捕獲用の writer を持つ fmt subscriber を `with_default` で局所的に当てる（グローバルは設定しない）。
//! 陽性対照で、捕獲が値を検出できること（検出力）も確かめる。ここは〔仮想〕の確認で、
//! 本番の試験は TASK-518（SECR-23・SECR-26 の走査）。
//!
//! probe は試験ごとに実行時に作り（コミットしない。`secret-values.md` の試験の値）、assert のメッセージには
//! 一致の有無と出力の長さだけを出す。捕獲した出力・値は失敗時にも出さない（SECR-23・SECR-26）。
//! 本番の probe（乱数 32 バイト・4 つの形の照合）は TASK-513・TASK-518 が引き取る。
//! 陽性対照（検出力の確認。AAUD-30・AAUD-31）の本番版は TASK-500、入れ物の試験（SECR-13）は TASK-513 が引き取る。

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io;
use std::sync::{Arc, Mutex};

use fandhe_auth_secret::SecretString;
use fandhe_auth_trial_deps::logging::{log_container_debug_fields, log_secret_debug_field};
use tracing_subscriber::fmt::MakeWriter;

/// 試験用の値を実行時に作る。実在の秘密ではない。呼ぶたびに別の値になる。
/// `RandomState` ごとに鍵が違うことを使い、新しい依存を足さない。
fn make_probe() -> String {
    let parts: Vec<String> = (0..3u64)
        .map(|i| format!("{:016x}", RandomState::new().hash_one(i)))
        .collect();
    format!("probe-{}", parts.join(""))
}

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
    let probe = make_probe();
    let s = SecretString::new(&probe);
    let out = capture(|| log_secret_debug_field(&s));
    assert!(
        out.contains("<redacted>"),
        "redacted marker present: false (output len={})",
        out.len()
    );
    let leaked = out.contains(&probe);
    assert!(
        !leaked,
        "probe found in output: {leaked} (output len={})",
        out.len()
    );
}

/// SECR-23・SECR-13: std の派生 `Debug` の入れ物（tuple・`Option`・slice）に入れても値が無い。
/// 自前の構造体の入れ物は TASK-513 の範囲。
#[test]
fn secr23_derived_debug_holder_has_no_value() {
    let probe = make_probe();
    let s = SecretString::new(&probe);
    let maybe = Some(SecretString::new(&probe));
    let list = vec![SecretString::new(&probe), SecretString::new(&probe)];
    let out = capture(|| log_container_debug_fields(&("label", &s), &maybe, &list));
    // 欄ごとにマスク済みの値が出ていることを確かめる（1 つの欄だけで通らないようにする）。
    let expected_fields = [
        ("pair", "pair=(\"label\", SecretString(<redacted>))"),
        ("maybe", "maybe=Some(SecretString(<redacted>))"),
        (
            "list",
            "list=[SecretString(<redacted>), SecretString(<redacted>)]",
        ),
    ];
    for (name, expected) in expected_fields {
        let present = out.contains(expected);
        assert!(
            present,
            "masked field `{name}` present: {present} (output len={})",
            out.len()
        );
    }
    let leaked = out.contains(&probe);
    assert!(
        !leaked,
        "probe found in output: {leaked} (output len={})",
        out.len()
    );
}

/// 陽性対照: `expose()` の値を出す経路では捕獲が値を検出できる（捕獲の検出力の確認）。
/// 試験の中だけで値を出す。lib には値を出す関数を置かない（SECR-16）。
#[test]
fn secr23_positive_control_detects_exposed_value() {
    // 注記: この expose() は試験専用の呼び出し。SECR-16 の呼び出し箇所の一覧（TASK-515）を作るときは、
    // 試験専用の例外として載せるか、それより前に本 crate を消す。
    let probe = make_probe();
    let s = SecretString::new(&probe);
    let out = capture(|| tracing::info!(leaked = s.expose(), "positive control emitted"));
    let detected = out.contains(&probe);
    assert!(
        detected,
        "capture detected probe: {detected} (output len={})",
        out.len()
    );
}
