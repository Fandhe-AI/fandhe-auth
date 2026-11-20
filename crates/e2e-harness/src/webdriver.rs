//! loopback 限定の最小の WebDriver クライアント（同期・HTTP/1.1・`Connection: close`）。
//!
//! 役割: [`crate::chrome`] が起動したドライバ（chromedriver）へ、W3C WebDriver と WebAuthn の拡張の命令を送る。
//! 外部の HTTP クライアント・非同期ランタイムを足さないための自前実装で、人の確認待ちの暫定（`docs/harness/e2e.md`）。
//!
//! 安全: 宛先は型で `127.0.0.1:<port>` に固定しホストを受けない・リダイレクトを追わない（A10）。メソッドは列挙型、
//! path は許可文字だけ、path に埋める id は英数字と `-` だけ（A03）。応答はステータス行・ヘッダーの構文が正しくない・
//! 長さが曖昧（重複・不正な Content-Length、chunked）・上限超過なら誤りにする（fail-closed）。
//! 誤りに URL・ポート・path・応答本文を載せない（SECR-11・SECR-23）。
//! 余分なバイトの拒否は、宣言した長さに届いた後の猶予（50ms と残り時間の小さい方）内に届いた分と、同じ受信に含まれた分までが保証の範囲
//! （接続は 1 命令で捨てる）。
//! 生の応答（`privateKey` を含み得る）を返す入口は crate 内に閉じ、外へは型付きの値だけを出す（SECR-10・SECR-11）。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::component::E2eComponentKind;
use crate::harness::{E2eError, E2eErrorKind};
use crate::virtual_authenticator::VirtualAuthenticatorOptions;

/// 応答の大きさの上限。ハーネスの安全上の上限で、要件の値ではない。
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// 宣言した長さに届いたあと、余分なバイトを待つ猶予。ハーネスの安全上の値で、要件の値ではない。
const EXTRA_BYTES_GRACE: Duration = Duration::from_millis(50);

/// 現在から `timeout` 後の期限。加算が溢れる大きな値は誤りにする（panic しない）。
pub(crate) fn deadline_after(timeout: Duration) -> Result<Instant, E2eError> {
    Instant::now().checked_add(timeout).ok_or_else(failed)
}

/// HTTP メソッド。使うものだけを列挙し、任意の文字列をリクエスト行に入れない（A03）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Delete => "DELETE",
        }
    }
}

/// path に許す文字。`/` 始まりで、英数字・`/`・`-`・`_` だけ（A03。空白・CR・LF・`?`・`#`・`..` 相当の `.` を含めない）。
fn validated_path(path: &str) -> Result<&str, E2eError> {
    let ok = path.starts_with('/')
        && path.len() <= 512
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_'));
    if ok { Ok(path) } else { Err(failed()) }
}

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

/// ドライバ 1 つへの接続先。ポートだけを持つ。crate 内でだけ使う。
#[derive(Debug, Clone, Copy)]
pub(crate) struct WebDriverClient {
    port: u16,
    timeout: Duration,
}

/// 期限までに `bytes` をすべて書く。部分書き込みのたびに残り時間を取り直す。
fn write_all_deadline(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> Result<(), E2eError> {
    while !bytes.is_empty() {
        stream
            .set_write_timeout(Some(remaining(deadline)?))
            .map_err(|_| failed())?;
        match stream.write(bytes) {
            Ok(0) => return Err(failed()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(failed()),
        }
    }
    Ok(())
}

impl WebDriverClient {
    /// loopback の `port` へ向ける。`timeout` は 1 命令（接続・書き込み・応答の読み取りの全体）の期限に使う。
    pub(crate) fn new(port: u16, timeout: Duration) -> Self {
        Self { port, timeout }
    }

    /// 1 命令を送り、応答の `value` を返す。WebDriver の誤り応答・非 2xx・形の不一致は `CommandFailed`。
    pub(crate) fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, E2eError> {
        let path = validated_path(path)?;
        let payload = match body {
            Some(v) => serde_json::to_vec(v).map_err(|_| failed())?,
            None => Vec::new(),
        };
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        // 命令全体の期限。少量ずつ返す・受け取るドライバでも `timeout` を超えて続けない。
        let deadline = deadline_after(self.timeout)?;
        let mut stream =
            TcpStream::connect_timeout(&addr, remaining(deadline)?).map_err(|_| failed())?;
        let mut message = format!(
            "{} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            method.as_str(),
            self.port,
            payload.len()
        )
        .into_bytes();
        message.extend_from_slice(&payload);
        write_all_deadline(&mut stream, &message, deadline)?;
        parse_response(&mut stream, deadline)
    }

    /// `GET /status` の `ready` が真か。
    pub(crate) fn is_ready(&self) -> bool {
        self.request(Method::Get, "/status", None)
            .ok()
            .and_then(|v| v.get("ready").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// 新しいセッションを作る。成功したらセッションとブラウザの版を返す。
    pub(crate) fn new_session(
        &self,
        capabilities: &Value,
    ) -> Result<(WebDriverSession, String), E2eError> {
        let v = self.request(Method::Post, "/session", Some(capabilities))?;
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

/// 応答の先頭部（ステータス行とヘッダー）の検証結果。
struct Head {
    status: u16,
    content_length: Option<usize>,
}

/// RFC 9110 の token（ヘッダー名に使える文字だけの 1 文字以上の並び）。
fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// HTAB 以外の制御文字（0x00〜0x1f・0x7f）を含むか。
fn has_control_char(s: &str) -> bool {
    s.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
}

/// ステータス行とヘッダーを厳格に読む。構文が正しくない・長さが曖昧（Content-Length の重複・不正、
/// Transfer-Encoding）なものは誤りにする（fail-closed。リクエスト・応答の smuggling 対策）。
fn parse_head(head: &[u8]) -> Result<Head, E2eError> {
    let text = std::str::from_utf8(head).map_err(|_| failed())?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or_else(failed)?;
    // `HTTP/1.x SP 3 桁 [SP reason]`。
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().ok_or_else(failed)?;
    let code = parts.next().ok_or_else(failed)?;
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || code.len() != 3
        || !code.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(failed());
    }
    let status = code.parse::<u16>().map_err(|_| failed())?;
    // 理由句は空でもよいが、制御文字（CR・LF・NUL 等）は含めない。
    if parts.next().is_some_and(has_control_char) {
        return Err(failed());
    }
    let mut content_length: Option<usize> = None;
    for line in lines {
        // 折り返し（obs-fold）・裸の CR/LF・`:` 無しは拒否する。
        if line.starts_with([' ', '\t']) || line.contains(['\r', '\n']) {
            return Err(failed());
        }
        let (name, value) = line.split_once(':').ok_or_else(failed)?;
        // 名前は RFC 9110 の token、値は制御文字（HTAB を除く）を含まない。
        if !is_token(name) || has_control_char(value) {
            return Err(failed());
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(failed());
        }
        if name.eq_ignore_ascii_case("content-length") {
            let v = value.trim_matches([' ', '\t']);
            if content_length.is_some() || v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(failed());
            }
            content_length = Some(v.parse::<usize>().map_err(|_| failed())?);
        }
    }
    Ok(Head {
        status,
        content_length,
    })
}

/// 応答を読んで `value` を返す。Content-Length が分かればその分で止め、無ければ EOF まで読む。
fn parse_response(stream: &mut TcpStream, deadline: Instant) -> Result<Value, E2eError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut head: Option<(usize, Head)> = None;
    let mut eof = false;
    loop {
        if let Some((end, h)) = &head
            && let Some(cl) = h.content_length
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
            eof = true;
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_RESPONSE_BYTES {
            return Err(failed());
        }
        if head.is_none()
            && let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n")
        {
            let parsed = parse_head(&buf[..pos])?;
            // 上限を超える Content-Length は読む前に拒否する（end + cl の overflow も防ぐ）。
            if parsed
                .content_length
                .is_some_and(|cl| cl > MAX_RESPONSE_BYTES)
            {
                return Err(failed());
            }
            head = Some((pos + 4, parsed));
        }
    }
    let (end, head) = head.ok_or_else(failed)?;
    // 宣言した長さに届いた時点で読み終えている場合、続きの余分なバイトを短い猶予で 1 回だけ確かめる。
    // 接続は 1 命令で捨てる（`Connection: close`）ので、猶予内に届かないバイトは見ない（保証は猶予内の到着まで）。
    if head.content_length.is_some() && !eof {
        // 確かめる窓は固定の猶予と残り時間の小さい方（絶対期限を超えない）。期限が尽きていたら確かめず、
        // 本文が届き切った完全な応答は成功にする（保証は期限内の到着まで）。
        if let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        {
            stream
                .set_read_timeout(Some(EXTRA_BYTES_GRACE.min(left)))
                .map_err(|_| failed())?;
            if matches!(stream.read(&mut chunk), Ok(n) if n > 0) {
                return Err(failed());
            }
        }
    }
    let body = &buf[end..];
    let body = match head.content_length {
        // 宣言より多い・少ない本文は曖昧なフレーミングとして拒否する。
        Some(cl) if body.len() == cl => body,
        Some(_) => return Err(failed()),
        None => body,
    };
    let json: Value = serde_json::from_slice(body).map_err(|_| failed())?;
    let value = json.get("value").cloned().ok_or_else(failed)?;
    if !(200..300).contains(&head.status) || value.get("error").is_some_and(Value::is_string) {
        return Err(failed());
    }
    Ok(value)
}

/// 確立したセッション。命令の path に session id を埋める操作をここに集める。
#[derive(Debug, Clone)]
pub(crate) struct WebDriverSession {
    client: WebDriverClient,
    id: String,
}

impl WebDriverSession {
    fn path(&self, tail: &str) -> String {
        format!("/session/{}{tail}", self.id)
    }

    /// セッションを閉じる。
    pub(crate) fn delete(&self) -> Result<(), E2eError> {
        self.client
            .request(Method::Delete, &format!("/session/{}", self.id), None)
            .map(|_| ())
    }

    /// URL へ移動する。
    pub(crate) fn navigate(&self, url: &str) -> Result<(), E2eError> {
        self.client
            .request(
                Method::Post,
                &self.path("/url"),
                Some(&json!({ "url": url })),
            )
            .map(|_| ())
    }

    /// 現在の URL を返す（移動後のリダイレクト先の確認用）。
    pub(crate) fn current_url(&self) -> Result<String, E2eError> {
        let v = self.client.request(Method::Get, &self.path("/url"), None)?;
        v.as_str().map(str::to_owned).ok_or_else(failed)
    }

    /// 非同期スクリプトの時間切れを設定する。
    pub(crate) fn set_script_timeout(&self, timeout: Duration) -> Result<(), E2eError> {
        let ms = u64::try_from(timeout.as_millis()).map_err(|_| failed())?;
        self.client
            .request(
                Method::Post,
                &self.path("/timeouts"),
                Some(&json!({ "script": ms })),
            )
            .map(|_| ())
    }

    /// 非同期スクリプトを実行する。スクリプトは試験の定数で、外部入力を連結しない。
    pub(crate) fn execute_async(&self, script: &str, args: &[Value]) -> Result<Value, E2eError> {
        self.client.request(
            Method::Post,
            &self.path("/execute/async"),
            Some(&json!({ "script": script, "args": args })),
        )
    }

    /// 仮想 authenticator を足す。
    pub(crate) fn add_virtual_authenticator(
        &self,
        options: &VirtualAuthenticatorOptions,
    ) -> Result<AuthenticatorId, E2eError> {
        let v = self.client.request(
            Method::Post,
            &self.path("/webauthn/authenticator"),
            Some(&options.to_webdriver_json()),
        )?;
        AuthenticatorId::new(v.as_str().ok_or_else(failed)?)
    }

    /// 仮想 authenticator の credential の要約を返す。秘密鍵の欄は読み捨てる。
    pub(crate) fn credentials(
        &self,
        id: &AuthenticatorId,
    ) -> Result<Vec<CredentialSummary>, E2eError> {
        let v = self.client.request(
            Method::Get,
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
mod tests;
