//! 試験コードの中だけの試験用ページの配信（TASK-11、`docs/harness/e2e.md` の候補 (b)。人の確認待ち）。
//!
//! 本番の画面ではない。loopback にだけ bind し、`GET /` に最小の HTML（スクリプト・外部資源なし）を返す。
//! ヘッダーの終端が無い要求には応えない。`Host` は `localhost:<port>` の 1 件だけを受け、それ以外は 400（DNS rebinding 対策）。origin は試験用の仮の値（`A-2026-10-28-1`〜`14`）。
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 同時に処理する接続数の上限。超えた接続は何も返さず閉じる（A03・A10）。
pub const MAX_CONNECTIONS: usize = 8;
/// ヘッダー部（終端の空行を含む）の上限。
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
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
        Self::start_with_body(DEFAULT_BODY)
    }

    /// `GET /` に返す HTML を差し替えて起動する（試験用の定数の HTML だけを渡す。外部入力を連結しない）。
    pub fn start_with_body(body: &str) -> Self {
        let body: Arc<str> = Arc::from(body);
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
                    let body = Arc::clone(&body);
                    std::thread::spawn(move || {
                        let _slot = slot;
                        handle(s, port, &body);
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

const DEFAULT_BODY: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>e2e</title></head><body>e2e</body></html>";

fn handle(mut s: TcpStream, port: u16, page: &str) {
    let deadline = Instant::now() + CONNECTION_DEADLINE;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_HEADER_BYTES {
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
    // ヘッダーの終端（空行）が上限内に無い要求には応えない。Host はヘッダー部だけから探す。
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return;
    };
    // 終端の 4 バイトを含めて上限内に収まること（読み取りの最後の塊で上限を超えた終端は受理しない）。
    if end + 4 > MAX_HEADER_BYTES {
        return;
    }
    let text = String::from_utf8_lossy(&buf[..end]);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split(' ').collect();
    let well_formed = parts.len() == 3 && parts[2] == "HTTP/1.1" && !parts[0].is_empty();
    let (method, path) = (parts[0], parts.get(1).copied().unwrap_or(""));
    // Host は正しい形式でちょうど 1 件のときだけ受ける。
    let mut hosts = Vec::new();
    let mut headers_ok = true;
    for l in lines {
        match l.split_once(':') {
            Some((k, v)) if !k.is_empty() && !k.contains([' ', '\t']) => {
                if k.eq_ignore_ascii_case("host") {
                    hosts.push(v.trim_matches([' ', '\t']));
                }
            }
            _ => headers_ok = false,
        }
    }
    let host_ok = hosts.len() == 1 && hosts[0] == format!("localhost:{port}");
    let (status, body) = if !well_formed || !headers_ok || !host_ok {
        ("400 Bad Request", "")
    } else if method != "GET" {
        ("405 Method Not Allowed", "")
    } else if path != "/" {
        ("404 Not Found", "")
    } else {
        ("200 OK", page)
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
