//! TASK-13 (4/8): DB の外の保存先の遮断の注入の試験。Docker・外部ネットワークなし（上流はローカルの echo）。
//! 時間に依る確認は下限だけにする。記録は事実の出力で合否の判定ではない（ACPT-7）。ORST-17・ORST-2。

use std::io::{self, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fandhe_auth_it_harness::fault::{
    DbFaultProxy, ExternalStoreFaultError, ExternalStoreFaultProxy, FaultRecordError, StallExit,
    UpstreamStall,
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

/// 接続の確立。失敗は段階（`connect`＝接続の確立、`set_read_timeout`＝接続後のソケット設定）つきの誤りで返す。
/// 接続確立の失敗とソケット設定の失敗を取り違えない（#6016 の調査）。
fn try_connect(port: u16) -> Result<TcpStream, (&'static str, std::io::Error)> {
    let s = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(5),
    )
    .map_err(|e| ("connect", e))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| ("set_read_timeout", e))?;
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
    /// 分類にない I/O エラー。パニックにせず返し、比較では常に不一致にして試験を失敗させる（原因は診断に出る）。
    Other(ErrorKind),
}

fn classify(e: &std::io::Error) -> Outcome {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Outcome::TimedOut,
        // 切断を示す種類だけ。それ以外は `Other` として返し、期待と食い違って失敗する（種類は診断に出る）。
        ErrorKind::UnexpectedEof
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe => Outcome::Disconnected,
        kind => Outcome::Other(kind),
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
/// 転送器の accept のスレッドが storm の接続を処理し終えたことは保証しない。
/// 各計数は接続が通る処理の段階を表す（詳細は `DbFaultProxy::upstream_attempt_count` の rustdoc）:
/// - `accepted`: 上流へつないで中継を始めた接続（成功の段階。`accepted_count`）
/// - `refused`: accept した後に直ちに閉じた接続。接続の失敗・遮断・停止のほか、上流への接続が成功した後の複製の失敗・
///   世代の不一致も含む（`refused_count`）。接続の失敗と中継前の拒否は `refused` だけでは区別できない
/// - `upstream attempts`: 上流への connect の試行。connect の直前に数えるので接続中の接続も含み、成否を表さない。
///   停止点で待っている接続と、そこで打ち切った接続も数に入る
/// - `upstream connected`: 上流への connect の成功。`attempts` との差が接続の失敗、`connected` と `accepted` の差が
///   中継前の拒否（世代の不一致・複製の失敗）の手掛かりになる
///
/// 4 つの計数は別々の時点に読むため、同一時点の集計ではない（非同期に採取した参考値）。差のずれから
/// 処理中の接続があるとは断定できず、手掛かりにとどめる（差がすべて 0 でも「カーネルの段で切れた」とは断定できない）。
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
        p.upstream_connected_count(),
    );
    let failure = match try_connect(port) {
        Err((stage, e)) => Some((stage, format!("{:?}", e.kind()))),
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
                p.upstream_connected_count(),
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
            "final {stage} after release failed: {what}; accepted +{}, refused +{}, upstream attempts +{}, upstream connected +{} (reference values sampled separately, not one atomic snapshot; see the test doc for limits)",
            last.0.saturating_sub(before.0),
            last.1.saturating_sub(before.1),
            last.2.saturating_sub(before.2),
            last.3.saturating_sub(before.3)
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

/// 上流側で観測した結果（ORST-17）。
#[derive(Debug, PartialEq, Eq)]
enum UpstreamObservation {
    /// 接続を受け、EOF までに受けたバイト数（読み取りの失敗の後は、届いた分があるときだけ）。
    Received(usize),
    /// accept か読み取りが失敗し、バイトが届いたかを確かめられない。
    Unverifiable(ErrorKind),
}

/// 上流の接続を 1 本だけ受け、閉じられるまでの受信バイト数を数える。呼び出し側は blocking のリスナーを渡す。
fn observe_upstream(up: TcpListener) -> thread::JoinHandle<UpstreamObservation> {
    thread::spawn(move || {
        let mut c = match up.accept() {
            Ok((c, _)) => c,
            Err(e) => return UpstreamObservation::Unverifiable(e.kind()),
        };
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        read_until_eof(&mut c)
    })
}

/// EOF までに受けたバイト数を数える。EOF（`Ok(0)`）を確認したときだけ「受信 0 バイト」を表せる。
/// 読み取りの失敗は、バイトが届いたかを確かめられない（RST は届いた分も捨てうる）ので、届いたバイトが
/// 無ければ `Unverifiable`（合格にしない）。届いたバイトがあれば、数を返す（判定は失敗のまま）（ORST-17・AUDB-6）。
fn read_until_eof<R: Read>(c: &mut R) -> UpstreamObservation {
    let mut total = 0;
    let mut buf = [0u8; 64];
    loop {
        match c.read(&mut buf) {
            Ok(0) => return UpstreamObservation::Received(total),
            Ok(n) => total += n,
            Err(_) if total > 0 => return UpstreamObservation::Received(total),
            Err(e) => return UpstreamObservation::Unverifiable(e.kind()),
        }
    }
}

/// ORST-17: 最初の読み取りが `ConnectionReset` 等で失敗したときは、受信 0 バイトとして合格にしない。
/// EOF を確認した場合だけ 0 バイト。バイトを受けた後の失敗は、受けた数を返す。
#[test]
fn read_failure_before_eof_is_unverifiable_not_zero_bytes() {
    struct Script(Vec<io::Result<Vec<u8>>>);
    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.remove(0) {
                Ok(b) => {
                    buf[..b.len()].copy_from_slice(&b);
                    Ok(b.len())
                }
                Err(e) => Err(e),
            }
        }
    }
    for kind in [
        ErrorKind::ConnectionReset,
        ErrorKind::ConnectionAborted,
        ErrorKind::BrokenPipe,
        ErrorKind::UnexpectedEof,
        ErrorKind::TimedOut,
        ErrorKind::WouldBlock,
        ErrorKind::Interrupted,
    ] {
        let obs = read_until_eof(&mut Script(vec![Err(kind.into())]));
        assert_eq!(obs, UpstreamObservation::Unverifiable(kind));
        assert!(judge_upstream(obs).is_err());
    }
    assert_eq!(
        read_until_eof(&mut Script(vec![Ok(vec![])])),
        UpstreamObservation::Received(0)
    );
    assert_eq!(
        read_until_eof(&mut Script(vec![
            Ok(b"ab".to_vec()),
            Err(ErrorKind::ConnectionReset.into())
        ])),
        UpstreamObservation::Received(2)
    );
}

/// 観測の判定。受信 0 バイトだけが合格で、バイトが届いた場合と観測できない場合は失敗（緩和の分岐を持たない）。
fn judge_upstream(obs: UpstreamObservation) -> Result<(), String> {
    match obs {
        UpstreamObservation::Received(0) => Ok(()),
        UpstreamObservation::Received(n) => {
            Err(format!("bytes from the severed conn reached upstream: {n}"))
        }
        UpstreamObservation::Unverifiable(kind) => Err(format!(
            "could not verify that no bytes reached upstream: {kind:?}"
        )),
    }
}

/// ORST-17: 観測の判定は、バイトの到達と観測の不能を合格にしない（accept が `ConnectionAborted`・`ConnectionReset` を返す
/// macOS の経路と同じ判定を、Linux でも通す）。
#[test]
fn judge_upstream_accepts_only_zero_bytes() {
    assert_eq!(judge_upstream(UpstreamObservation::Received(0)), Ok(()));
    assert!(judge_upstream(UpstreamObservation::Received(1)).is_err());
    for kind in [ErrorKind::ConnectionAborted, ErrorKind::ConnectionReset] {
        assert!(judge_upstream(UpstreamObservation::Unverifiable(kind)).is_err());
    }
}

/// 観測のスレッドは、実際に届いたバイトを数える（陽性対照。0 と数え違えると回帰を見逃す）。
#[test]
fn observe_upstream_counts_delivered_bytes() {
    let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = up.local_addr().unwrap().port();
    let h = observe_upstream(up);
    let mut c = connect(port);
    c.write_all(b"abc").unwrap();
    drop(c);
    assert_eq!(h.join().unwrap(), UpstreamObservation::Received(3));
}

/// 遮断・解除は、accept のスレッドが上流への接続の直前で止まっていても待たされない。
/// `Applied` の記録から実際の遮断までに、上流への接続の上限（5 秒）に及ぶずれを作らない（ORST-17）。
/// 止まった接続は、遮断が解除された後に上流への接続が成立しても中継されず、client のバイトは上流へ届かない
/// （遮断の世代の検査。現在の遮断状態だけでは通らないよう、先に解除してから停止点を開ける）。
///
/// 上流への接続の直前に停止点（転送器が持つ同期点）を置き、到達の通知を待ってから `block()` を呼ぶ
/// （OS の待ち行列の挙動に依らず、停止点を同期で作る）。宛先の決定と接続は転送器が行う。
#[test]
fn block_returns_promptly_while_accept_is_stalled_on_upstream() {
    let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let upstream_port = up.local_addr().unwrap().port();
    // blocking の accept で待つ観測のスレッドを、転送器の起動の前に作る（待ち行列に残って切られる前に受ける）。
    up.set_nonblocking(false).unwrap();
    let observer = observe_upstream(up);
    let stall = UpstreamStall::new();
    let mut p =
        ExternalStoreFaultProxy::start_to_port_with_stall(upstream_port, stall.clone()).unwrap();
    let mut tl = timeline();
    let mut stuck = connect(p.listen_port());
    // 遮断の前に送る。遮断が無ければ、上流へ中継される値。
    stuck.write_all(b"s").unwrap();
    assert!(
        stall.wait_entered(Duration::from_secs(5)),
        "accept thread never reached the upstream stall point"
    );
    // 前提: 停止点で止まっており、拒否も転送も始まっていない。
    assert_eq!(p.refused_count(), 0, "connect finished before the block");
    assert_eq!(p.accepted_count(), 0, "conn was relayed before the block");
    let started = Instant::now();
    p.block(&mut tl).unwrap();
    let blocked_in = started.elapsed();
    // ORST-17: 遮断と停止点を維持した状態で、client が切られていることを確かめる（解除後の世代検査に頼らない）。
    assert_eq!(
        roundtrip(&mut stuck, b"t"),
        Outcome::Disconnected,
        "stalled conn is cut by the block"
    );
    // 世代の検査だけを対象にするため、現在の遮断状態を外してから停止点を開ける。
    let started = Instant::now();
    p.release_block(&mut tl).unwrap();
    let released_in = started.elapsed();
    stall.release();
    // 停止点が期限切れで抜けていないこと。期限切れなら遮断中の即時切断を見たことにならない（ORST-17・AUDB-6）。
    assert!(
        wait_until(|| stall.exit_reason().is_some()),
        "accept thread never left the stall point"
    );
    assert_eq!(
        stall.exit_reason(),
        Some(StallExit::Released),
        "stall point expired before the explicit release"
    );
    assert!(
        blocked_in < Duration::from_secs(1),
        "block waited for the stalled accept: {blocked_in:?}"
    );
    assert!(
        released_in < Duration::from_secs(1),
        "release waited for the stalled accept: {released_in:?}"
    );
    // 上流への接続が成立したこと（遮断前の接続の中継を拒む分岐に届いたこと）を確かめる。
    // 対象の接続について、上流への接続の成功と世代の不一致による拒否を、専用の計数で同期的に確かめる
    // （どちらの分岐でも。接続の試行数・拒否の合計では、接続の失敗や複製の失敗でも増えるため代わりにならない。ORST-17・AUDB-6）。
    assert!(
        wait_until(|| p.upstream_connected_count() == 1 && p.generation_refused_count() == 1),
        "expected 1 upstream connect and 1 generation refusal, got {} and {}",
        p.upstream_connected_count(),
        p.generation_refused_count()
    );
    // 遮断前のバイトが上流へ届いていないことを、上流側の受信バイト数で確かめる（ORST-17・AUDB-6）。
    // 観測は上で起動した専用のスレッド（blocking の accept）が行う。観測できない場合（accept・読み取りの失敗、
    // macOS の `ConnectionAborted`・`ConnectionReset` を含む）は合格にせず失敗にする。計数だけでは、
    // 世代の検査より前にバイトを送る回帰を検出できないため、どの OS でも同じ判定を通す。
    if let Err(why) = judge_upstream(observer.join().unwrap()) {
        panic!("{why}");
    }
    assert!(wait_until(|| p.refused_count() >= 1));
    assert_eq!(p.accepted_count(), 0, "stalled conn was relayed");
}

/// 停止点で止まったままでも `stop()` は戻る（停止の信号で停止点の待ちが解ける）。
#[test]
fn stop_returns_while_accept_is_held_at_stall_point() {
    let up = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    up.set_nonblocking(true).unwrap();
    let stall = UpstreamStall::new();
    let p = ExternalStoreFaultProxy::start_to_port_with_stall(
        up.local_addr().unwrap().port(),
        stall.clone(),
    )
    .unwrap();
    let _c = connect(p.listen_port());
    assert!(stall.wait_entered(Duration::from_secs(5)));
    let started = Instant::now();
    p.stop();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "stop waited for the stall point"
    );
    // 期限切れではなく、停止の通知で待ちが解けたこと。
    assert!(wait_until(|| stall.exit_reason().is_some()));
    assert_eq!(stall.exit_reason(), Some(StallExit::Stopped));
    // 停止後に上流へ接続していない（専用の上流リスナーに接続が届いていない。ORST-17）。
    assert_eq!(
        up.accept().map(|_| ()).unwrap_err().kind(),
        ErrorKind::WouldBlock,
        "stop() connected to the upstream"
    );
}

/// 上流の待ち行列を満杯にして、本物の上流への connect が固まる状態を作る。返す値は drop まで保つ。
/// 待ち行列があふれると SYN を捨てる挙動に依るので Linux だけに限る（`docs/harness/fault-injection.md` の OS の条件）。
#[cfg(target_os = "linux")]
struct StalledUpstream {
    port: u16,
    _listener: TcpListener,
    _filled: Vec<TcpStream>,
}

#[cfg(target_os = "linux")]
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

/// `/proc/net/tcp` に、宛先ポートが `port` で SYN_SENT（状態 02）の接続があるか。
/// 溜まりきった待ち行列への connect は SYN を落とされ、OS の中で SYN_SENT のまま待つ。
#[cfg(target_os = "linux")]
fn syn_sent_to(port: u16) -> bool {
    let want = format!(":{port:04X}");
    std::fs::read_to_string("/proc/net/tcp").is_ok_and(|t| {
        t.lines().skip(1).any(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            f.len() > 3 && f[2].ends_with(&want) && f[3] == "02"
        })
    })
}

/// ORST-17: 本物の上流への connect が固まっていても `block()` は待たされない（Linux のみ。検出を足す試験で、契約を変えない）。
/// 停止点の試験（全 OS）は OS の connect の 1 行を停止点の外に残す。この試験は本物の connect を詰まらせ、
/// その 1 行だけを `conns` のロックで囲む回帰を `block()` の 1 秒超えで検出する。
#[cfg(target_os = "linux")]
#[test]
fn block_returns_promptly_while_real_upstream_connect_is_stalled_linux() {
    let up = stalled_upstream();
    let mut p = ExternalStoreFaultProxy::start_to_port(up.port).unwrap();
    let mut tl = timeline();
    let mut stuck = connect(p.listen_port());
    // 転送器の connect が実際に固まっている（試行 1 回・成功 0 回・拒否 0 回）のを確かめてから `block()` を呼ぶ。
    assert!(
        wait_until(|| p.upstream_attempt_count() == 1
            && p.upstream_connected_count() == 0
            && p.refused_count() == 0),
        "the forwarder connect did not reach the stalled state"
    );
    // 計数は `connect_real` を呼ぶ前に増える。OS が SYN を送って待つ状態（SYN_SENT）になったことまで確かめる。
    // `conns` のロックで囲む回帰では、ロックを取った後でなければこの状態にならない（コードに同期点を足さない）。
    assert!(
        wait_until(|| syn_sent_to(up.port)),
        "no SYN_SENT socket toward the stalled upstream"
    );
    let started = Instant::now();
    p.block(&mut tl).unwrap();
    let elapsed = started.elapsed();
    // 回帰が無ければ connect はまだ SYN_SENT のまま。外れていたら観測が遅れて試験が空振りしたので、失敗に倒す。
    assert!(
        syn_sent_to(up.port),
        "the connect left SYN_SENT before block returned; the check did not exercise the stall"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "block waited for the stalled connect: {elapsed:?}"
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
