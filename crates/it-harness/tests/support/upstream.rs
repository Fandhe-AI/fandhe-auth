//! 試験用の上流のサーバー（`fault_db.rs`・`fault_baseline.rs` が `#[path]` で読む。`tests/` 直下ではないので試験の対象にならない）。
//! 127.0.0.1 の空きポートだけで待ち受ける。外向きの通信・環境変数は使わない（DPOL-32）。

#![allow(dead_code)]

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// 上流の各接続の書き込みの打ち切り（試験用の仮の値。要件の値ではない）。
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// 上流のサーバー。drop で待ち受けと接続のスレッドを止めて回収する（試験の後にスレッドとポートを残さない）。
pub struct Upstream {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Upstream {
    /// 受けた接続ごとに `conn` を別スレッドで走らせる。`conn` は `stop` が立つか相手が閉じたら戻ること。
    pub fn serve(conn: impl Fn(TcpStream, &AtomicBool) + Send + Sync + 'static) -> Self {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let conn = Arc::new(conn);
        let handle = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop2.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        // 書き込みに打ち切りを置く。相手が読まなくても `write_all` が戻り、`drop` の join に期限ができる（#129）。
                        let _ = s.set_write_timeout(Some(WRITE_TIMEOUT));
                        let (stop3, conn) = (Arc::clone(&stop2), Arc::clone(&conn));
                        workers.push(thread::spawn(move || conn(s, &stop3)));
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for w in workers {
                let _ = w.join();
            }
        });
        Self {
            port,
            stop,
            handle: Some(handle),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// echo の上流。
pub fn echo_upstream() -> Upstream {
    Upstream::serve(|mut s, stop| {
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
    })
}
