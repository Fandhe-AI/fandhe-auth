//! TASK-13 (2/8): 実際の PostgreSQL（dev/postgres）に対する DB の遮断と遅延の試験。Docker が要るので
//! `#[ignore]`。`make it-db` で流す。ドライバーは使わず、固定の 8 バイトの SSLRequest（長さ 8・要求コード
//! 80877103）に対する 1 バイトの `S` の応答だけで経路を確かめる（秘密値は使わない）。
//! 記録は事実の出力で合否の判定ではない（ACPT-7）。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use fandhe_auth_it_harness::fault::DbFaultProxy;
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{Database, DatabaseConfig, FaultTimeline, Harness, HarnessConfig};

const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f];

/// SSLRequest を送って 1 バイトの応答を読む。切れていれば `None`。
fn ssl_probe(port: u16) -> Option<u8> {
    let mut s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )
    .ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(&SSL_REQUEST).ok()?;
    let mut b = [0u8; 1];
    s.read_exact(&mut b).ok()?;
    Some(b[0])
}

#[test]
#[ignore = "requires docker (dev/postgres); run via make it-db"]
fn db_block_and_delay_through_proxy() {
    let db = Database::new(DatabaseConfig::new());
    let project = db.project().to_string();
    let handle = db.endpoint_handle();
    // 転送器は Database の起動前に作れる（接続のたびにポートを引く）。
    let mut proxy = DbFaultProxy::start_for(handle.clone()).expect("proxy starts");
    let running = Harness::start(HarnessConfig::new().with(Box::new(db))).expect("harness starts");
    let ep = proxy.proxied_endpoint().expect("endpoint after start");
    assert_ne!(ep.port, handle.get().expect("upstream").port);
    let port = proxy.listen_port();

    let mut tl = FaultTimeline::start(Instant::now(), SystemTime::now()).unwrap();
    assert_eq!(ssl_probe(port), Some(b'S'));

    proxy.block(&mut tl).unwrap();
    assert_eq!(ssl_probe(port), None, "blocked");
    proxy.release_block(&mut tl).unwrap();
    assert_eq!(ssl_probe(port), Some(b'S'), "recovered");

    proxy.delay(&mut tl, Duration::from_millis(300)).unwrap();
    let t = Instant::now();
    assert_eq!(ssl_probe(port), Some(b'S'));
    assert!(t.elapsed() >= Duration::from_millis(300));
    proxy.release_delay(&mut tl).unwrap();

    let rec = tl
        .finish(UtcTimestamp::from_system_time(SystemTime::now()).unwrap())
        .unwrap();
    assert_eq!(rec.unreleased_count(), 0);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = rec.write_new(&dir).expect("record written");
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!text.contains(&port.to_string()));
    assert!(!text.contains(&project));

    proxy.stop();
    running.stop().expect("stops");
    let out = Command::new("docker")
        .args(["ps", "-a", "-q", "--filter"])
        .arg(format!("label=com.docker.compose.project={project}"))
        .output()
        .expect("docker ps");
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "no leftover containers");
}
