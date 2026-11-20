//! 試験用 origin の試験。前提・操作・期待の形。試験用ページは本番の画面ではない（TASK-11）。

mod support;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

use support::TestOrigin;

fn get(port: u16, host: &str, method: &str, path: &str) -> String {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn serves_root_with_no_store_and_no_script() {
    // 前提: 起動済み。操作: 正しい Host で GET /。期待: 200・no-store・script を含まない。
    let o = TestOrigin::start();
    let r = get(o.port(), &format!("localhost:{}", o.port()), "GET", "/");
    assert!(r.starts_with("HTTP/1.1 200"));
    assert!(r.contains("Cache-Control: no-store"));
    assert!(!r.to_ascii_lowercase().contains("<script"));
}

#[test]
fn rejects_wrong_host_other_path_and_method() {
    let o = TestOrigin::start();
    let host = format!("localhost:{}", o.port());
    assert!(get(o.port(), "evil.example", "GET", "/").starts_with("HTTP/1.1 400"));
    assert!(get(o.port(), &host, "GET", "/x").starts_with("HTTP/1.1 404"));
    assert!(get(o.port(), &host, "POST", "/").starts_with("HTTP/1.1 405"));
}

#[test]
fn origin_is_localhost_with_port() {
    let o = TestOrigin::start();
    assert_eq!(o.origin(), format!("http://localhost:{}", o.port()));
}

/// サーバー側が接続を閉じたか（EOF か reset）。`wait` の間に何も起きなければ `false`。
fn closed_by_server(s: &mut TcpStream, wait: Duration) -> bool {
    s.set_read_timeout(Some(wait)).unwrap();
    let mut b = [0u8; 16];
    match s.read(&mut b) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

#[test]
fn connections_over_the_cap_are_closed_by_the_server_immediately() {
    // 前提: ヘッダーを途中までしか送らない接続を上限より多く開き、保つ。
    // 期待: 上限を超えた分はサーバーが期限を待たずに閉じる（クライアント側の送信の成否ではなく切断を観測する）。
    let o = TestOrigin::start();
    let total = support::MAX_CONNECTIONS + 12;
    let mut conns: Vec<TcpStream> = (0..total)
        .map(|_| {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
            s.write_all(b"GET / HTTP/1.1\r\n").unwrap();
            s
        })
        .collect();
    // 超えた分（後ろから開いた分）は、期限（2 秒）よりずっと前に切られている。
    let (held, over) = conns.split_at_mut(support::MAX_CONNECTIONS);
    for s in over {
        assert!(closed_by_server(s, Duration::from_millis(300)));
    }
    // 上限内の分は、期限までは切られない。
    for s in held {
        assert!(!closed_by_server(s, Duration::from_millis(20)));
    }
}

#[test]
fn stalled_connection_is_cut_by_deadline_and_server_recovers() {
    // 前提: ヘッダーを途中までしか送らない 1 接続（上限には達しない）。
    // 期待: 絶対期限の後にサーバーが切断し、そのあと正常な要求に応える。
    let o = TestOrigin::start();
    let mut stalled = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
    stalled.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    assert!(
        !closed_by_server(&mut stalled, Duration::from_millis(300)),
        "期限の前に切られた"
    );
    assert!(closed_by_server(
        &mut stalled,
        support::CONNECTION_DEADLINE + Duration::from_millis(700)
    ));
    let r = get(o.port(), &format!("localhost:{}", o.port()), "GET", "/");
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
}

#[test]
fn request_without_header_terminator_or_with_duplicate_host_gets_no_page() {
    // ヘッダーの終端が無い要求には応えず、Host が 2 件なら 400。
    let o = TestOrigin::start();
    let host = format!("localhost:{}", o.port());
    let r = {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
        write!(s, "GET / HTTP/1.1\r\nHost: {host}\r\n").unwrap();
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    assert!(r.is_empty(), "{r}");
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
    write!(
        s,
        "GET / HTTP/1.1\r\nHost: {host}\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 400"), "{out}");
    // 本文に Host らしい行があっても、ヘッダー部の Host だけを見る。
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
    write!(
        s,
        "GET / HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\nHost: {host}\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 400"), "{out}");
}
