//! TASK-13 (2/8): DB の遮断と遅延の注入の試験。Docker・外部ネットワークなし（上流はローカルの echo）。
//! 時間に依る試験は下限だけを確かめる。値は試験の中だけの仮の値（THRT-29・A-2026-10-28-1）。
//! 記録は事実の出力で合否の判定ではない（ACPT-7）。

use std::io::{ErrorKind, Read, Write};
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

/// 往復の結果。タイムアウトは切断と区別する（応答を止めるだけの実装を「切れた」と読まないため。AUDB-6）。
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Echoed(Vec<u8>),
    Disconnected,
    TimedOut,
}

/// 切断を示す I/O エラーだけを `Disconnected` にする。タイムアウトは `TimedOut`、それ以外は試験の失敗。
fn classify(e: &std::io::Error) -> Outcome {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Outcome::TimedOut,
        ErrorKind::UnexpectedEof
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe => Outcome::Disconnected,
        kind => panic!("unexpected io error kind: {kind:?}"),
    }
}

fn outcome(s: &mut TcpStream, msg: &[u8]) -> Outcome {
    if let Err(e) = s.write_all(msg) {
        return classify(&e);
    }
    let mut buf = vec![0u8; msg.len()];
    match s.read_exact(&mut buf) {
        Ok(()) => Outcome::Echoed(buf),
        Err(e) => classify(&e),
    }
}

/// 1 往復。つながっていれば Some(応答)、明示の切断なら None。タイムアウトは失敗にする。
fn roundtrip(s: &mut TcpStream, msg: &[u8]) -> Option<Vec<u8>> {
    match outcome(s, msg) {
        Outcome::Echoed(b) => Some(b),
        Outcome::Disconnected => None,
        Outcome::TimedOut => panic!("round trip timed out (not a disconnect)"),
    }
}

/// 遮断中の新しい接続。待ち受けは閉じないので connect は成功し、転送器が accept した直後に切る。
/// 往復は明示の切断になる。タイムアウト・別のエラーは失敗にする。
fn assert_cut_while_blocked(port: u16) {
    let mut s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )
    .expect("connect succeeds while blocked");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(outcome(&mut s, b"c"), Outcome::Disconnected);
}

/// 応答を返さずに黙る接続は `TimedOut` と分類する（`roundtrip` が切断として通さないことの確認）。
#[test]
fn silent_peer_is_timed_out_not_disconnected() {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let mut c = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    c.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let (_held, _) = l.accept().unwrap();
    assert_eq!(outcome(&mut c, b"x"), Outcome::TimedOut);
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
    assert_cut_while_blocked(p.listen_port());
    assert!(p.refused_count() >= 1, "blocked conn is counted as refused");

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
    assert!(roundtrip(&mut c, b"z").is_some());
    // 時間の上限は見ない。遮断されず（refused 0）転送された（accepted 1）ことで確かめる。
    assert_eq!(p.refused_count(), 0);
    assert_eq!(p.accepted_count(), 1);
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

#[test]
fn release_delay_ends_in_flight_wait() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    p.delay(&mut tl, Duration::from_secs(30)).unwrap();
    let mut c = connect(p.listen_port());
    c.write_all(b"ping").unwrap();
    // 応答が遅延の待ちに入るのを待ってから解除する。
    thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    p.release_delay(&mut tl).unwrap();
    let mut buf = [0u8; 4];
    c.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn delay_applies_to_open_connection_with_traffic_in_flight() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let mut tl = timeline();
    let mut c = connect(p.listen_port());
    // 通信が流れ続ける接続に遅延を適用し、適用後の応答に遅延が掛かることを確かめる。
    for _ in 0..20 {
        assert!(roundtrip(&mut c, b"a").is_some());
    }
    p.delay(&mut tl, Duration::from_millis(300)).unwrap();
    let t = Instant::now();
    assert!(roundtrip(&mut c, b"b").is_some());
    assert!(t.elapsed() >= Duration::from_millis(250));
    p.release_delay(&mut tl).unwrap();
}

/// 受けたバイト数を数える上流（返さない）。接続ごとに、読み取りの終了（EOF・エラー）を channel で知らせる。
fn counting_upstream() -> (
    u16,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    mpsc::Receiver<()>,
) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = Arc::clone(&n);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let n = Arc::clone(&n2);
            let tx = tx.clone();
            thread::spawn(move || {
                let mut s = s;
                let mut b = [0u8; 1024];
                while let Ok(k) = s.read(&mut b) {
                    if k == 0 {
                        break;
                    }
                    n.fetch_add(k, Ordering::SeqCst);
                }
                let _ = tx.send(());
            });
        }
    });
    (port, n, rx)
}

/// ORST-17・#5983: `block()` が戻った後は、開いていた接続から上流へ新しい塊が届かない。
/// 戻る前に送った分は下限として数え、戻った後の増加だけを見る。
#[test]
fn no_client_bytes_reach_upstream_after_block_returns() {
    use std::sync::atomic::Ordering;
    let (up, count, ended) = counting_upstream();
    let mut p = DbFaultProxy::start_to_port(up).unwrap();
    let mut tl = timeline();
    let mut c = connect(p.listen_port());
    c.write_all(b"before").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while count.load(Ordering::SeqCst) < 6 {
        assert!(Instant::now() < deadline, "first chunk never arrived");
        thread::sleep(Duration::from_millis(5));
    }
    p.block(&mut tl).unwrap();
    // 戻った後の転送器は上流へ書かない。カーネルの送信バッファに入っていた分は届き得るので、
    // 対象の接続の上流側の読み取りが終わる（切断）のを期限付きで待ってから控える。
    ended
        .recv_timeout(Duration::from_secs(5))
        .expect("upstream side of the conn was not closed after block");
    let settled = count.load(Ordering::SeqCst);
    for _ in 0..20 {
        if c.write_all(b"after").is_err() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(count.load(Ordering::SeqCst), settled, "bytes after block");
    let mut buf = [0u8; 8];
    match c.read(&mut buf) {
        Ok(0) => {}
        Ok(_) => panic!("unexpected data"),
        Err(e) => assert_eq!(classify(&e), Outcome::Disconnected),
    }
    p.release_block(&mut tl).unwrap();
}

/// ORST-17・#5983: 上流が書き続ける接続は、`block()` の後に期限の中で切断に至る（client 向き）。
/// バッファに既に入っていた分のバイト数は問わない。
#[test]
fn streaming_conn_ends_after_block_returns() {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let up = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            thread::spawn(move || {
                let mut s = s;
                while s.write_all(&[0u8; 512]).is_ok() {
                    thread::sleep(Duration::from_millis(2));
                }
            });
        }
    });
    let mut p = DbFaultProxy::start_to_port(up).unwrap();
    let mut tl = timeline();
    let mut c = connect(p.listen_port());
    let mut buf = [0u8; 512];
    c.read_exact(&mut buf).unwrap();
    p.block(&mut tl).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match c.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => assert!(Instant::now() < deadline, "still streaming after block"),
            Err(e) => {
                assert_eq!(classify(&e), Outcome::Disconnected);
                break;
            }
        }
    }
}

/// #5983: `Applied`・`Released` の `offset_ms` は、呼び出しが戻る時刻より後にならない（下限の契約）。
#[test]
fn recorded_offsets_are_not_after_call_return() {
    let mut p = DbFaultProxy::start_to_port(echo_upstream()).unwrap();
    let start = Instant::now();
    let mut tl = FaultTimeline::start(start, t0()).unwrap();
    p.block(&mut tl).unwrap();
    let after_block = start.elapsed().as_millis() as u64;
    p.release_block(&mut tl).unwrap();
    let after_release = start.elapsed().as_millis() as u64;
    let text = finish(tl).to_string();
    let offsets: Vec<u64> = text
        .lines()
        .filter(|l| l.starts_with("injection:"))
        .map(|l| {
            l.split("offset_ms=")
                .nth(1)
                .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse().ok())
                .expect("offset")
        })
        .collect();
    assert_eq!(offsets.len(), 2);
    assert!(offsets[0] <= after_block, "{offsets:?} {after_block}");
    assert!(offsets[1] <= after_release, "{offsets:?} {after_release}");
    assert!(offsets[0] <= offsets[1]);
}

/// ORST-17・#5983: 遮断中に connect して読まずに置いた接続は、解除の後に往復しても上流へ届かない。
/// accept のスレッドが先に切っても、解除の排出で切っても合格（「届かない」だけを固定する）。
#[test]
fn conn_queued_during_block_never_reaches_upstream_after_release() {
    use std::sync::atomic::Ordering;
    let (up, count, _ended) = counting_upstream();
    let mut p = DbFaultProxy::start_to_port(up).unwrap();
    let mut tl = timeline();
    p.block(&mut tl).unwrap();
    let mut queued = connect(p.listen_port());
    p.release_block(&mut tl).unwrap();
    let _ = queued.write_all(b"late");
    // 対象の接続が切れるのを期限付きで待つ（転送されていれば切れず、読みはタイムアウトになる）。
    let mut buf = [0u8; 8];
    match queued.read(&mut buf) {
        Ok(0) => {}
        Ok(_) => panic!("unexpected data"),
        Err(e) => assert_eq!(classify(&e), Outcome::Disconnected),
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "queued conn reached upstream"
    );
    assert_eq!(p.accepted_count(), 0);
    let mut after = connect(p.listen_port());
    let _ = after.write_all(b"ok");
    let deadline = Instant::now() + Duration::from_secs(5);
    while count.load(Ordering::SeqCst) < 2 {
        assert!(
            Instant::now() < deadline,
            "later conn never reached upstream"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
