//! TASK-13 (4/8): DB の外の保存先の遮断の注入の試験。Docker・外部ネットワークなし（上流はローカルの echo）。
//! 時間に依る確認は下限だけにする。記録は事実の出力で合否の判定ではない（ACPT-7）。ORST-17・ORST-2。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{
    DbFaultProxy, ExternalStoreFaultError, ExternalStoreFaultProxy, FaultRecordError,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{EndpointHandle, FaultRecord, FaultTimeline};

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn timeline() -> FaultTimeline {
    FaultTimeline::start(Instant::now(), t0()).unwrap()
}

fn finish(tl: FaultTimeline) -> FaultRecord {
    tl.finish(UtcTimestamp::from_unix_seconds(1_700_000_060))
        .unwrap()
}

fn echo_upstream() -> u16 {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            thread::spawn(move || {
                let mut s = s;
                let mut b = [0u8; 1024];
                while let Ok(n) = s.read(&mut b) {
                    if n == 0 || s.write_all(&b[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )
    .unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

fn roundtrip(s: &mut TcpStream, msg: &[u8]) -> Option<Vec<u8>> {
    if s.write_all(msg).is_err() {
        return None;
    }
    let mut buf = vec![0u8; msg.len()];
    s.read_exact(&mut buf).ok().map(|()| buf)
}

#[test]
fn block_cuts_open_and_refuses_new_then_release_recovers() {
    let mut p = ExternalStoreFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    let mut open = connect(p.listen_port());
    assert!(roundtrip(&mut open, b"a").is_some());

    p.block(&mut tl).unwrap();
    assert!(roundtrip(&mut open, b"b").is_none(), "open conn is cut");
    let mut fresh = connect(p.listen_port());
    assert!(roundtrip(&mut fresh, b"c").is_none(), "new conn is closed");
    assert!(p.refused_count() >= 1);

    p.release_block(&mut tl).unwrap();
    let mut again = connect(p.listen_port());
    assert_eq!(roundtrip(&mut again, b"d").as_deref(), Some(&b"d"[..]));
    assert!(p.accepted_count() >= 2);

    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
    assert!(text.contains("unreleased_count: 0\n"));
    assert!(text.contains("applied external_store_block target=external_store"));
    assert!(text.contains("released external_store_block target=external_store"));
    assert!(!text.contains("db_block"));
}

#[test]
fn baseline_timeline_rejects_injection_and_does_not_inject() {
    let mut p = ExternalStoreFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    assert_eq!(
        p.block(&mut tl),
        Err(ExternalStoreFaultError::Record(
            FaultRecordError::EventsInBaseline
        ))
    );
    let mut c = connect(p.listen_port());
    assert!(roundtrip(&mut c, b"z").is_some());
    assert_eq!(p.refused_count(), 0);
}

#[test]
fn double_and_missing_operations_are_rejected_without_recording() {
    let mut p = ExternalStoreFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    assert_eq!(
        p.release_block(&mut tl),
        Err(ExternalStoreFaultError::NotActive)
    );
    p.block(&mut tl).unwrap();
    assert_eq!(
        p.block(&mut tl),
        Err(ExternalStoreFaultError::AlreadyActive)
    );
    p.release_block(&mut tl).unwrap();
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
}

#[test]
fn db_block_and_external_store_block_coexist_in_one_timeline() {
    let up = echo_upstream();
    let mut db = DbFaultProxy::start_to_port(up).unwrap();
    let mut ext = ExternalStoreFaultProxy::start_to_port(up).unwrap();
    let mut tl = timeline();
    db.block(&mut tl).unwrap();
    ext.block(&mut tl).unwrap();
    ext.release_block(&mut tl).unwrap();
    db.release_block(&mut tl).unwrap();
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 4\n"));
    assert!(text.contains("unreleased_count: 0\n"));
    assert!(text.contains("applied db_block target=database"));
    assert!(text.contains("applied external_store_block target=external_store"));
}

#[test]
fn stop_without_release_shows_unreleased() {
    let mut p = ExternalStoreFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    p.block(&mut tl).unwrap();
    p.stop();
    let text = finish(tl).to_string();
    assert!(text.contains("unreleased_count: 1\n"));
}

#[test]
fn debug_and_display_do_not_leak_port() {
    let p = ExternalStoreFaultProxy::start_to_port(echo_upstream()).unwrap();
    let port = p.listen_port().to_string();
    let dbg = format!("{p:?}");
    assert!(!dbg.contains(&port));
    for e in [
        ExternalStoreFaultError::StartFailed,
        ExternalStoreFaultError::AlreadyActive,
        ExternalStoreFaultError::NotActive,
        ExternalStoreFaultError::Internal,
        ExternalStoreFaultError::Record(FaultRecordError::EventsInBaseline),
    ] {
        assert!(!e.to_string().contains(&port));
    }
}

#[test]
fn start_for_with_empty_handle_closes_connections() {
    let p = ExternalStoreFaultProxy::start_for(EndpointHandle::default()).unwrap();
    let mut c = connect(p.listen_port());
    assert!(roundtrip(&mut c, b"x").is_none());
}
