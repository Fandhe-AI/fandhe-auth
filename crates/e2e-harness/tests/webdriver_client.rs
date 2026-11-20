//! WebDriver クライアントの試験。偽のドライバ（loopback の定型応答）に対して流す。chromedriver の構成要素の試験は
//! `chrome_component.rs`（unix 限定）。
//!
//! 偽のドライバは通信の形を確かめるためのもので、E2E の合格に数えない（ACPT-7）。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fandhe_auth_e2e_harness::{
    AuthenticatorId, E2eErrorKind, Protocol, Transport, VirtualAuthenticatorOptions,
    WebDriverClient,
};
use serde_json::{Value, json};

/// `POST /session` の本文の代わり（偽のドライバは中身を見ない）。
fn caps() -> Value {
    json!({ "capabilities": {} })
}

/// 1 接続だけ受け、受けた要求（先頭行と本文）を返し、`response` をそのまま書く。
fn fake_driver(response: Vec<u8>) -> (u16, JoinHandle<()>, mpsc::Receiver<String>) {
    fake_driver_seq(vec![response])
}

/// 接続ごとに `responses` を順に返す（接続数は応答の数）。
fn fake_driver_seq(responses: Vec<Vec<u8>>) -> (u16, JoinHandle<()>, mpsc::Receiver<String>) {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let h = std::thread::spawn(move || {
        for response in responses {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                buf.extend_from_slice(&chunk[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                    let cl = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buf.len() >= p + 4 + cl || n == 0 {
                        break;
                    }
                } else if n == 0 {
                    break;
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
            let _ = s.write_all(&response);
        }
    });
    (port, h, rx)
}

fn http(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn client(port: u16) -> WebDriverClient {
    WebDriverClient::new(port, Duration::from_secs(5))
}

#[test]
fn huge_content_length_is_rejected_without_panic() {
    // 上限超過・usize::MAX の Content-Length は overflow せず CommandFailed にする（fail-closed）。
    for cl in ["18446744073709551615", "99999999"] {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {cl}\r\nConnection: close\r\n\r\n{{\"value\":{{}}}}"
        )
        .into_bytes();
        let (port, h, _rx) = fake_driver(resp);
        let err = client(port).request("GET", "/status", None).unwrap_err();
        assert_eq!(err.kind(), E2eErrorKind::CommandFailed);
        h.join().unwrap();
    }
}

#[test]
fn request_returns_value_on_success() {
    let (port, h, rx) = fake_driver(http("200 OK", r#"{"value":{"ready":true}}"#));
    let v = client(port).request("GET", "/status", None).unwrap();
    assert_eq!(v["ready"], true);
    let req = rx.recv().unwrap();
    assert!(req.starts_with("GET /status HTTP/1.1"));
    assert!(req.contains("Connection: close"));
    h.join().unwrap();
}

#[test]
fn webdriver_error_response_is_command_failed() {
    let (port, h, _) = fake_driver(http(
        "404 Not Found",
        r#"{"value":{"error":"no such window","message":"x"}}"#,
    ));
    let e = client(port).request("GET", "/x", None).unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::CommandFailed);
    h.join().unwrap();
}

#[test]
fn error_value_with_2xx_status_is_command_failed() {
    let (port, h, _) = fake_driver(http("200 OK", r#"{"value":{"error":"e"}}"#));
    assert!(client(port).request("GET", "/x", None).is_err());
    h.join().unwrap();
}

#[test]
fn chunked_response_is_rejected() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\n{}\r\n0\r\n\r\n".to_vec();
    let (port, h, _) = fake_driver(raw);
    assert!(client(port).request("GET", "/x", None).is_err());
    h.join().unwrap();
}

#[test]
fn oversize_response_is_rejected() {
    let big = "a".repeat(1024 * 1024 + 10);
    let (port, h, _) = fake_driver(http("200 OK", &format!(r#"{{"value":"{big}"}}"#)));
    assert!(client(port).request("GET", "/x", None).is_err());
    let _ = h.join();
}

#[test]
fn broken_json_is_rejected() {
    let (port, h, _) = fake_driver(http("200 OK", "{not json"));
    assert!(client(port).request("GET", "/x", None).is_err());
    h.join().unwrap();
}

#[test]
fn error_display_has_no_url_port_or_path() {
    let (port, h, _) = fake_driver(http(
        "500 Internal Server Error",
        r#"{"value":{"error":"e"}}"#,
    ));
    let e = client(port)
        .request("GET", "/secret-path", None)
        .unwrap_err();
    let shown = format!("{e} {e:?}");
    assert!(!shown.contains(&port.to_string()));
    assert!(!shown.contains("secret-path"));
    assert!(!shown.contains("127.0.0.1"));
    h.join().unwrap();
}

#[test]
fn session_commands_send_the_five_keys_and_parse_credentials() {
    // 順に: new_session・add_virtual_authenticator・credentials。
    let (port, h, rx) = fake_driver_seq(vec![
        http(
            "200 OK",
            r#"{"value":{"sessionId":"sid-1","capabilities":{"browserVersion":"1.2.3"}}}"#,
        ),
        http("200 OK", r#"{"value":"abc-123"}"#),
        http(
            "200 OK",
            r#"{"value":[{"credentialId":"x","isResidentCredential":false,"rpId":"localhost","privateKey":"p","signCount":0}]}"#,
        ),
    ]);
    let (sess, ver) = client(port).new_session(&caps()).unwrap();
    assert_eq!(ver, "1.2.3");
    let aid = sess.add_virtual_authenticator(&opts()).unwrap();
    let creds = sess.credentials(&aid).unwrap();
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].rp_id, "localhost");
    assert!(!creds[0].is_resident);
    let reqs: Vec<String> = rx.iter().take(3).collect();
    assert!(reqs[0].starts_with("POST /session HTTP/1.1"));
    assert!(reqs[1].starts_with("POST /session/sid-1/webauthn/authenticator HTTP/1.1"));
    for k in [
        "\"protocol\":\"ctap2\"",
        "\"transport\":\"internal\"",
        "\"hasResidentKey\":true",
        "\"hasUserVerification\":true",
        "\"isUserVerified\":true",
    ] {
        assert!(reqs[1].contains(k), "{k}");
    }
    assert!(
        reqs[2]
            .starts_with("GET /session/sid-1/webauthn/authenticator/abc-123/credentials HTTP/1.1")
    );
    h.join().unwrap();
}

#[test]
fn session_id_with_path_characters_is_rejected() {
    let (port, h, _) = fake_driver(http("200 OK", r#"{"value":{"sessionId":"a/../b"}}"#));
    assert!(client(port).new_session(&caps()).is_err());
    h.join().unwrap();
}

fn opts() -> VirtualAuthenticatorOptions {
    // 試験の値。PoC の値（QP-1）であり要件の値ではない。
    VirtualAuthenticatorOptions {
        protocol: Protocol::Ctap2,
        transport: Transport::Internal,
        has_resident_key: true,
        has_user_verification: true,
        is_user_verified: true,
    }
}

#[test]
fn ids_with_path_characters_are_rejected() {
    for bad in ["", "a/b", "a b", "../x", "a?b", "a\r\nb"] {
        assert!(AuthenticatorId::new(bad).is_err(), "{bad:?}");
    }
    assert!(AuthenticatorId::new("ok-1").is_ok());
}

#[test]
fn slow_driver_is_cut_off_by_the_whole_command_deadline() {
    // 前提: 偽のドライバが応答を 1 バイトずつ 100ms 間隔で返し続ける。
    // 操作: 期限 500ms で 1 命令を送る。期待: read ごとの timeout ではなく命令全体の期限で CommandFailed になる。
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n";
        let _ = s.write_all(head);
        for _ in 0..50 {
            if s.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let started = Instant::now();
    let e = WebDriverClient::new(port, Duration::from_millis(500))
        .request("GET", "/status", None)
        .unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::CommandFailed);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let _ = h.join();
}

#[test]
fn unrepresentable_timeout_is_an_error_not_a_panic() {
    // 期限を表せない timeout は panic せず CommandFailed にする。
    let e = WebDriverClient::new(1, Duration::MAX)
        .request("GET", "/status", None)
        .unwrap_err();
    assert_eq!(e.kind(), E2eErrorKind::CommandFailed);
}

#[test]
fn debug_output_has_no_session_id_authenticator_id_or_port() {
    // SECR-11: Debug にセッション ID・authenticator の ID・ポートを出さない。
    let (port, h, _) = fake_driver_seq(vec![
        http(
            "200 OK",
            r#"{"value":{"sessionId":"sid-visible-1","capabilities":{"browserVersion":"1"}}}"#,
        ),
        http("200 OK", r#"{"value":"aid-visible-2"}"#),
    ]);
    let c = client(port);
    let (sess, _) = c.new_session(&caps()).unwrap();
    let aid = sess.add_virtual_authenticator(&opts()).unwrap();
    for shown in [
        format!("{sess:?}"),
        format!("{sess:#?}"),
        format!("{c:?}"),
        format!("{aid:?}"),
    ] {
        assert!(!shown.contains("sid-visible-1"), "{shown}");
        assert!(!shown.contains("aid-visible-2"), "{shown}");
        assert!(!shown.contains(&port.to_string()), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
    }
    h.join().unwrap();
}
