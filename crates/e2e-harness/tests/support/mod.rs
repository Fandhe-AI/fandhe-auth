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
                    // 枠を取ってからスレッドを作る。上限を超えた接続はスレッドを作らず閉じる。
                    // 投機的な preconnect が受け付けを塞がないよう、枠の中では接続ごとに別スレッドで処理する。
                    let Some(slot) = Slot::acquire() else {
                        continue;
                    };
                    std::thread::spawn(move || handle(s, port, slot));
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

/// ヘッダー部の上限。ハーネスの安全上の値で、要件の値ではない。
const MAX_HEADER_BYTES: usize = 16 * 1024;
/// 1 接続がヘッダーを送り終えるまでの期限（スローロリス対策。試験用の安い上限）。
const HEADER_DEADLINE: Duration = Duration::from_secs(2);
/// 同時に処理する接続数の上限。超えた接続は黙って閉じる。
const MAX_CONNECTIONS: usize = 64;

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// 接続数の計上。`Drop` で必ず戻す。
struct Slot;

impl Slot {
    fn acquire() -> Option<Slot> {
        if ACTIVE.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// ヘッダー部（`\r\n\r\n` の前）だけを見て、`Host` がちょうど 1 つで `expected` と等しいか。
/// 終端が無い・`Host` が 0 件か 2 件以上・値が違う場合は false（DNS rebinding 対策）。
fn host_is_exact(head: &str, expected: &str) -> bool {
    let mut hosts = head.split("\r\n").skip(1).filter_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("host").then(|| v.trim())
    });
    matches!((hosts.next(), hosts.next()), (Some(h), None) if h == expected)
}

fn handle(mut s: TcpStream, port: u16, _slot: Slot) {
    let deadline = Instant::now() + HEADER_DEADLINE;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    // ヘッダーの終端が上限の中に無ければ受理しない。期限は接続全体で数える。
    let end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        else {
            return;
        };
        let _ = s.set_read_timeout(Some(left));
        if buf.len() >= MAX_HEADER_BYTES {
            reject(&mut s);
            return;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => {
                // 終端なしで閉じられた・止まった要求は受理しない。
                reject(&mut s);
                return;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    if end > MAX_HEADER_BYTES {
        reject(&mut s);
        return;
    }
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    let request_line = text.split("\r\n").next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let host_ok = host_is_exact(&text, &format!("localhost:{port}"));
    let (status, body) = if !host_ok {
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

fn reject(s: &mut TcpStream) {
    let _ =
        s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}
