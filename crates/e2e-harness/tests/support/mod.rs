//! 試験コードの中だけの試験用ページの配信（TASK-11、`docs/harness/e2e.md` の候補 (b)。人の確認待ち）。
//!
//! 本番の画面ではない。loopback にだけ bind し、`GET /` に最小の HTML（スクリプト・外部資源なし）を返す。
//! `Host` が `localhost:<port>` 以外は 400（DNS rebinding 対策）。origin は試験用の仮の値（`A-2026-10-28-1`〜`14`）。
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 同時に処理する接続数の上限。超えた接続は何も返さず閉じる（A03・A10）。
const MAX_CONNECTIONS: usize = 8;
/// 1 接続の処理全体（読み取りと書き込み）の絶対期限。
pub const CONNECTION_DEADLINE: Duration = Duration::from_secs(2);

/// 処理中の接続数を数え、drop で戻す。
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

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
        let active = Arc::new(AtomicUsize::new(0));
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    // 投機的な preconnect が受け付けを塞がないよう、接続ごとに別スレッドで処理する。
                    // ただし同時数に上限を置き、超えた接続は閉じる。
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let slot = Slot(Arc::clone(&active));
                    std::thread::spawn(move || {
                        let _slot = slot;
                        handle(s, port);
                    });
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
    let deadline = Instant::now() + CONNECTION_DEADLINE;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        // read ごとに残り時間を取り直し、少量ずつ送る相手でも絶対期限で打ち切る。
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        else {
            return;
        };
        let _ = s.set_read_timeout(Some(left));
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
    if let Some(left) = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        let _ = s.set_write_timeout(Some(left));
        let _ = s.write_all(resp.as_bytes());
    }
}
