//! DB 構成要素の起動・停止の試験（TASK-10 (2/6)）。Docker が要るので `#[ignore]`。`make it-db` で流す。
//!
//! 下の Fake Host は Host の実体の代わりではなく、起動順（Database が先）の確認だけに使う。
//! Host の起動は未達で、この試験の合格に数えない（ACPT-7・TODO(TASK-716)）。

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fandhe_auth_it_harness::{
    Component, ComponentKind, Database, DatabaseConfig, EndpointHandle, Harness, HarnessConfig,
    HarnessError,
};

struct FakeHost {
    handle: EndpointHandle,
    seen: Arc<Mutex<Option<u16>>>,
}

impl Component for FakeHost {
    fn kind(&self) -> ComponentKind {
        ComponentKind::Host
    }
    fn start(&mut self) -> Result<(), HarnessError> {
        *self.seen.lock().unwrap() = self.handle.get().map(|e| e.port);
        Ok(())
    }
    fn stop(&mut self) -> Result<(), HarnessError> {
        Ok(())
    }
}

#[test]
#[ignore = "requires docker (dev/postgres); run via make it-db"]
fn database_starts_and_stops_per_test() {
    let db = Database::new(DatabaseConfig::new());
    let project = db.project().to_string();
    let handle = db.endpoint_handle();
    let seen = Arc::new(Mutex::new(None));
    let host = FakeHost {
        handle: handle.clone(),
        seen: seen.clone(),
    };
    let running = Harness::start(HarnessConfig::new().with(Box::new(host)).with(Box::new(db)))
        .expect("harness starts");

    let ep = handle.get().expect("endpoint after start");
    assert_eq!(
        *seen.lock().unwrap(),
        Some(ep.port),
        "Database starts before Host"
    );
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, ep.port)),
        Duration::from_secs(5),
    )
    .expect("reachable");

    running.stop().expect("stops");
    assert!(handle.get().is_none());
    let out = Command::new("docker")
        .args(["ps", "-a", "-q", "--filter"])
        .arg(format!("label=com.docker.compose.project={project}"))
        .output()
        .expect("docker ps");
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "no leftover containers");
}
