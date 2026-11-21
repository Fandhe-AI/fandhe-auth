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

/// 接続の確立。失敗は呼び出し側が段階つきで扱えるよう、誤りのまま返す。
fn try_connect(port: u16) -> std::io::Result<TcpStream> {
    let s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    Ok(s)
}

fn connect(port: u16) -> TcpStream {
    try_connect(port).unwrap()
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

/// 往復を一か所で行う。誤りがあったときは種類も返す（失敗の文言用）。比較には `Outcome` だけを使う。
fn roundtrip_detail(s: &mut TcpStream, msg: &[u8]) -> (Outcome, Option<ErrorKind>) {
    if let Err(e) = s.write_all(msg) {
        return (classify(&e), Some(e.kind()));
    }
    let mut buf = vec![0u8; msg.len()];
    match s.read_exact(&mut buf) {
        Ok(()) => (Outcome::Echoed(buf), None),
        Err(e) => (classify(&e), Some(e.kind())),
    }
}

fn roundtrip(s: &mut TcpStream, msg: &[u8]) -> Outcome {
    roundtrip_detail(s, msg).0
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
    assert_cut_while_blocked(p.listen_port(), "new conn is cut");
    assert!(wait_until(|| p.refused_count() >= 1));

    p.release_block(&mut tl).unwrap();
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

/// 遮断中の新しい接続。待ち受けは閉じないので connect は成功し、転送器が accept した直後に切る。
/// 往復は明示の切断になり、上流へは届かない。
fn assert_cut_while_blocked(port: u16, what: &str) {
    let mut s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )
    .expect("connect succeeds while blocked");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(roundtrip(&mut s, b"x"), Outcome::Disconnected, "{what}");
}

/// ORST-17: `block()` の後に転送器が accept した接続は、上流へ届かずに切られる。解除後の新しい接続は届く。
/// 遮断と解除を繰り返し、遮断中の接続が一度も上流へ届かないこと（`accepted_count == 0`）を見る。
/// 解除の瞬間に待ち行列に残っていた接続の扱い（解除の排出で切る）は `fault_db` の試験で固定する（#5983）。
/// この試験は遮断中に接続を張り、切断を観測してから解除する形。
#[test]
fn conn_accepted_during_block_is_cut_and_later_conn_reaches_upstream() {
    let up = echo_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let mut tl = timeline();
    for _ in 0..30 {
        p.block(&mut tl).unwrap();
        assert_cut_while_blocked(p.listen_port(), "conn during block");
        p.release_block(&mut tl).unwrap();
    }
    assert_eq!(p.accepted_count(), 0, "nothing reached upstream");
    assert!(p.refused_count() >= 30);
    let mut after = connect(p.listen_port());
    assert_eq!(roundtrip(&mut after, b"r"), Outcome::Echoed(b"r".to_vec()));
}

/// 遮断中に再接続を繰り返す相手がいても、解除は期限の中で戻る。
///
/// macOS で最後の往復が切れる（`Disconnected`）ことが報告されている（#6016。原因は未確定で、macOS での確認は未検証）。
/// 期待（最後の往復が `Echoed`）は変えない（AUDB-6）。失敗の文言には、失敗した段階（connect・往復）と誤りの種類、
/// 最後の接続の前後の `accepted`・`refused`・上流への試行の差を出す。
/// 集計値の限界: 計数は転送器全体の合計で、最後の接続だけのものではない。`storm.join()` が待つのは接続側のスレッドで、
/// 転送器の accept のスレッドが storm の接続を処理し終えたことは保証しない。accept 済みで上流へ接続中の接続は
/// どの計数も増やさないので、差がすべて 0 でも「カーネルの段で切れた」とは断定できない（手掛かりにとどめる）。
#[test]
fn release_returns_in_time_under_reconnect_storm() {
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
    let before = (
        p.accepted_count(),
        p.refused_count(),
        p.upstream_attempt_count(),
    );
    let failure = match try_connect(port) {
        Err(e) => Some(("connect", format!("{:?}", e.kind()))),
        Ok(mut c) => match roundtrip_detail(&mut c, b"s") {
            (Outcome::Echoed(b), _) if b == b"s" => None,
            (outcome, kind) => Some(("roundtrip", format!("{outcome:?} (io kind {kind:?})"))),
        },
    };
    if let Some((stage, what)) = failure {
        // 診断の値を安定させるため、計数が落ち着くまで短く待つ（合否には使わない。失敗したときだけ）。
        let snap = || {
            (
                p.accepted_count(),
                p.refused_count(),
                p.upstream_attempt_count(),
            )
        };
        let mut last = snap();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
            let now = snap();
            if now == last {
                break;
            }
            last = now;
        }
        panic!(
            "final {stage} after release failed: {what}; accepted +{}, refused +{}, upstream attempts +{} (aggregate counters; see the test doc for limits)",
            last.0.saturating_sub(before.0),
            last.1.saturating_sub(before.1),
            last.2.saturating_sub(before.2)
        );
    }
}

#[test]
fn baseline_timeline_rejects_injection_and_does_not_inject() {
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
    p.release_block(&mut tl).unwrap();
    let text = finish(tl).to_string();
    assert!(text.contains("injection_count: 2\n"));
}

#[test]
fn db_block_and_external_store_block_coexist_in_one_timeline() {
    let up = echo_upstream();
    let mut db = DbFaultProxy::start_to_port(up.port()).unwrap();
    let mut ext = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
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
    let up = echo_upstream();
    let p = ExternalStoreFaultProxy::start_to_port(up.port()).unwrap();
    let port = p.listen_port().to_string();
    let dbg = format!("{p:?}");
    assert!(!dbg.contains(&port));
    for e in [
        ExternalStoreFaultError::StartFailed,
        ExternalStoreFaultError::AlreadyActive,
        ExternalStoreFaultError::NotActive,
        ExternalStoreFaultError::DrainFailed,
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
            Err(e) => panic!(
                "cannot stall upstream connect: error kind {:?} after {} filled connections (accept-queue overflow is one possible cause; fd exhaustion or an OS that resets on overflow are others)",
                e.kind(),
                filled.len()
            ),
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
///
/// 上流を固める土台は、待ち行列があふれたときに SYN を黙って捨てる挙動（Linux の既定 `tcp_abort_on_overflow=0`）に依る。
/// 土台が組めない環境では `ignore` で外さず、観測した誤りの種類つきで失敗させる（検証を黙って外さない。AUDB-6）。
/// `block()` の前に、accept のスレッドが上流への connect に入ったこと（`upstream_attempt_count`）を待って確かめ、
/// その後も拒否も転送も始まっていないことを見る（中身の無い合格にしない）。
#[test]
fn block_returns_promptly_while_accept_is_stalled_on_upstream() {
    let up = stalled_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port).unwrap();
    let mut tl = timeline();
    let mut stuck = connect(p.listen_port());
    // 停止点への到達: accept のスレッドが上流への connect に入ったことを同期して待つ。
    assert!(
        wait_until(|| p.upstream_attempt_count() >= 1),
        "accept thread never began the upstream connect"
    );
    // 前提: 入った後も上流への接続で止まっており、拒否も転送も始まっていない（上流の待ち行列の満杯は
    // `stalled_upstream` が connect の時間切れで確かめ済み。connect の上限は 5 秒で、ここまでは数 ms）。
    assert_eq!(
        p.refused_count(),
        0,
        "upstream connect failed instead of stalling"
    );
    assert_eq!(p.accepted_count(), 0, "upstream connect was not stalled");
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
