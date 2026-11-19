//! TASK-13 (2/8): DB の遮断と遅延の注入の試験。Docker・外部ネットワークなし（上流はローカルの echo）。
//! 時間に依る試験は下限だけを確かめる。値は試験の中だけの仮の値（THRT-29・A-2026-10-28-1）。
//! 記録は事実の出力で合否の判定ではない（ACPT-7）。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{DbFaultError, DbFaultProxy, FaultRecordError};
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

/// echo の上流。接続ごとにスレッドを立てる（試験プロセスの終了で回収）。
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

/// 1 往復。つながっていれば Some(応答)、切れていれば None。
fn roundtrip(s: &mut TcpStream, msg: &[u8]) -> Option<Vec<u8>> {
    if s.write_all(msg).is_err() {
        return None;
    }
    let mut buf = vec![0u8; msg.len()];
    s.read_exact(&mut buf).ok().map(|()| buf)
}

#[test]
fn forwards_bytes_unchanged() {
    let p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut c = connect(p.listen_port());
    assert_eq!(roundtrip(&mut c, b"hello").as_deref(), Some(&b"hello"[..]));
    assert_eq!(p.accepted_count(), 1);
    assert_eq!(p.refused_count(), 0);
}

#[test]
fn block_cuts_open_and_refuses_new_then_release_recovers() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
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

    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
    assert!(text.contains("unreleased_count: 0\n"));
    assert!(text.contains("applied db_block target=database"));
    assert!(text.contains("released db_block target=database"));
}

#[test]
fn delay_adds_lower_bound_and_release_removes_it() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    let mut c = connect(p.listen_port());
    p.delay(&mut tl, Duration::from_millis(200)).unwrap();
    let t = Instant::now();
    assert!(roundtrip(&mut c, b"x").is_some());
    assert!(t.elapsed() >= Duration::from_millis(200));

    p.release_delay(&mut tl).unwrap();
    assert!(roundtrip(&mut c, b"y").is_some());
    let text = finish(tl).to_string();
    assert!(text.contains("applied db_delay target=database"));
    assert!(text.contains("released db_delay target=database"));
    assert!(text.contains("delay_ms=200"));
    assert!(text.contains("unreleased_count: 0\n"));
}

#[test]
fn baseline_timeline_rejects_injection_and_does_not_inject() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    assert_eq!(
        p.block(&mut tl),
        Err(DbFaultError::Record(FaultRecordError::EventsInBaseline))
    );
    assert_eq!(
        p.delay(&mut tl, Duration::from_millis(500)),
        Err(DbFaultError::Record(FaultRecordError::EventsInBaseline))
    );
    let mut c = connect(p.listen_port());
    let t = Instant::now();
    assert!(roundtrip(&mut c, b"z").is_some());
    assert!(t.elapsed() < Duration::from_millis(500));
}

#[test]
fn double_and_missing_operations_are_rejected_without_recording() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    assert_eq!(p.release_block(&mut tl), Err(DbFaultError::NotActive));
    assert_eq!(p.release_delay(&mut tl), Err(DbFaultError::NotActive));
    assert_eq!(
        p.delay(&mut tl, Duration::ZERO),
        Err(DbFaultError::InvalidParam)
    );
    p.block(&mut tl).unwrap();
    assert_eq!(p.block(&mut tl), Err(DbFaultError::AlreadyActive));
    p.delay(&mut tl, Duration::from_millis(5)).unwrap();
    assert_eq!(
        p.delay(&mut tl, Duration::from_millis(6)),
        Err(DbFaultError::AlreadyActive)
    );
    let r = finish(tl);
    assert_eq!(r.events().len(), 2);
}

#[test]
fn unreleased_injections_are_counted() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    p.block(&mut tl).unwrap();
    p.delay(&mut tl, Duration::from_millis(5)).unwrap();
    let r = finish(tl);
    assert_eq!(r.unreleased_count(), 2);
}

#[test]
fn empty_endpoint_handle_fails_closed() {
    let p = DbFaultProxy::start_for(EndpointHandle::default()).unwrap();
    assert!(p.proxied_endpoint().is_none());
    let mut c = connect(p.listen_port());
    assert!(roundtrip(&mut c, b"q").is_none());
    assert_eq!(p.accepted_count(), 0);
}

#[test]
fn stop_and_drop_finish_in_any_state() {
    for state in 0..4 {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
            let mut tl = timeline();
            let mut c = connect(p.listen_port());
            assert!(roundtrip(&mut c, b"s").is_some());
            if state & 1 != 0 {
                p.delay(&mut tl, Duration::from_secs(30)).unwrap();
            }
            if state & 2 != 0 {
                p.block(&mut tl).unwrap();
            }
            if state == 3 {
                p.stop();
            } else {
                drop(p);
            }
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("stop finishes promptly");
    }
}

#[test]
fn outputs_do_not_leak_ports_or_addresses() {
    let up = echo_upstream();
    let mut p = DbFaultProxy::start_to_port(up).unwrap();
    let listen = p.listen_port();
    let mut tl = timeline();
    p.block(&mut tl).unwrap();
    let err = p.block(&mut tl).unwrap_err();
    let texts = [
        format!("{err:?}"),
        err.to_string(),
        format!("{p:?}"),
        finish(tl).to_string(),
    ];
    for t in texts {
        for needle in [
            "127.0.0.1".to_string(),
            format!(":{listen}"),
            format!(":{up}"),
        ] {
            assert!(!t.contains(&needle), "leaked an address or port");
        }
    }
}
