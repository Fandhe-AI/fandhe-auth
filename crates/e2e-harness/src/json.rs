//! 最小の JSON（値の型・直列化・厳格な構文解析）。外部依存を足さないための自前実装。
//!
//! 役割: WebDriver の要求の組み立てと、応答の解析だけに使う。構文が正しくない入力・深さの超過・重複キー・
//! 対でないサロゲートは誤りにする（fail-closed）。誤りに入力の断片を載せない（SECR-11・SECR-23）。
//! 資格情報の応答は [`Parser::credentials`] が必要な欄だけを読み、他の欄は値を保持せずに捨てる（SECR-10・SECR-29）。

use std::ops::Index;

/// 入れ子の深さの上限。ハーネスの安全上の値で、要件の値ではない。
const MAX_DEPTH: usize = 32;

/// 解析の失敗。理由・入力の断片は持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonError;

/// JSON の値。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `null`。
    Null,
    /// 真偽。
    Bool(bool),
    /// 負でない整数。
    Uint(u64),
    /// 上以外の数。
    Number(f64),
    /// 文字列。
    Str(String),
    /// 配列。
    Array(Vec<Value>),
    /// オブジェクト（キーは重複しない）。
    Object(Vec<(String, Value)>),
}

static NULL: Value = Value::Null;

impl Value {
    /// オブジェクトを作る。
    pub fn object<const N: usize>(pairs: [(&str, Value); N]) -> Self {
        Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// 配列を作る。
    pub fn array(items: impl IntoIterator<Item = Value>) -> Self {
        Value::Array(items.into_iter().collect())
    }

    /// オブジェクトの値。無ければ `None`。
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 文字列なら中身。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// 真偽なら中身。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// 負でない整数なら中身。
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Uint(n) => Some(*n),
            _ => None,
        }
    }

    /// 配列なら中身。
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// 文字列か。
    pub fn is_string(&self) -> bool {
        matches!(self, Value::Str(_))
    }

    /// 直列化する。
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        self.write_to(&mut out);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Uint(n) => out.push_str(&n.to_string()),
            Value::Number(f) if f.is_finite() => out.push_str(&f.to_string()),
            Value::Number(_) => out.push_str("null"),
            Value::Str(s) => write_string(s, out),
            Value::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_to(out);
                }
                out.push(']');
            }
            Value::Object(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write_to(out);
                }
                out.push('}');
            }
        }
    }

    /// 構文を厳格に検証して解析する。
    pub fn parse(body: &[u8]) -> Result<Value, JsonError> {
        let mut p = Parser::new(body)?;
        let v = p.value()?;
        p.finish()?;
        Ok(v)
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

impl Index<usize> for Value {
    type Output = Value;
    fn index(&self, i: usize) -> &Value {
        match self {
            Value::Array(a) => a.get(i).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Uint(n)
    }
}

/// 資格情報の応答から読む欄だけを持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFields {
    /// `rpId`。
    pub rp_id: String,
    /// `isResidentCredential`。
    pub is_resident: bool,
}

/// 厳格な構文解析器。入力全体を UTF-8 として検証してから読む。
pub struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(body: &'a [u8]) -> Result<Self, JsonError> {
        std::str::from_utf8(body).map_err(|_| JsonError)?;
        Ok(Self {
            b: body,
            i: 0,
            depth: 0,
        })
    }

    /// 資格情報の応答 `{"value":[{...},...]}` から、各要素の `rpId`・`isResidentCredential` だけを読む。
    /// それ以外の欄（`privateKey`・`userHandle` 等）は構文だけを確かめて値を保持せずに捨てる（SECR-10・SECR-29）。
    /// キー名は JSON のエスケープを解いてから照合する（`\u0072pId` も `rpId` と読む）。
    pub fn credentials(body: &[u8]) -> Result<Vec<CredentialFields>, JsonError> {
        let mut p = Parser::new(body)?;
        p.ws();
        p.expect(b'{')?;
        p.enter()?;
        let mut out: Option<Vec<CredentialFields>> = None;
        let mut seen: Vec<String> = Vec::new();
        p.ws();
        if p.peek() != Some(b'}') {
            loop {
                p.ws();
                let key = p.string()?;
                dedup(&mut seen, &key)?;
                p.ws();
                p.expect(b':')?;
                p.ws();
                if key == "value" {
                    out = Some(p.credential_array()?);
                } else {
                    p.skip_value()?;
                }
                p.ws();
                match p.next()? {
                    b',' => continue,
                    b'}' => break,
                    _ => return Err(JsonError),
                }
            }
        } else {
            p.i += 1;
        }
        p.leave();
        p.finish()?;
        out.ok_or(JsonError)
    }

    fn credential_array(&mut self) -> Result<Vec<CredentialFields>, JsonError> {
        self.expect(b'[')?;
        self.enter()?;
        let mut list = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            self.leave();
            return Ok(list);
        }
        loop {
            self.ws();
            list.push(self.credential_object()?);
            self.ws();
            match self.next()? {
                b',' => continue,
                b']' => break,
                _ => return Err(JsonError),
            }
        }
        self.leave();
        Ok(list)
    }

    fn credential_object(&mut self) -> Result<CredentialFields, JsonError> {
        self.expect(b'{')?;
        self.enter()?;
        let mut rp_id: Option<String> = None;
        let mut resident: Option<bool> = None;
        let mut seen: Vec<String> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            return Err(JsonError);
        }
        loop {
            self.ws();
            let key = self.string()?;
            dedup(&mut seen, &key)?;
            self.ws();
            self.expect(b':')?;
            self.ws();
            match key.as_str() {
                "rpId" => rp_id = Some(self.string()?),
                "isResidentCredential" => resident = Some(self.boolean()?),
                _ => self.skip_value()?,
            }
            self.ws();
            match self.next()? {
                b',' => continue,
                b'}' => break,
                _ => return Err(JsonError),
            }
        }
        self.leave();
        Ok(CredentialFields {
            rp_id: rp_id.ok_or(JsonError)?,
            is_resident: resident.ok_or(JsonError)?,
        })
    }

    fn finish(&mut self) -> Result<(), JsonError> {
        self.ws();
        if self.i == self.b.len() {
            Ok(())
        } else {
            Err(JsonError)
        }
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(JsonError)
        } else {
            Ok(())
        }
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn next(&mut self) -> Result<u8, JsonError> {
        let c = self.peek().ok_or(JsonError)?;
        self.i += 1;
        Ok(c)
    }

    fn expect(&mut self, c: u8) -> Result<(), JsonError> {
        if self.next()? == c {
            Ok(())
        } else {
            Err(JsonError)
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), JsonError> {
        if self.b.get(self.i..self.i + word.len()) == Some(word) {
            self.i += word.len();
            Ok(())
        } else {
            Err(JsonError)
        }
    }

    fn boolean(&mut self) -> Result<bool, JsonError> {
        match self.peek() {
            Some(b't') => self.literal(b"true").map(|()| true),
            Some(b'f') => self.literal(b"false").map(|()| false),
            _ => Err(JsonError),
        }
    }

    fn value(&mut self) -> Result<Value, JsonError> {
        self.ws();
        match self.peek().ok_or(JsonError)? {
            b'n' => self.literal(b"null").map(|()| Value::Null),
            b't' | b'f' => self.boolean().map(Value::Bool),
            b'"' => self.string().map(Value::Str),
            b'[' => {
                self.i += 1;
                self.enter()?;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                } else {
                    loop {
                        a.push(self.value()?);
                        self.ws();
                        match self.next()? {
                            b',' => continue,
                            b']' => break,
                            _ => return Err(JsonError),
                        }
                    }
                }
                self.leave();
                Ok(Value::Array(a))
            }
            b'{' => {
                self.i += 1;
                self.enter()?;
                let mut m: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        let k = self.string()?;
                        if m.iter().any(|(e, _)| *e == k) {
                            return Err(JsonError);
                        }
                        self.ws();
                        self.expect(b':')?;
                        let v = self.value()?;
                        m.push((k, v));
                        self.ws();
                        match self.next()? {
                            b',' => continue,
                            b'}' => break,
                            _ => return Err(JsonError),
                        }
                    }
                }
                self.leave();
                Ok(Value::Object(m))
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(JsonError),
        }
    }

    /// 値を読み捨てる。構文は `value` と同じに確かめ、文字列・数は保持しない。
    fn skip_value(&mut self) -> Result<(), JsonError> {
        self.ws();
        match self.peek().ok_or(JsonError)? {
            b'n' => self.literal(b"null"),
            b't' | b'f' => self.boolean().map(|_| ()),
            b'"' => self.string_inner(None),
            b'[' => {
                self.i += 1;
                self.enter()?;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                } else {
                    loop {
                        self.skip_value()?;
                        self.ws();
                        match self.next()? {
                            b',' => continue,
                            b']' => break,
                            _ => return Err(JsonError),
                        }
                    }
                }
                self.leave();
                Ok(())
            }
            b'{' => {
                self.i += 1;
                self.enter()?;
                let mut seen: Vec<String> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        let k = self.string()?;
                        dedup(&mut seen, &k)?;
                        self.ws();
                        self.expect(b':')?;
                        self.skip_value()?;
                        self.ws();
                        match self.next()? {
                            b',' => continue,
                            b'}' => break,
                            _ => return Err(JsonError),
                        }
                    }
                }
                self.leave();
                Ok(())
            }
            b'-' | b'0'..=b'9' => self.number().map(|_| ()),
            _ => Err(JsonError),
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(JsonError),
        }
        let mut integer = self.b[start] != b'-';
        if self.peek() == Some(b'.') {
            integer = false;
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError);
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError);
            }
            self.digits();
        }
        let text = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| JsonError)?;
        if integer && let Ok(n) = text.parse::<u64>() {
            return Ok(Value::Uint(n));
        }
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| JsonError)
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        let mut s = String::new();
        self.string_inner(Some(&mut s))?;
        Ok(s)
    }

    /// 文字列を読む。`out` が `None` なら内容を保持せずに構文だけを確かめる。
    fn string_inner(&mut self, mut out: Option<&mut String>) -> Result<(), JsonError> {
        self.expect(b'"')?;
        loop {
            let c = self.next()?;
            match c {
                b'"' => return Ok(()),
                b'\\' => {
                    let ch = match self.next()? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return Err(JsonError),
                    };
                    if let Some(o) = out.as_deref_mut() {
                        o.push(ch);
                    }
                }
                0..=0x1f => return Err(JsonError),
                _ if c < 0x80 => {
                    if let Some(o) = out.as_deref_mut() {
                        o.push(char::from(c));
                    }
                }
                _ => {
                    // 入力全体を UTF-8 と検証済み。多バイト文字は先頭バイトの長さ分をまとめて扱う。
                    let len = match c {
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    let end = self.i - 1 + len;
                    let chunk = self.b.get(self.i - 1..end).ok_or(JsonError)?;
                    let text = std::str::from_utf8(chunk).map_err(|_| JsonError)?;
                    if let Some(o) = out.as_deref_mut() {
                        o.push_str(text);
                    }
                    self.i = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut n = 0u32;
        for _ in 0..4 {
            let d = char::from(self.next()?).to_digit(16).ok_or(JsonError)?;
            n = n * 16 + d;
        }
        Ok(n)
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let hi = self.hex4()?;
        if (0xd800..0xdc00).contains(&hi) {
            // 上位サロゲートの直後には下位サロゲートの `\u` が要る。
            self.expect(b'\\')?;
            self.expect(b'u')?;
            let lo = self.hex4()?;
            if !(0xdc00..0xe000).contains(&lo) {
                return Err(JsonError);
            }
            char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)).ok_or(JsonError)
        } else {
            char::from_u32(hi).ok_or(JsonError)
        }
    }
}

fn dedup(seen: &mut Vec<String>, key: &str) -> Result<(), JsonError> {
    if seen.iter().any(|k| k == key) {
        return Err(JsonError);
    }
    seen.push(key.to_owned());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_request_shape() {
        let v = Value::object([
            (
                "a",
                Value::array([Value::from("x\"y\n"), Value::from(3u64)]),
            ),
            ("b", Value::from(true)),
        ]);
        let s = v.to_json_string();
        assert_eq!(Value::parse(s.as_bytes()).unwrap(), v);
    }

    #[test]
    fn parser_is_strict() {
        for bad in [
            "",
            "{",
            "{\"a\":1,}",
            "[1,]",
            "{\"a\":1}x",
            "01",
            "1.",
            "\"\\x\"",
            "\"\\ud800\"",
            "{\"a\":1,\"a\":2}",
            "\"a\nb\"",
            "nul",
        ] {
            assert!(Value::parse(bad.as_bytes()).is_err(), "{bad}");
        }
        assert!(Value::parse(&[b'"', 0xff, b'"']).is_err());
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert!(Value::parse(deep.as_bytes()).is_err());
    }

    #[test]
    fn parses_escapes_and_numbers() {
        let v =
            Value::parse(r#"{"k":"\u0041\ud83d\ude00é","n":12,"f":-1.5e1,"z":null}"#.as_bytes())
                .unwrap();
        assert_eq!(v["k"].as_str(), Some("A\u{1f600}é"));
        assert_eq!(v["n"].as_u64(), Some(12));
        assert_eq!(v["f"], Value::Number(-15.0));
        assert_eq!(v["z"], Value::Null);
    }

    #[test]
    fn credentials_reads_only_two_fields_even_with_escaped_keys() {
        let body = br#"{"value":[{"rpId":"localhost","isResidentCredential":true,"\u0070rivateKey":"SECRET","userHandle":null,"nested":{"x":[1,2]}}],"extra":1}"#;
        let c = Parser::credentials(body).unwrap();
        assert_eq!(
            c,
            vec![CredentialFields {
                rp_id: "localhost".to_owned(),
                is_resident: true
            }]
        );
        // 欄の欠け・重複・型違い・構文違反は誤り。
        for bad in [
            r#"{"value":[{"rpId":"a"}]}"#,
            r#"{"value":[{"rpId":"a","isResidentCredential":true,"rpId":"b"}]}"#,
            r#"{"value":[{"rpId":1,"isResidentCredential":true}]}"#,
            r#"{"value":{"error":"x"}}"#,
            r#"{"value":[{"rpId":"a","isResidentCredential":true,"k":"\q"}]}"#,
            r#"{"other":[]}"#,
        ] {
            assert!(Parser::credentials(bad.as_bytes()).is_err(), "{bad}");
        }
        assert_eq!(Parser::credentials(br#"{"value":[]}"#).unwrap(), vec![]);
    }
}
