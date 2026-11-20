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

/// 生のバイト列を送り、書き込み側を閉じて、応答を読む。
fn raw(port: u16, bytes: &[u8]) -> String {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.write_all(bytes).unwrap();
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn host_in_the_body_is_not_accepted() {
    // 前提: ヘッダーの Host が無く、終端より後ろ（本文）に正しい Host の行がある。期待: 400。
    let o = TestOrigin::start();
    let req = format!(
        "GET / HTTP/1.1\r\nConnection: close\r\n\r\nHost: localhost:{}\r\n",
        o.port()
    );
    assert!(raw(o.port(), req.as_bytes()).starts_with("HTTP/1.1 400"));
}

#[test]
fn request_without_header_terminator_is_rejected() {
    // 前提: 終端（空行）が上限の中に無い。期待: 正しい Host があっても 400（受理しない）。
    let o = TestOrigin::start();
    let req = format!("GET / HTTP/1.1\r\nHost: localhost:{}\r\n", o.port());
    assert!(raw(o.port(), req.as_bytes()).starts_with("HTTP/1.1 400"));
    // 上限を超えるヘッダーで終端が無い場合も 400。
    let mut big = format!("GET / HTTP/1.1\r\nHost: localhost:{}\r\n", o.port()).into_bytes();
    big.extend_from_slice(&vec![b'a'; 20 * 1024]);
    assert!(raw(o.port(), &big).starts_with("HTTP/1.1 400"));
}

#[test]
fn duplicate_or_conflicting_host_headers_are_rejected() {
    let o = TestOrigin::start();
    let good = format!("localhost:{}", o.port());
    for hosts in [
        vec![good.as_str(), "evil.example"],
        vec!["evil.example", good.as_str()],
        vec![good.as_str(), good.as_str()],
    ] {
        let mut req = String::from("GET / HTTP/1.1\r\n");
        for h in &hosts {
            req.push_str(&format!("Host: {h}\r\n"));
        }
        req.push_str("Connection: close\r\n\r\n");
        assert!(
            raw(o.port(), req.as_bytes()).starts_with("HTTP/1.1 400"),
            "{hosts:?}"
        );
    }
}

#[test]
fn exactly_one_correct_host_header_is_accepted() {
    let o = TestOrigin::start();
    let req = format!(
        "GET / HTTP/1.1\r\nhost:   localhost:{}\r\nConnection: close\r\n\r\n",
        o.port()
    );
    assert!(raw(o.port(), req.as_bytes()).starts_with("HTTP/1.1 200"));
}

#[test]
fn stalled_connections_do_not_block_a_following_request() {
    // 前提: 何も送らず開きっぱなしの接続が複数ある。期待: 続く正しい要求は応じられる（接続ごとの別スレッド）。
    let o = TestOrigin::start();
    let _held: Vec<TcpStream> = (0..8)
        .map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, o.port())).unwrap())
        .collect();
    let r = get(o.port(), &format!("localhost:{}", o.port()), "GET", "/");
    assert!(r.starts_with("HTTP/1.1 200"));
}

/// ヘッダー名の大文字小文字を無視して値を返す（応答のヘッダー部だけを見る）。
fn header<'a>(resp: &'a str, name: &str) -> Option<&'a str> {
    let head = resp.split("\r\n\r\n").next()?;
    head.split("\r\n").skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[test]
fn page_carries_the_safety_headers() {
    // CSRF-28・CSRF-31・CSRF-32・CSRF-51: 試験用ページにも CSP・X-Frame-Options・nosniff・Referrer-Policy を付ける。
    let o = TestOrigin::start();
    let r = get(o.port(), &format!("localhost:{}", o.port()), "GET", "/");
    assert!(r.starts_with("HTTP/1.1 200"));
    let csp = header(&r, "content-security-policy").expect("CSP");
    let directives: Vec<&str> = csp.split(';').map(str::trim).collect();
    for d in [
        "default-src 'none'",
        "script-src 'self'",
        "base-uri 'none'",
        "frame-ancestors 'none'",
        "form-action 'self'",
    ] {
        assert!(directives.contains(&d), "{d} in {csp}");
    }
    // `unsafe-inline`・`unsafe-eval`・ワイルドカードを足さない。
    assert!(!csp.contains("unsafe"), "{csp}");
    assert!(!csp.contains('*'), "{csp}");
    assert_eq!(header(&r, "x-frame-options"), Some("DENY"));
    assert_eq!(header(&r, "x-content-type-options"), Some("nosniff"));
    assert_eq!(header(&r, "referrer-policy"), Some("no-referrer"));
}
