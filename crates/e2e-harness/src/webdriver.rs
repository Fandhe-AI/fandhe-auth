//! loopback 限定の最小の WebDriver クライアント（同期・HTTP/1.1・`Connection: close`）。
//!
//! 役割: [`crate::chrome`] が起動したドライバ（chromedriver）へ、W3C WebDriver と WebAuthn の拡張の命令を送る。
//! 外部の HTTP クライアント・非同期ランタイムを足さないための自前実装で、人の確認待ちの暫定（`docs/harness/e2e.md`）。
//!
//! 安全: 宛先は型で `127.0.0.1:<port>` に固定しホストを受けない・リダイレクトを追わない（A10）。path に埋める id は
//! 英数字と `-` だけ（A03）。応答は上限を超えたら・chunked なら・形が合わなければ誤りにする（fail-closed）。
//! 誤りに URL・ポート・path・応答本文を載せない（SECR-11・SECR-23）。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

use crate::component::E2eComponentKind;
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;

/// 応答の大きさの上限。ハーネスの安全上の上限で、要件の値ではない。
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

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

/// 仮想 authenticator の識別子。ドライバが返した値を検証して持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatorId(String);

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
#[derive(Debug, Clone, Copy)]
pub struct WebDriverClient {
    port: u16,
    timeout: Duration,
}

impl WebDriverClient {
    /// loopback の `port` へ向ける。`timeout` は接続・読み書きのそれぞれに掛ける。
    pub fn new(port: u16, timeout: Duration) -> Self {
        Self { port, timeout }
    }

    /// 1 命令を送り、応答の `value` を返す。WebDriver の誤り応答・非 2xx・形の不一致は `CommandFailed`。
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, E2eError> {
        let payload = match body {
            Some(v) => serde_json::to_vec(v).map_err(|_| failed())?,
            None => Vec::new(),
        };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut stream = TcpStream::connect_timeout(&addr, self.timeout).map_err(|_| failed())?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|_| failed())?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|_| failed())?;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.port,
            payload.len()
        );
        stream.write_all(head.as_bytes()).map_err(|_| failed())?;
        stream.write_all(&payload).map_err(|_| failed())?;
        parse_response(&mut stream)
    }

    /// `GET /status` の `ready` が真か。
    pub fn is_ready(&self) -> bool {
        self.request("GET", "/status", None)
            .ok()
            .and_then(|v| v.get("ready").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// 新しいセッションを作る。成功したらセッションとブラウザの版を返す。
    pub fn new_session(
        &self,
        capabilities: &Value,
    ) -> Result<(WebDriverSession, String), E2eError> {
        let v = self.request("POST", "/session", Some(capabilities))?;
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
                id: validated_id(id)?,
            },
            version,
        ))
    }
}

/// 応答を読んで `value` を返す。Content-Length が分かればその分で止め、無ければ EOF まで読む。
fn parse_response(stream: &mut TcpStream) -> Result<Value, E2eError> {
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
            content_length = header_value(&buf[..pos], "content-length")
                .map(|v| v.parse::<usize>().map_err(|_| failed()))
                .transpose()?;
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
    let status = std::str::from_utf8(head)
        .ok()
        .and_then(|h| h.lines().next())
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(failed)?;
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
#[derive(Debug, Clone)]
pub struct WebDriverSession {
    client: WebDriverClient,
    id: String,
}

impl WebDriverSession {
    fn path(&self, tail: &str) -> String {
        format!("/session/{}{tail}", self.id)
    }

    /// セッションを閉じる。
    pub fn delete(&self) -> Result<(), E2eError> {
        self.client
            .request("DELETE", &format!("/session/{}", self.id), None)
            .map(|_| ())
    }

    /// URL へ移動する。
    pub fn navigate(&self, url: &str) -> Result<(), E2eError> {
        self.client
            .request("POST", &self.path("/url"), Some(&json!({ "url": url })))
            .map(|_| ())
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        let ms = u64::try_from(timeout.as_millis()).map_err(|_| failed())?;
        self.client
            .request(
                "POST",
                &self.path("/timeouts"),
                Some(&json!({ "script": ms })),
            )
            .map(|_| ())
    }

    /// 非同期スクリプトを実行する。スクリプトは試験の定数で、外部入力を連結しない。
    pub fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
        self.client.request(
            "POST",
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
            "POST",
            &self.path("/webauthn/authenticator"),
            Some(&options.to_webdriver_json()),
        )?;
        AuthenticatorId::new(v.as_str().ok_or_else(failed)?)
    }

    /// 仮想 authenticator の credential の要約を返す。秘密鍵の欄は読み捨てる。
    pub fn credentials(&self, id: &AuthenticatorId) -> Result<Vec<CredentialSummary>, E2eError> {
        let v = self.client.request(
            "GET",
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
