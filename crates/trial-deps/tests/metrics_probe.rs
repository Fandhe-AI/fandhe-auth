//! `metrics` の出す口が key・ラベル・増分を recorder へ渡す（SECR-26・SECR-27 の試し）。
//!
//! 追加の crate を使わず、自前の最小の `Recorder` を `with_local_recorder` で当てる。
//! ラベルに秘密値を載せない型の制約は範囲外。〔仮想〕の確認。

use std::sync::atomic::{AtomicU64, Ordering};
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

struct Handle(Arc<AtomicU64>);

impl CounterFn for Handle {
    fn increment(&self, value: u64) {
        self.0.fetch_add(value, Ordering::SeqCst);
    }
    fn absolute(&self, value: u64) {
        self.0.fetch_max(value, Ordering::SeqCst);
    }
}

struct MinimalRecorder {
    registered: Mutex<Registered>,
    total: Arc<AtomicU64>,
}

impl Recorder for MinimalRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        let labels = key
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        self.registered
            .lock()
            .unwrap()
            .keys
            .push((key.name().to_owned(), labels));
        Counter::from_arc(Arc::new(Handle(self.total.clone())))
    }
    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }
    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

/// SECR-26・SECR-27: counter の key・ラベル・増分を recorder が受ける。
#[test]
fn secr26_probe_counter_key_label_and_increment_received() {
    let rec = MinimalRecorder {
        registered: Mutex::new(Registered::default()),
        total: Arc::new(AtomicU64::new(0)),
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
    assert_eq!(rec.total.load(Ordering::SeqCst), 1);
}
