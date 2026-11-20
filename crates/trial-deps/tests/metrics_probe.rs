//! `metrics` の出す口が key・ラベル・増分を recorder へ渡す（SECR-26・SECR-27 の試し）。
//!
//! 追加の crate を使わず、自前の最小の `Recorder` を `with_local_recorder` で当てる。
//! ラベルに秘密値を載せない型の制約は範囲外。〔仮想〕の確認。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fandhe_auth_trial_deps::metrics_probe::emit_probe_counter;
use metrics::{
    Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};

#[derive(Default)]
struct Registered {
    /// 登録時の (name, labels)。
    keys: Vec<(String, Vec<(String, String)>)>,
}

/// `increment` と `absolute` を区別して記録する。
#[derive(Default)]
struct Calls {
    /// `increment` に渡された値を呼ばれた順に持つ。
    increments: Mutex<Vec<u64>>,
    /// `absolute` に渡された値を呼ばれた順に持つ。
    absolutes: Mutex<Vec<u64>>,
}

struct Handle(Arc<Calls>);

impl CounterFn for Handle {
    fn increment(&self, value: u64) {
        self.0.increments.lock().unwrap().push(value);
    }
    fn absolute(&self, value: u64) {
        self.0.absolutes.lock().unwrap().push(value);
    }
}

struct MinimalRecorder {
    registered: Mutex<Registered>,
    calls: Arc<Calls>,
    /// 想定外の呼び出しの件数（`describe_*`・gauge・histogram の登録）。受け取った文字列は持たない（SECR-26）。
    unexpected: AtomicUsize,
}

impl MinimalRecorder {
    fn note_unexpected(&self) {
        self.unexpected.fetch_add(1, Ordering::SeqCst);
    }
}

impl Recorder for MinimalRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {
        self.note_unexpected();
    }
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {
        self.note_unexpected();
    }
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {
        self.note_unexpected();
    }

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        // 期待する key 以外の counter の登録は想定外として数える。
        if key.name() != "trial_probe_total" {
            self.note_unexpected();
        }
        let labels = key
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        self.registered
            .lock()
            .unwrap()
            .keys
            .push((key.name().to_owned(), labels));
        Counter::from_arc(Arc::new(Handle(self.calls.clone())))
    }
    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        self.note_unexpected();
        Gauge::noop()
    }
    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        self.note_unexpected();
        Histogram::noop()
    }
}

/// SECR-26・SECR-27: counter の key・ラベル・増分を recorder が受ける。
#[test]
fn secr26_probe_counter_key_label_and_increment_received() {
    let rec = MinimalRecorder {
        registered: Mutex::new(Registered::default()),
        calls: Arc::new(Calls::default()),
        unexpected: AtomicUsize::new(0),
    };
    metrics::with_local_recorder(&rec, emit_probe_counter);

    let reg = rec.registered.lock().unwrap();
    assert_eq!(
        reg.keys,
        vec![(
            "trial_probe_total".to_owned(),
            vec![("kind".to_owned(), "probe".to_owned())]
        )]
    );
    assert_eq!(*rec.calls.increments.lock().unwrap(), vec![1]);
    assert!(rec.calls.absolutes.lock().unwrap().is_empty());
    // 想定外の計測・説明文が 0 件であること。件数だけを出し、key・label・説明文は出さない。
    let unexpected = rec.unexpected.load(Ordering::SeqCst);
    assert_eq!(unexpected, 0, "unexpected metrics calls: {unexpected}");
}
