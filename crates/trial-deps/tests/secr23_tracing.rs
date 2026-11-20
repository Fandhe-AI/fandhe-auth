//! SECR-23・SECR-11: `tracing` で出した出力に秘密値が現れない。
//!
//! 捕獲用の writer を持つ fmt layer と、欄を名前と値に分けて集める試験用 Layer を `with_default` で局所的に当てる（グローバルは設定しない）。
//! 陽性対照で、捕獲が値を検出できること（検出力）も確かめる。ここは〔仮想〕の確認で、
//! 本番の試験は TASK-518（SECR-23・SECR-26 の走査）。
//!
//! probe は試験ごとに実行時に作り（コミットしない。`secret-values.md` の試験の値）、assert のメッセージには
//! 一致の有無と出力の長さだけを出す。捕獲した出力・値は失敗時にも出さない（SECR-23・SECR-26）。
//! 本番の probe（乱数 32 バイト・4 つの形の照合）は TASK-513・TASK-518 が引き取る。
//! 陽性対照（検出力の確認。AAUD-30・AAUD-31）の本番版は TASK-500、入れ物の試験（SECR-13）は TASK-513 が引き取る。

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::BuildHasher;
use std::io;
use std::sync::{Arc, Mutex};

use fandhe_auth_secret::SecretString;
use fandhe_auth_trial_deps::logging::{log_container_debug_fields, log_secret_debug_field};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

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

/// 捕まえたイベント 1 件。欄を名前と値に分け、メッセージは別に持つ（SECR-23）。
/// 値は `format!("{:?}")` の形で持つ。失敗メッセージには出さない（SECR-26）。
#[derive(Clone, Default)]
struct CapturedEvent {
    message: String,
    fields: Vec<(String, String)>,
}

#[derive(Default)]
struct FieldVisitor(CapturedEvent);

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.0.message = rendered;
        } else {
            self.0.fields.push((field.name().to_owned(), rendered));
        }
    }
}

/// 試験の中だけで使う Layer。イベントのフィールドを名前と値に分けて集める。
#[derive(Clone, Default)]
struct EventSink(Arc<Mutex<Vec<CapturedEvent>>>);

impl<S: Subscriber> Layer<S> for EventSink {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.0.lock().unwrap().push(visitor.0);
    }
}

/// `f` を局所の subscriber の下で流し、整形済みの出力全体と、欄ごとに分けたイベントを返す。
fn capture(f: impl FnOnce()) -> (String, Vec<CapturedEvent>) {
    let cap = Capture::default();
    let sink = EventSink::default();
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(cap.clone())
                .with_ansi(false),
        )
        .with(sink.clone());
    tracing::subscriber::with_default(subscriber, f);
    let bytes = cap.0.lock().unwrap().clone();
    let events = sink.0.lock().unwrap().clone();
    // SECR-23・SECR-26: `FromUtf8Error` の `Debug` は捕獲したバイト列を含むため、誤りの中身を捨てて固定のメッセージで落とす。
    // UTF-8 でない出力は試験の失敗として扱う（`from_utf8_lossy` には替えない）。
    let out = String::from_utf8(bytes)
        .map_err(|_| ())
        .expect("captured output is not UTF-8");
    (out, events)
}

/// 欄の名前の並びと値の並びが期待と完全に一致するか。
fn fields_match(ev: &CapturedEvent, expected: &[(&str, &str)]) -> bool {
    ev.fields.len() == expected.len()
        && ev
            .fields
            .iter()
            .zip(expected)
            .all(|((n, v), (en, ev_))| n == en && v == ev_)
}

/// 整形済みの出力・メッセージ・全欄の値のどこにも probe が無いか（`true` なら漏れている）。
fn leaked(probe: &str, out: &str, events: &[CapturedEvent]) -> bool {
    out.contains(probe)
        || events.iter().any(|e| {
            e.message.contains(probe)
                || e.fields
                    .iter()
                    .any(|(n, v)| n.contains(probe) || v.contains(probe))
        })
}

/// SECR-23・SECR-11: `?` 形式の欄に `SecretString` を出しても値が無い。
#[test]
fn secr23_secret_debug_field_has_no_value() {
    let probe = make_probe();
    let s = SecretString::new(&probe);
    let (out, events) = capture(|| log_secret_debug_field(&s));
    // イベントは 1 件で、メッセージは固定文、欄は `token` の 1 つだけ、値はマスクの文字列と完全一致する。
    // 末尾の部分一致ではなく、欄を名前と値に分けて照合する（マスクがメッセージへ移る変異を検知する）。
    let one_event = events.len() == 1;
    let ev = events.first().cloned().unwrap_or_default();
    let message_ok = ev.message == "secret field emitted";
    let fields_ok = fields_match(&ev, &[("token", "SecretString(<redacted>)")]);
    assert!(
        one_event && message_ok && fields_ok,
        "events==1: {one_event}, message exact: {message_ok}, field `token` exact: {fields_ok} (events={}, fields={})",
        events.len(),
        ev.fields.len()
    );
    let is_leaked = leaked(&probe, &out, &events);
    assert!(
        !is_leaked,
        "probe found in output or fields: {is_leaked} (output len={})",
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
    let (out, events) = capture(|| log_container_debug_fields(&s, &maybe, &list));
    let one_event = events.len() == 1;
    let ev = events.first().cloned().unwrap_or_default();
    let message_ok = ev.message == "container fields emitted";
    let fields_ok = fields_match(
        &ev,
        &[
            ("pair", "(\"label\", SecretString(<redacted>))"),
            ("maybe", "Some(SecretString(<redacted>))"),
            (
                "list",
                "[SecretString(<redacted>), SecretString(<redacted>)]",
            ),
        ],
    );
    assert!(
        one_event && message_ok && fields_ok,
        "events==1: {one_event}, message exact: {message_ok}, fields `pair`/`maybe`/`list` exact: {fields_ok} (events={}, fields={})",
        events.len(),
        ev.fields.len()
    );
    let is_leaked = leaked(&probe, &out, &events);
    assert!(
        !is_leaked,
        "probe found in output or fields: {is_leaked} (output len={})",
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
    let (out, events) = capture(|| tracing::info!(leaked = s.expose(), "positive control emitted"));
    // 整形済みの出力と、欄ごとの捕獲の両方で検出できること（Layer 側の検出力も確かめる）。
    let detected = out.contains(&probe) && leaked(&probe, "", &events);
    assert!(
        detected,
        "capture detected probe: {detected} (output len={})",
        out.len()
    );
}
