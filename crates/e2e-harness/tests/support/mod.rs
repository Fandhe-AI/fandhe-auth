//! 試験コードの中だけの試験用ページの配信（TASK-11、`docs/harness/e2e.md` の候補 (b)。人の確認待ち）。
//!
//! 本番の画面ではない。loopback にだけ bind し、`GET /` に最小の HTML（スクリプト・外部資源なし）を返す。
//! `Host` が `localhost:<port>` 以外は 400（DNS rebinding 対策）。origin は試験用の仮の値（`A-2026-10-28-1`〜`14`）。
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct TestOrigin {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestOrigin {
    pub fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    handle(s, port);
                }
            }
        });
        Self {
            port,
            stop,
            thread: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://localhost:<port>`。
    pub fn origin(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

impl Drop for TestOrigin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // accept を起こしてループを抜ける。
        let _ = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn handle(mut s: TcpStream, port: u16) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let host = lines
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("host").then(|| v.trim())
        })
        .unwrap_or("");
    let (status, body) = if host != format!("localhost:{port}") {
        ("400 Bad Request", "")
    } else if method != "GET" {
        ("405 Method Not Allowed", "")
    } else if path != "/" {
        ("404 Not Found", "")
    } else {
        (
            "200 OK",
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>e2e</title></head><body>e2e</body></html>",
        )
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(resp.as_bytes());
}
