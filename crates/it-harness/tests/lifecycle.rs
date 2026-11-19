//! 起動順・停止順・途中失敗時の後始末の試験（TASK-10 (1/6)）。DB・Docker・ネットワークなし。

use std::sync::{Arc, Mutex};

use fandhe_auth_it_harness::{
    Component, ComponentKind, Harness, HarnessConfig, HarnessError, HarnessErrorKind,
};

type Log = Arc<Mutex<Vec<String>>>;

struct Fake {
    kind: ComponentKind,
    log: Log,
    fail_start: bool,
}

impl Component for Fake {
    fn kind(&self) -> ComponentKind {
        self.kind
    }
    fn start(&mut self) -> Result<(), HarnessError> {
        if self.fail_start {
            return Err(HarnessError::new(HarnessErrorKind::StartFailed, self.kind));
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("start {:?}", self.kind));
        Ok(())
    }
    fn stop(&mut self) -> Result<(), HarnessError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("stop {:?}", self.kind));
        Ok(())
    }
}

fn fake(kind: ComponentKind, log: &Log, fail_start: bool) -> Box<dyn Component> {
    Box::new(Fake {
        kind,
        log: log.clone(),
        fail_start,
    })
}

fn entries(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn all(log: &Log, fail: Option<ComponentKind>) -> HarnessConfig {
    // 渡す順は起動順と逆にして、並べ替えを確かめる。
    ComponentKind::START_ORDER
        .iter()
        .rev()
        .fold(HarnessConfig::new(), |c, k| {
            c.with(fake(*k, log, Some(*k) == fail))
        })
}

#[test]
fn starts_in_order_and_stops_in_reverse() {
    let log: Log = Arc::default();
    let running = Harness::start(all(&log, None)).unwrap();
    assert_eq!(running.kinds(), ComponentKind::START_ORDER);
    running.stop().unwrap();
    assert_eq!(
        entries(&log),
        [
            "start Database",
            "start Host",
            "start HubStub",
            "start ConsoleStub",
            "stop ConsoleStub",
            "stop HubStub",
            "stop Host",
            "stop Database",
        ]
    );
}

#[test]
fn start_failure_stops_started_ones_in_reverse() {
    let log: Log = Arc::default();
    let err = Harness::start(all(&log, Some(ComponentKind::HubStub)))
        .err()
        .unwrap();
    assert_eq!(err.kind(), HarnessErrorKind::StartFailed);
    assert_eq!(err.component(), ComponentKind::HubStub);
    assert_eq!(
        entries(&log),
        ["start Database", "start Host", "stop Host", "stop Database"]
    );
}

#[test]
fn duplicate_kind_is_rejected_before_start() {
    let log: Log = Arc::default();
    let cfg = HarnessConfig::new()
        .with(fake(ComponentKind::Database, &log, false))
        .with(fake(ComponentKind::Database, &log, false));
    let err = Harness::start(cfg).err().unwrap();
    assert_eq!(err.kind(), HarnessErrorKind::Duplicate);
    assert!(entries(&log).is_empty());
}

#[test]
fn drop_stops_and_explicit_stop_does_not_double_stop() {
    let log: Log = Arc::default();
    let cfg = HarnessConfig::new().with(fake(ComponentKind::Database, &log, false));
    drop(Harness::start(cfg).unwrap());
    assert_eq!(entries(&log), ["start Database", "stop Database"]);

    let log: Log = Arc::default();
    let cfg = HarnessConfig::new().with(fake(ComponentKind::Host, &log, false));
    Harness::start(cfg).unwrap().stop().unwrap();
    assert_eq!(entries(&log), ["start Host", "stop Host"]);
}

#[test]
fn error_shows_only_kinds() {
    let e = HarnessError::new(HarnessErrorKind::StartFailed, ComponentKind::Database);
    assert_eq!(e.to_string(), "harness error: StartFailed (Database)");
    assert_eq!(
        format!("{e:?}"),
        "HarnessError { kind: StartFailed, component: Database }"
    );
}
