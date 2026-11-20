//! Chrome の仮想 authenticator で passkey の登録 1 件を通す E2E（`#[ignore]`。`make e2e-chrome` で流す。CI 外）。
//!
//! 前提: chromedriver と Chrome が手元にある。無ければ失敗する（黙って飛ばさない。DPOL-30〜32）。
//! 結果は〔仮想〕で、実機・Safari に数えない（ACPT-7・ACPT-8）。サーバー側の検証は `TASK-716` 待ちで、PKEY の確定を主張しない。

#![cfg(unix)]

mod support;

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fandhe_auth_e2e_harness::{
    Browser, ChromeDriverComponent, ChromeDriverConfig, E2eConfig, E2eHarness, PasskeyOperation,
    Protocol, Transport, VirtualAuthenticatorOptions, register_once,
};
use support::TestOrigin;

#[test]
#[ignore = "chromedriver と Chrome が要る。make e2e-chrome で流す"]
fn registers_one_passkey_with_chrome_virtual_authenticator() {
    let origin = TestOrigin::start();
    let chrome = ChromeDriverComponent::new(
        ChromeDriverConfig::new("chromedriver")
            .with_allowed_origin(&origin.origin())
            .expect("origin"),
    );
    let handle = chrome.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Registration, Browser::Chrome).with(Box::new(chrome)),
    )
    .expect("start");

    // 試験の値。PoC の値（QP-1）であり要件の値ではない。
    let aid = handle
        .add_virtual_authenticator(&VirtualAuthenticatorOptions {
            protocol: Protocol::Ctap2,
            transport: Transport::Internal,
            has_resident_key: true,
            has_user_verification: true,
            is_user_verified: true,
        })
        .expect("authenticator");
    handle.navigate(&origin.origin()).expect("navigate");
    // script timeout は command timeout（既定 30 秒）より短くし、時間切れを WebDriver の
    // script timeout として観測できるようにする。
    handle
        .set_script_timeout(Duration::from_secs(20))
        .expect("timeout");

    let outcome = register_once(&handle, &origin.origin(), "localhost").expect("register");
    assert!(outcome.succeeded(), "error={:?}", outcome.error());
    assert!(!outcome.evidence().counts_as_real_device());
    assert!(!outcome.evidence().counts_as_safari());

    let creds = handle.credentials(&aid).expect("credentials");
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].rp_id, "localhost");

    running.stop().expect("stop");
}

/// 接続を 1 件受けるたびに 204 を返して数える待ち受け（対照用。応答しないとページの load が終わらない）。
/// スコープの終了（`Drop`）で受け付けのスレッドを止めて join し、ポートを残さない（`TestOrigin` と同じ）。
struct CountingServer {
    count: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    port: u16,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CountingServer {
    fn start(listener: TcpListener) -> Self {
        let port = listener.local_addr().expect("addr").port();
        let count = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (c, st) = (Arc::clone(&count), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut s) = conn else { return };
                c.fetch_add(1, Ordering::SeqCst);
                let _ = s.write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        Self {
            count,
            stop,
            port,
            thread: Some(thread),
        }
    }

    fn hits(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl Drop for CountingServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // accept を起こしてループを抜ける。
        let _ = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// 待ち受けを nonblocking にし、`wait` の間に接続が 1 件でも来たか。
fn connection_arrives(listener: &TcpListener, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if listener.accept().is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
#[ignore = "chromedriver と Chrome が要る。make e2e-chrome で流す"]
fn chrome_never_connects_to_a_loopback_port_that_is_not_registered() {
    // A10: 登録外のループバックのポートへ、ページのサブリソースが実際に届かないことを受信側で確かめる。
    // 対照: 登録したループバックのポートへのサブリソースは届く（受信側の観測が働いていることの確認）。
    let forbidden = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    forbidden.set_nonblocking(true).expect("nonblocking");
    let allowed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let fport = forbidden.local_addr().expect("addr").port();
    let aport = allowed.local_addr().expect("addr").port();
    let allowed_server = CountingServer::start(allowed);

    let page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>e2e</title></head><body>\
            <img src=\"http://localhost:{fport}/x\"><img src=\"http://localhost:{aport}/x\"></body></html>"
    );
    let origin = TestOrigin::start_with_body(&page);
    let chrome = ChromeDriverComponent::new(
        ChromeDriverConfig::new("chromedriver")
            .with_allowed_origin(&origin.origin())
            .expect("origin")
            .with_allowed_origin(&format!("http://localhost:{aport}"))
            .expect("origin"),
    );
    let handle = chrome.handle();
    let running = E2eHarness::start(
        E2eConfig::new(PasskeyOperation::Registration, Browser::Chrome).with(Box::new(chrome)),
    )
    .expect("start");
    handle.navigate(&origin.origin()).expect("navigate");

    assert!(
        allowed_server.hits() > 0,
        "control: the registered port must receive the subresource request"
    );
    assert!(
        !connection_arrives(&forbidden, Duration::from_secs(1)),
        "the unregistered loopback port must receive no connection"
    );
    running.stop().expect("stop");
}
