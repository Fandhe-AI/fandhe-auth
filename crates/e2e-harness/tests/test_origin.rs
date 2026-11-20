//! 試験用 origin の試験。前提・操作・期待の形。試験用ページは本番の画面ではない（TASK-11）。

mod support;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};

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

#[test]
fn stalled_connections_are_cut_by_deadline_and_server_recovers() {
    // 前提: ヘッダーを途中までしか送らない接続を上限より多く開いたまま保つ。
    // 期待: 絶対期限で切られ、期限後は正常な要求に応える（接続数・処理時間が無制限ではない）。
    let o = TestOrigin::start();
    let stalled: Vec<TcpStream> = (0..20)
        .map(|_| {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap();
            s.write_all(b"GET / HTTP/1.1\r\n").unwrap();
            s
        })
        .collect();
    std::thread::sleep(support::CONNECTION_DEADLINE + std::time::Duration::from_millis(700));
    let r = get(o.port(), &format!("localhost:{}", o.port()), "GET", "/");
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    drop(stalled);
}
