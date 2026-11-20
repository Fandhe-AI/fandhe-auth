//! 枠の起動・停止の試験。前提・操作・期待の形。
//!
//! Fake は枠の順序と後始末を確かめるためのもので、E2E の合格に数えない（ACPT-7）。

use std::sync::{Arc, Mutex};

use fandhe_auth_e2e_harness::{
    Browser, E2eComponent, E2eComponentKind, E2eConfig, E2eError, E2eErrorKind, E2eHarness,
    PasskeyOperation,
};

type Log = Arc<Mutex<Vec<String>>>;

struct Fake {
    kind: E2eComponentKind,
    log: Log,
    fail_start: bool,
}

impl Fake {
    fn boxed(kind: E2eComponentKind, log: &Log, fail_start: bool) -> Box<dyn E2eComponent> {
        Box::new(Fake {
            kind,
            log: Arc::clone(log),
            fail_start,
        })
    }
}

impl E2eComponent for Fake {
    fn kind(&self) -> E2eComponentKind {
        self.kind
    }
    fn browser(&self) -> Option<Browser> {
        // BrowserDriver は config() のブラウザ（Chrome）に結び付く。他は結び付かない。
        (self.kind == E2eComponentKind::BrowserDriver).then_some(Browser::Chrome)
    }
    fn start(&mut self) -> Result<(), E2eError> {
        if self.fail_start {
            return Err(E2eError::new(E2eErrorKind::StartFailed, self.kind));
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("start {:?}", self.kind));
        Ok(())
    }
    fn stop(&mut self) -> Result<(), E2eError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("stop {:?}", self.kind));
        Ok(())
    }
}

fn config() -> E2eConfig {
    E2eConfig::new(PasskeyOperation::Registration, Browser::Chrome)
}

fn entries(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

/// 渡した順に依らず RelyingParty → BrowserDriver で起動し、逆順で停止する。
#[test]
fn starts_in_fixed_order_and_stops_in_reverse() {
    let log: Log = Log::default();
    let cfg = config()
        .with(Fake::boxed(E2eComponentKind::BrowserDriver, &log, false))
        .with(Fake::boxed(E2eComponentKind::RelyingParty, &log, false));
    let running = E2eHarness::start(cfg).unwrap();
    assert_eq!(
        running.kinds(),
        vec![
            E2eComponentKind::RelyingParty,
            E2eComponentKind::BrowserDriver
        ]
    );
    assert_eq!(running.operation(), PasskeyOperation::Registration);
    assert_eq!(running.browser(), Browser::Chrome);
    running.stop().unwrap();
    assert_eq!(
        entries(&log),
        vec![
            "start RelyingParty",
            "start BrowserDriver",
            "stop BrowserDriver",
            "stop RelyingParty"
        ]
    );
}

/// 同じ種類の 2 つ目は起動前に誤りになり、どれも起動しない。
#[test]
fn duplicate_kind_is_rejected_before_any_start() {
    let log: Log = Log::default();
    let cfg = config()
        .with(Fake::boxed(E2eComponentKind::RelyingParty, &log, false))
        .with(Fake::boxed(E2eComponentKind::RelyingParty, &log, false));
    let err = E2eHarness::start(cfg).err().unwrap();
    assert_eq!(err.kind(), E2eErrorKind::Duplicate);
    assert!(entries(&log).is_empty());
}

/// 構成のブラウザと食い違うドライバは起動前に拒否され、どれも起動しない。
#[test]
fn browser_mismatch_is_rejected_before_any_start() {
    struct Gecko(Log);
    impl E2eComponent for Gecko {
        fn kind(&self) -> E2eComponentKind {
            E2eComponentKind::BrowserDriver
        }
        fn browser(&self) -> Option<Browser> {
            Some(Browser::Firefox)
        }
        fn start(&mut self) -> Result<(), E2eError> {
            self.0.lock().unwrap().push("start".to_owned());
            Ok(())
        }
        fn stop(&mut self) -> Result<(), E2eError> {
            Ok(())
        }
    }
    let log: Log = Log::default();
    let cfg = config().with(Box::new(Gecko(Arc::clone(&log))));
    let err = E2eHarness::start(cfg).err().unwrap();
    assert_eq!(err.kind(), E2eErrorKind::BrowserMismatch);
    assert!(entries(&log).is_empty());
}

/// 後ろの起動が失敗すると、起動済みを停止してから誤りが返る。
#[test]
fn failed_start_stops_started_components() {
    let log: Log = Log::default();
    let cfg = config()
        .with(Fake::boxed(E2eComponentKind::RelyingParty, &log, false))
        .with(Fake::boxed(E2eComponentKind::BrowserDriver, &log, true));
    let err = E2eHarness::start(cfg).err().unwrap();
    assert_eq!(err.kind(), E2eErrorKind::StartFailed);
    assert_eq!(
        entries(&log),
        vec!["start RelyingParty", "stop RelyingParty"]
    );
    // SECR-11・SECR-23: E2eError は種類（kind）と構成要素の種類しか保持できず、
    // 秘密値・URL・パスを載せる欄が型に無い。そのため値の混入は型で保証し、ここでは検査しない。
}

/// `Drop` でも停止が呼ばれる。
#[test]
fn drop_stops_components() {
    let log: Log = Log::default();
    {
        let cfg = config().with(Fake::boxed(E2eComponentKind::RelyingParty, &log, false));
        let _running = E2eHarness::start(cfg).unwrap();
    }
    assert_eq!(
        entries(&log),
        vec!["start RelyingParty", "stop RelyingParty"]
    );
}

/// browser() 未実装（None）の BrowserDriver は起動前に拒否する。
#[test]
fn browser_driver_without_browser_is_rejected() {
    struct Unbound;
    impl E2eComponent for Unbound {
        fn kind(&self) -> E2eComponentKind {
            E2eComponentKind::BrowserDriver
        }
        fn start(&mut self) -> Result<(), E2eError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), E2eError> {
            Ok(())
        }
    }
    let Err(err) = E2eHarness::start(config().with(Box::new(Unbound))) else {
        panic!("started");
    };
    assert_eq!(err.kind(), E2eErrorKind::BrowserMismatch);
}
