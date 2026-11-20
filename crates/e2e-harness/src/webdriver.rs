//! loopback 限定の最小の WebDriver クライアント（同期・HTTP/1.1・`Connection: close`）。
//!
//! 役割: [`crate::chrome`] が起動したドライバ（chromedriver）へ、W3C WebDriver と WebAuthn の拡張の命令を送る。
//! 外部の HTTP クライアント・非同期ランタイムを足さないための自前実装で、人の確認待ちの暫定（`docs/harness/e2e.md`）。
//! JSON は `serde_json` で扱う（2026-11-20 の利用者の決定）。
//!
//! 安全: 宛先は型で `127.0.0.1:<port>` に固定しホストを受けない・リダイレクトを追わない（A10）。path に埋める id は
//! 英数字と `-` だけ（A03）。応答は上限を超えたら・chunked なら・形が合わなければ誤りにする（fail-closed）。
//! 誤りに URL・ポート・path・応答本文を載せない（SECR-11・SECR-23）。

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::component::E2eComponentKind;
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;

/// 応答の大きさの上限。ハーネスの安全上の上限で、要件の値ではない。
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// 期限までの残り時間。尽きていたら誤り（0 は socket の timeout に渡せないため）。
fn remaining(deadline: Instant) -> Result<Duration, E2eError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(failed)
}

fn failed() -> E2eError {
    E2eError::new(E2eErrorKind::CommandFailed, E2eComponentKind::BrowserDriver)
}

/// path に埋める id。英数字と `-` だけを受ける（A03）。
fn validated_id(raw: &str) -> Result<String, E2eError> {
    let ok = !raw.is_empty()
        && raw.len() <= 128
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(raw.to_owned())
    } else {
        Err(failed())
    }
}

/// 要求のメソッド。列挙にして、任意の文字列を要求行へ埋め込めないようにする（A03）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpMethod {
    Get,
    Post,
    Delete,
}

impl HttpMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }
}

/// 要求行に埋める path の検査。`/` で始まり、可視の ASCII（0x21〜0x7e）だけを受ける。
/// 空白・制御文字・改行・非 ASCII は拒否する（要求行・ヘッダーへの注入を防ぐ。A03）。
fn validated_path(path: &str) -> Result<&str, E2eError> {
    let ok = path.starts_with('/') && path.bytes().all(|b| (0x21..=0x7e).contains(&b));
    if ok { Ok(path) } else { Err(failed()) }
}

/// `Debug` で値の代わりに出す固定の文字列（SECR-11）。
const REDACTED: &str = "<redacted>";

/// セッション ID。セッションを操作できる値のため、専用の型で持つ（SECR-10・SECR-30。`fandhe-auth-secret` へは
/// `scripts/check-crate-deps.sh` の許可表で辺が許されていないため、この crate の中に非公開の型を置く）。
///
/// `Debug` は固定の文字列だけ。`Display`・`Clone` 以外の暗黙の取り出し経路を持たず、値は要求の path を
/// 組み立てるときだけ [`SessionId::expose`] で借りる（SECR-11・SECR-16）。
#[derive(Clone)]
struct SessionId(String);

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SessionId").field(&REDACTED).finish()
    }
}

impl SessionId {
    fn new(raw: &str) -> Result<Self, E2eError> {
        validated_id(raw).map(Self)
    }

    /// 要求の path の組み立てだけで使う。戻り値をログ・誤りに渡さない。
    fn expose(&self) -> &str {
        &self.0
    }
}

/// 仮想 authenticator の識別子。ドライバが返した値を検証して持つ。
///
/// `Debug` は値を出さない（SECR-11。authenticator の操作に使う値のため）。
#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticatorId(String);

impl fmt::Debug for AuthenticatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AuthenticatorId").field(&REDACTED).finish()
    }
}

impl AuthenticatorId {
    /// 検証して作る。英数字と `-` 以外を含む値は拒否する。
    pub fn new(raw: &str) -> Result<Self, E2eError> {
        validated_id(raw).map(Self)
    }
}

/// 仮想 authenticator が保持する credential の要約。判定に要る欄だけを持ち、秘密鍵・user handle は保持しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSummary {
    /// RP ID。
    pub rp_id: String,
    /// resident credential か。
    pub is_resident: bool,
}

/// ドライバ 1 つへの接続先。ポートだけを持つ。
///
/// `Debug` はポートを出さない（SECR-11・SECR-23）。
#[derive(Clone, Copy)]
pub struct WebDriverClient {
    port: u16,
    timeout: Duration,
}

impl fmt::Debug for WebDriverClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDriverClient")
            .field("port", &REDACTED)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl WebDriverClient {
    /// loopback の `port` へ向ける。`timeout` は 1 命令（接続・書き込み・応答の読み取りの全体）の期限に使う。
    pub fn new(port: u16, timeout: Duration) -> Self {
        Self { port, timeout }
    }

    /// 1 命令を送り、応答の `value` を返す。WebDriver の誤り応答・非 2xx・形の不一致は `CommandFailed`。
    ///
    /// 非公開。公開の入口は定義済みの操作（`status`・`new_session` と [`WebDriverSession`] の各操作）だけで、
    /// 仮想 credential の秘密鍵を含み得る応答を生の `Value` として公開しない（A03・SECR-11）。
    fn request(
        &self,
        method: HttpMethod,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, E2eError> {
        let path = validated_path(path)?;
        let method = method.as_str();
        let payload = match body {
            Some(v) => serde_json::to_vec(v).map_err(|_| failed())?,
            None => Vec::new(),
        };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        // 命令全体の期限。少量ずつ返すドライバでも `timeout` を超えて続けない。
        // 期限を表せないほど大きい `timeout` は panic せず誤りにする。
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(failed)?;
        let mut stream =
            TcpStream::connect_timeout(&addr, remaining(deadline)?).map_err(|_| failed())?;
        let mut out = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.port,
            payload.len()
        )
        .into_bytes();
        out.extend_from_slice(&payload);
        // 部分書き込みごとに残り時間を設定し直し、書き込みも命令全体の期限で打ち切る。
        let mut sent = 0;
        while sent < out.len() {
            stream
                .set_write_timeout(Some(remaining(deadline)?))
                .map_err(|_| failed())?;
            match stream.write(&out[sent..]) {
                Ok(0) => return Err(failed()),
                Ok(n) => sent += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(failed()),
            }
        }
        parse_response(&mut stream, deadline)
    }

    /// `GET /status` の応答の `value`。
    pub fn status(&self) -> Result<Value, E2eError> {
        self.request(HttpMethod::Get, "/status", None)
    }

    /// `GET /status` の `ready` が真か。
    pub fn is_ready(&self) -> bool {
        self.request(HttpMethod::Get, "/status", None)
            .ok()
            .and_then(|v| v.get("ready").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// 新しいセッションを作る。成功したらセッションとブラウザの版を返す。
    pub fn new_session(
        &self,
        capabilities: &Value,
    ) -> Result<(WebDriverSession, String), E2eError> {
        let v = self.request(HttpMethod::Post, "/session", Some(capabilities))?;
        let id = v
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(failed)?;
        let version = v
            .pointer("/capabilities/browserVersion")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Ok((
            WebDriverSession {
                client: *self,
                id: SessionId::new(id)?,
            },
            version,
        ))
    }
}

/// 応答を読んで `value` を返す。Content-Length が分かればその分で止め、無ければ EOF まで読む。
fn parse_response(stream: &mut TcpStream, deadline: Instant) -> Result<Value, E2eError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end: Option<usize> = None;
    let mut content_length: Option<usize> = None;
    loop {
        if let (Some(end), Some(cl)) = (header_end, content_length)
            && buf.len() >= end + cl
        {
            break;
        }
        // read ごとに残り時間を設定し、命令全体を期限で打ち切る。
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|_| failed())?;
        let n = stream.read(&mut chunk).map_err(|_| failed())?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_RESPONSE_BYTES {
            return Err(failed());
        }
        if header_end.is_none()
            && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
        {
            header_end = Some(pos + 4);
            validate_head(&buf[..pos])?;
            content_length = content_length_of(&buf[..pos])?;
            // 上限を超える Content-Length は読む前に拒否する（end + cl の overflow も防ぐ）。
            if content_length.is_some_and(|cl| cl > MAX_RESPONSE_BYTES) {
                return Err(failed());
            }
        }
    }
    let end = header_end.ok_or_else(failed)?;
    let head = &buf[..end - 4];
    // chunked は読まない（fail-closed）。
    if header_value(head, "transfer-encoding").is_some() {
        return Err(failed());
    }
    let status = parse_status_line(head)?;
    let body = &buf[end..];
    let body = match content_length {
        Some(cl) if body.len() >= cl => &body[..cl],
        Some(_) => return Err(failed()),
        None => body,
    };
    let json: Value = serde_json::from_slice(body).map_err(|_| failed())?;
    let value = json.get("value").cloned().ok_or_else(failed)?;
    if !(200..300).contains(&status) || value.get("error").is_some_and(Value::is_string) {
        return Err(failed());
    }
    Ok(value)
}

/// ステータス行を `HTTP/1.1 NNN[ 理由]` の形に限って読む。形が違えば誤り（fail-closed）。
fn parse_status_line(head: &[u8]) -> Result<u16, E2eError> {
    let text = std::str::from_utf8(head).map_err(|_| failed())?;
    let line = text.split("\r\n").next().ok_or_else(failed)?;
    let rest = line.strip_prefix("HTTP/1.1 ").ok_or_else(failed)?;
    let code = rest.split(' ').next().unwrap_or("");
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(failed());
    }
    code.parse::<u16>().map_err(|_| failed())
}

/// ヘッダー部の構文を一度だけ検証する。行は CRLF だけで区切り（裸の CR・LF を含む行は誤り）、
/// 2 行目以降は名前が空でない `名前: 値` の形に限る。解釈できない行を読み飛ばして `Content-Length` を
/// 見落とすことがないようにする（fail-closed）。
fn validate_head(head: &[u8]) -> Result<(), E2eError> {
    let text = std::str::from_utf8(head).map_err(|_| failed())?;
    for (i, line) in text.split("\r\n").enumerate() {
        if line.contains(['\r', '\n']) {
            return Err(failed());
        }
        if i > 0 {
            match line.split_once(':') {
                Some((name, _)) if !name.trim().is_empty() => {}
                _ => return Err(failed()),
            }
        }
    }
    Ok(())
}

/// `Content-Length` を読む。複数あって値が食い違えば誤り。数字以外（`+`・`0x` 等）も誤り。
fn content_length_of(head: &[u8]) -> Result<Option<usize>, E2eError> {
    let mut found: Option<usize> = None;
    for v in header_values(head, "content-length") {
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(failed());
        }
        let n = v.parse::<usize>().map_err(|_| failed())?;
        match found {
            Some(prev) if prev != n => return Err(failed()),
            _ => found = Some(n),
        }
    }
    Ok(found)
}

fn header_values(head: &[u8], name: &str) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(head) else {
        return Vec::new();
    };
    text.split("\r\n")
        .skip(1)
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
        .collect()
}

fn header_value(head: &[u8], name: &str) -> Option<String> {
    let text = std::str::from_utf8(head).ok()?;
    text.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_owned())
    })
}

/// 確立したセッション。命令の path に session id を埋める操作をここに集める。
///
/// `Debug` はセッション ID とポートを出さない（SECR-11・SECR-23。セッションを操作できる値のため）。
#[derive(Clone)]
pub struct WebDriverSession {
    client: WebDriverClient,
    id: SessionId,
}

impl fmt::Debug for WebDriverSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDriverSession")
            .field("client", &self.client)
            .field("id", &REDACTED)
            .finish()
    }
}

impl WebDriverSession {
    fn path(&self, tail: &str) -> String {
        format!("/session/{}{tail}", self.id.expose())
    }

    /// セッションを閉じる。
    pub fn delete(&self) -> Result<(), E2eError> {
        self.client
            .request(
                HttpMethod::Delete,
                &format!("/session/{}", self.id.expose()),
                None,
            )
            .map(|_| ())
    }

    /// URL へ移動する。
    pub fn navigate(&self, url: &str) -> Result<(), E2eError> {
        self.client
            .request(
                HttpMethod::Post,
                &self.path("/url"),
                Some(&json!({ "url": url })),
            )
            .map(|_| ())
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        let ms = u64::try_from(timeout.as_millis()).map_err(|_| failed())?;
        self.client
            .request(
                HttpMethod::Post,
                &self.path("/timeouts"),
                Some(&json!({ "script": ms })),
            )
            .map(|_| ())
    }

    /// 非同期スクリプトを実行する。スクリプトは試験の定数で、外部入力を連結しない。
    /// 応答は生の `Value` のため crate の外へ公開しない（呼ぶのは `register_once` 等の定義済みの手順）。
    pub(crate) fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
        self.client.request(
            HttpMethod::Post,
            &self.path("/execute/async"),
            Some(&json!({ "script": script, "args": args })),
        )
    }

    /// 仮想 authenticator を足す。
    pub fn add_virtual_authenticator(
        &self,
        options: &VirtualAuthenticatorOptions,
    ) -> Result<AuthenticatorId, E2eError> {
        let v = self.client.request(
            HttpMethod::Post,
            &self.path("/webauthn/authenticator"),
            Some(&options.to_webdriver_json()),
        )?;
        AuthenticatorId::new(v.as_str().ok_or_else(failed)?)
    }

    /// 仮想 authenticator の credential の要約を返す。秘密鍵の欄は読み捨てる。
    pub fn credentials(&self, id: &AuthenticatorId) -> Result<Vec<CredentialSummary>, E2eError> {
        let v = self.client.request(
            HttpMethod::Get,
            &self.path(&format!("/webauthn/authenticator/{}/credentials", id.0)),
            None,
        )?;
        v.as_array()
            .ok_or_else(failed)?
            .iter()
            .map(|c| {
                Ok(CredentialSummary {
                    rp_id: c
                        .get("rpId")
                        .and_then(Value::as_str)
                        .ok_or_else(failed)?
                        .to_owned(),
                    is_resident: c
                        .get("isResidentCredential")
                        .and_then(Value::as_bool)
                        .ok_or_else(failed)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn session_id_debug_is_redacted_and_value_is_borrowed_only_by_expose() {
        // SECR-11・SECR-30: 秘密値として持つ型。derive(Debug) の入れ物に入れても値が出ない。
        #[derive(Debug)]
        struct Holder {
            _id: SessionId,
        }
        let id = SessionId::new("sid-visible-9").unwrap();
        assert_eq!(id.expose(), "sid-visible-9");
        for shown in [
            format!("{id:?}"),
            format!("{id:#?}"),
            format!("{:?}", Holder { _id: id.clone() }),
        ] {
            assert!(!shown.contains("sid-visible-9"), "{shown}");
            assert!(shown.contains("<redacted>"), "{shown}");
        }
        assert!(SessionId::new("a/b").is_err());
    }

    #[test]
    fn unsafe_paths_are_rejected_before_anything_is_sent() {
        // A03: 空白・制御文字・改行を含む path は、接続する前に拒否する（要求行・ヘッダーへの注入を防ぐ）。
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let c = WebDriverClient::new(port, Duration::from_secs(1));
        for bad in [
            "",
            "status",
            "/a b",
            "/a\r\nX-Injected: 1",
            "/a\nb",
            "/a\tb",
            "/a\0b",
            "/a\x7fb",
            "/日本語",
        ] {
            assert!(c.request(HttpMethod::Get, bad, None).is_err(), "{bad:?}");
        }
        assert!(l.accept().is_err(), "接続が来てはならない");
    }
}
