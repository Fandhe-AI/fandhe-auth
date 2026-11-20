//! TASK-13 (4/8): DB の外の保存先の遮断の注入の試験。Docker・外部ネットワークなし（上流はローカルの echo）。
//! 時間に依る確認は下限だけにする。記録は事実の出力で合否の判定ではない（ACPT-7）。ORST-17・ORST-2。

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{
    DbFaultProxy, ExternalStoreFaultError, ExternalStoreFaultProxy, FaultRecordError,
};
use fandhe_auth_it_harness::record::UtcTimestamp;
use fandhe_auth_it_harness::{EndpointHandle, FaultRecord, FaultTimeline};

/// 遮断の間は待ち受けのポートを手放すので、同じ試験プロセスの他の試験が `bind(0)` で同じポートを取ると
/// 解除が失敗する（方式の限界。`docs/harness/fault-injection.md`）。このため試験どうしを直列にする。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 解除する。他のプロセスが遮断中にポートを取っていると `RebindFailed`（遮断のまま・記録なし）になる。
/// これは方式の限界（`docs/harness/fault-injection.md`）で、取られていなくなるまで再試行する。
/// それ以外の誤りは失敗にする。
fn release_retry(p: &mut ExternalStoreFaultProxy, tl: &mut FaultTimeline) {
    for _ in 0..500 {
        match p.release_block(tl) {
            Ok(()) => return,
            Err(ExternalStoreFaultError::RebindFailed) => thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("release failed: {e:?}"),
        }
    }
    panic!("port stayed taken");
}

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

/// 上流の echo サーバー。drop で待ち受けと接続のスレッドを止めて回収する。
struct EchoUpstream {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl EchoUpstream {
    fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for EchoUpstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn echo_upstream() -> EchoUpstream {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    l.set_nonblocking(true).unwrap();
    let port = l.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        let mut conns = Vec::new();
        while !stop2.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    let stop3 = Arc::clone(&stop2);
                    conns.push(thread::spawn(move || echo_conn(s, &stop3)));
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        for c in conns {
            let _ = c.join();
        }
    });
    EchoUpstream {
        port,
        stop,
        handle: Some(handle),
    }
}

fn echo_conn(mut s: TcpStream, stop: &AtomicBool) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_millis(50)));
    let mut b = [0u8; 1024];
    while !stop.load(Ordering::SeqCst) {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => {
                if s.write_all(&b[..n]).is_err() {
                    break;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
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

/// 往復の結果。タイムアウトは切断と区別する（接続を閉じずに応答を止める回帰を検出するため）。
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Echoed(Vec<u8>),
    Disconnected,
    TimedOut,
}

fn classify(e: &std::io::Error) -> Outcome {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Outcome::TimedOut,
        // 切断を示す種類だけ。それ以外の I/O エラーは試験の失敗にする（原因を隠さない）。
        ErrorKind::UnexpectedEof
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe => Outcome::Disconnected,
        kind => panic!("unexpected io error kind: {kind:?}"),
    }
}

fn roundtrip(s: &mut TcpStream, msg: &[u8]) -> Outcome {
    if let Err(e) = s.write_all(msg) {
        return classify(&e);
    }
    let mut buf = vec![0u8; msg.len()];
    match s.read_exact(&mut buf) {
        Ok(()) => Outcome::Echoed(buf),
        Err(e) => classify(&e),
    }
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    f()
}

#[test]
fn block_cuts_open_and_refuses_new_then_release_recovers() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    let mut open = connect(p.listen_port());
    assert_eq!(roundtrip(&mut open, b"a"), Outcome::Echoed(b"a".to_vec()));

    p.block(&mut tl).unwrap();
    assert_eq!(
        roundtrip(&mut open, b"b"),
        Outcome::Disconnected,
        "open conn is cut"
    );
    assert_refused_or_cut(p.listen_port(), "new conn is rejected");

    release_retry(&mut p, &mut tl);
    let mut again = connect(p.listen_port());
    assert_eq!(roundtrip(&mut again, b"d"), Outcome::Echoed(b"d".to_vec()));
    assert!(wait_until(|| p.accepted_count() >= 2));

    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
    assert!(text.contains("unreleased_count: 0\n"));
    assert!(text.contains("applied external_store_block target=external_store"));
    assert!(text.contains("released external_store_block target=external_store"));
    assert!(!text.contains("db_block"));
}

/// 遮断中の新しい接続は、待ち受けが閉じているのでカーネルが拒否する（ECONNREFUSED）。
/// 万一つながっても、往復は切断になる。上流へは届かない。
fn assert_refused_or_cut(port: u16, what: &str) {
    match TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    ) {
        Err(e) => assert_eq!(e.kind(), ErrorKind::ConnectionRefused, "{what}"),
        Ok(mut s) => {
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            assert_eq!(roundtrip(&mut s, b"x"), Outcome::Disconnected, "{what}");
        }
    }
}

/// ORST-17: 遮断中に始めた接続は、解除の後に上流へ届かない。遮断中の connect は拒否され、
/// 解除後の新しい接続だけが届く。拒否を待たずに続けて解除を繰り返す形で、順序の取りこぼしを探す。
#[test]
fn conn_started_during_block_never_reaches_upstream_after_release() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    for _ in 0..30 {
        p.block(&mut tl).unwrap();
        assert_refused_or_cut(p.listen_port(), "conn during block");
        release_retry(&mut p, &mut tl);
    }
    assert_eq!(p.accepted_count(), 0, "nothing reached upstream");
    let mut after = connect(p.listen_port());
    assert_eq!(roundtrip(&mut after, b"r"), Outcome::Echoed(b"r".to_vec()));
}

/// 遮断中に再接続を繰り返す相手がいても、解除は期限の中で戻る。
#[test]
fn release_returns_in_time_under_reconnect_storm() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    let port = p.listen_port();
    p.block(&mut tl).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    let storm = thread::spawn(move || {
        while !stop2.load(Ordering::SeqCst) {
            let _ = TcpStream::connect_timeout(
                &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                Duration::from_millis(200),
            );
        }
    });
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let r = p.release_block(&mut tl);
    let elapsed = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    storm.join().unwrap();
    r.unwrap();
    assert!(elapsed < Duration::from_secs(5), "release returned late");
    let mut c = connect(port);
    assert_eq!(roundtrip(&mut c, b"s"), Outcome::Echoed(b"s".to_vec()));
}

/// 作り直しに失敗したら誤りを返し、遮断のまま `Released` を記録しない（fail-closed）。
#[test]
fn release_fails_closed_when_port_is_taken() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    let port = p.listen_port();
    p.block(&mut tl).unwrap();
    let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    assert_eq!(
        p.release_block(&mut tl),
        Err(ExternalStoreFaultError::RebindFailed)
    );
    drop(squatter);
    assert_refused_or_cut(port, "still blocked after failed release");
    release_retry(&mut p, &mut tl);
    let mut c = connect(port);
    assert_eq!(roundtrip(&mut c, b"t"), Outcome::Echoed(b"t".to_vec()));
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"), "one Released only");
    assert!(text.contains("unreleased_count: 0\n"));
}

#[test]
fn baseline_timeline_rejects_injection_and_does_not_inject() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = FaultTimeline::baseline(Instant::now(), t0()).unwrap();
    assert_eq!(
        p.block(&mut tl),
        Err(ExternalStoreFaultError::Record(
            FaultRecordError::EventsInBaseline
        ))
    );
    let mut c = connect(p.listen_port());
    assert_eq!(roundtrip(&mut c, b"z"), Outcome::Echoed(b"z".to_vec()));
    assert_eq!(p.refused_count(), 0);
}

#[test]
fn double_and_missing_operations_are_rejected_without_recording() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
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
    release_retry(&mut p, &mut tl);
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
}

#[test]
fn db_block_and_external_store_block_coexist_in_one_timeline() {
    let _serial = serial();
    let up = echo_upstream();
    let mut db = DbFaultProxy::start_to_port(up.port()).unwrap();
    let mut ext = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    db.block(&mut tl).unwrap();
    ext.block(&mut tl).unwrap();
    release_retry(&mut ext, &mut tl);
    db.release_block(&mut tl).unwrap();
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 4\n"));
    assert!(text.contains("unreleased_count: 0\n"));
    assert!(text.contains("applied db_block target=database"));
    assert!(text.contains("applied external_store_block target=external_store"));
}

#[test]
fn stop_without_release_shows_unreleased() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    p.block(&mut tl).unwrap();
    p.stop();
    let text = finish(tl).to_string();
    assert!(text.contains("unreleased_count: 1\n"));
}

#[test]
fn debug_and_display_do_not_leak_port() {
    let _serial = serial();
    let up = echo_upstream();
    let p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let port = p.listen_port().to_string();
    let dbg = format!("{p:?}");
    assert!(!dbg.contains(&port));
    for e in [
        ExternalStoreFaultError::StartFailed,
        ExternalStoreFaultError::AlreadyActive,
        ExternalStoreFaultError::NotActive,
        ExternalStoreFaultError::Internal,
        ExternalStoreFaultError::RebindFailed,
        ExternalStoreFaultError::RebindFailedAfterRecord,
        ExternalStoreFaultError::Record(FaultRecordError::EventsInBaseline),
    ] {
        assert!(!e.to_string().contains(&port));
    }
}

#[test]
fn start_for_with_empty_handle_closes_connections() {
    let _serial = serial();
    let p = ExternalStoreFaultProxy::start_for(EndpointHandle::default()).unwrap();
    let mut c = connect(p.listen_port());
    assert_eq!(roundtrip(&mut c, b"x"), Outcome::Disconnected);
}

/// 上流の待ち行列を満杯にして、新しい上流への connect が固まる状態を作る（accept のスレッドを止める用）。
/// 返す値は drop まで保つ（待ち受けと満杯にした接続）。
struct StalledUpstream {
    port: u16,
    _listener: TcpListener,
    _filled: Vec<TcpStream>,
}

fn stalled_upstream() -> StalledUpstream {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut filled = Vec::new();
    loop {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(100)) {
            Ok(s) => filled.push(s),
            Err(e) if e.kind() == ErrorKind::TimedOut => break,
            Err(e) => panic!("unexpected connect error: {:?}", e.kind()),
        }
        assert!(filled.len() < 20_000, "backlog never filled");
    }
    StalledUpstream {
        port,
        _listener: listener,
        _filled: filled,
    }
}

/// 遮断中に始めた接続・遮断は、accept のスレッドが上流への接続で固まっていても待たされない。
/// `Applied` の記録から実際の遮断までに、上流への接続の上限（5 秒）に及ぶずれを作らない（ORST-17）。
/// 固まった接続は遮断で切れる。
#[test]
fn block_returns_promptly_while_accept_is_stalled_on_upstream() {
    let _serial = serial();
    let up = stalled_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port).unwrap();
    let mut tl = timeline();
    let mut stuck = connect(p.listen_port());
    // accept のスレッドが上流への connect に入るのを待つ（値は下限。待ちすぎても害はない）。
    thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    p.block(&mut tl).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "block waited for the stalled accept: {elapsed:?}"
    );
    assert_eq!(
        roundtrip(&mut stuck, b"s"),
        Outcome::Disconnected,
        "stalled conn is cut by the block"
    );
}

/// 接続の試行が続いていても `block()` は期限の中で戻る（Cursor Bugbot の指摘の回帰防止。
/// accept のスレッドがロックを取り直し続けて `block()` を待たせない）。
#[test]
fn block_returns_in_time_under_connection_storm() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    let port = p.listen_port();
    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    if let Ok(mut s) = TcpStream::connect_timeout(
                        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                        Duration::from_millis(200),
                    ) {
                        let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
                        let _ = s.write_all(b"w");
                        let mut b = [0u8; 1];
                        let _ = s.read(&mut b);
                    }
                }
            })
        })
        .collect();
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    p.block(&mut tl).unwrap();
    let elapsed = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    for w in workers {
        w.join().unwrap();
    }
    assert!(
        elapsed < Duration::from_secs(2),
        "block starved: {elapsed:?}"
    );
}

/// ORST-17: 解除と接続の試行を同時に始め、`Released` の記録より前に確立した接続が上流へ届かないことを見る。
/// 判定できるのは「接続が完了した時刻（ms）が `Released` の offset_ms より小さい」接続だけ（それは届かない）。
/// 窓は小さいので、この試験は競合を探す補助で、決定的な確認は `fault/db.rs` の単体試験が行う。
#[test]
fn conns_established_before_release_record_do_not_echo() {
    let _serial = serial();
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let origin = Instant::now();
    let mut tl = FaultTimeline::start(origin, t0()).unwrap();
    let port = p.listen_port();
    p.block(&mut tl).unwrap();
    let workers_n = 6;
    let barrier = Arc::new(std::sync::Barrier::new(workers_n + 1));
    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (0..workers_n)
        .map(|w| {
            let (barrier, stop) = (Arc::clone(&barrier), Arc::clone(&stop));
            thread::spawn(move || {
                barrier.wait();
                let mut echoed_at = Vec::new();
                let mut seq = 0u32;
                while !stop.load(Ordering::SeqCst) {
                    seq += 1;
                    let Ok(mut s) = TcpStream::connect_timeout(
                        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                        Duration::from_millis(200),
                    ) else {
                        continue;
                    };
                    let done_ms = origin.elapsed().as_millis();
                    let msg = format!("{w}-{seq}").into_bytes();
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                    if s.write_all(&msg).is_err() {
                        continue;
                    }
                    let mut buf = vec![0u8; msg.len()];
                    if s.read_exact(&mut buf).is_ok() && buf == msg {
                        echoed_at.push(done_ms);
                    }
                }
                echoed_at
            })
        })
        .collect();
    barrier.wait();
    thread::sleep(Duration::from_millis(5));
    release_retry(&mut p, &mut tl);
    thread::sleep(Duration::from_millis(100));
    stop.store(true, Ordering::SeqCst);
    let echoed: Vec<u128> = workers
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    let text = finish(tl).to_string();
    let released_ms: u128 = text
        .lines()
        .find(|l| l.contains("released external_store_block"))
        .and_then(|l| l.split("offset_ms=").nth(1))
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .expect("released offset");
    assert!(
        !echoed.is_empty(),
        "no connection reached upstream after release"
    );
    for at in &echoed {
        assert!(
            *at >= released_ms,
            "conn done at {at} ms echoed before Released at {released_ms} ms"
        );
    }
}
