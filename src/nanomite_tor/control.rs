//! Tor control-port protocol (`control-spec.txt`).
//!
//! A typed client over a [`ControlTransport`]: command construction, reply
//! framing (multiline `250-`, data blocks `250+`, errors `5xx`), asynchronous
//! `650` events, protocol negotiation and the high-level commands the rest of
//! the module needs.
//!
//! Every reply is treated as untrusted text: parsers are total and malformed
//! input becomes a typed error, never a panic.

use crate::nanomite_tor::bootstrap::{self, BootstrapStatus};
use crate::nanomite_tor::circuit::{self, Circuit, Stream};
use crate::nanomite_tor::onion::{OnionRequest, OnionResponse};
use crate::nanomite_tor::{TorError, TorResult};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

/// How to authenticate to the control port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlAuth {
    /// `AUTHENTICATE` with no credential.
    None,
    /// `AUTHENTICATE "<password>"` (HashedControlPassword).
    Password(String),
    /// `AUTHENTICATE <hex>` using the contents of `control_auth_cookie`.
    CookieHex(String),
}

impl ControlAuth {
    /// Read `control_auth_cookie` and build cookie authentication.
    pub fn cookie_from_file(path: &Path) -> TorResult<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| TorError::Control(format!("cannot read control cookie: {e}")))?;
        if bytes.is_empty() {
            return Err(TorError::Control("control cookie is empty".into()));
        }
        Ok(ControlAuth::CookieHex(hex::encode(bytes)))
    }

    /// The exact command line to send.
    pub fn command(&self) -> String {
        match self {
            ControlAuth::None => "AUTHENTICATE".to_string(),
            ControlAuth::Password(password) => {
                format!("AUTHENTICATE {}", quote_arg(password))
            }
            ControlAuth::CookieHex(cookie) => format!("AUTHENTICATE {cookie}"),
        }
    }
}

/// A signal accepted by `SIGNAL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorSignal {
    Newnym,
    Reload,
    Shutdown,
    Halt,
    Dump,
    Debug,
}

impl TorSignal {
    fn token(self) -> &'static str {
        match self {
            TorSignal::Newnym => "NEWNYM",
            TorSignal::Reload => "RELOAD",
            TorSignal::Shutdown => "SHUTDOWN",
            TorSignal::Halt => "HALT",
            TorSignal::Dump => "DUMP",
            TorSignal::Debug => "DEBUG",
        }
    }
}

/// A complete control reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlReply {
    /// Status code (`250`, `552`, …).
    pub code: u16,
    /// Mid lines (`250-`) and the final line body, without their prefixes.
    pub lines: Vec<String>,
    /// Payload of a `250+` data block, if any.
    pub data: Option<String>,
}

impl ControlReply {
    pub fn is_ok(&self) -> bool {
        self.code == 250
    }

    /// Join all text lines (including the final one) with newlines.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Payload of a data block, else the joined lines.
    pub fn body(&self) -> String {
        self.data.clone().unwrap_or_else(|| self.text())
    }

    /// Parse `Key=Value` pairs from the reply lines, unquoting values.
    pub fn values(&self) -> Vec<(String, String)> {
        self.lines
            .iter()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), unquote(value)))
            .collect()
    }

    /// First value for `key` (case-insensitive).
    pub fn value(&self, key: &str) -> Option<String> {
        self.values()
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn ensure_ok(&self) -> TorResult<&Self> {
        if self.is_ok() {
            Ok(self)
        } else {
            Err(TorError::Control(format!(
                "control error {}: {}",
                self.code,
                self.text()
            )))
        }
    }
}

/// Feeding a line to [`ReplyAccumulator::push`] either completes a reply or
/// asks for the next line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyProgress {
    NeedMore,
    Complete(ControlReply),
}

/// Incremental reply framer. One accumulator handles exactly one reply.
#[derive(Debug, Default)]
pub struct ReplyAccumulator {
    code: Option<u16>,
    lines: Vec<String>,
    data: Option<String>,
    data_buf: Option<String>,
}

impl ReplyAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// True before the first line of a reply has been consumed.
    pub fn is_fresh(&self) -> bool {
        self.code.is_none() && self.data_buf.is_none()
    }

    /// Consume one line.
    pub fn push(&mut self, line: &str) -> TorResult<ReplyProgress> {
        if let Some(buf) = self.data_buf.as_mut() {
            if line == "." {
                self.data = self.data_buf.take();
                return Ok(ReplyProgress::NeedMore);
            }
            // RFC-style dot-stuffing: a leading ".." represents a single ".".
            if let Some(rest) = line.strip_prefix("..") {
                buf.push('.');
                buf.push_str(rest);
            } else {
                buf.push_str(line);
            }
            buf.push('\n');
            return Ok(ReplyProgress::NeedMore);
        }

        if line.len() < 4 {
            return Err(TorError::Control(format!("short control line: {line:?}")));
        }
        let code: u16 = line[..3]
            .parse()
            .map_err(|_| TorError::Control(format!("bad control status: {line:?}")))?;
        let separator = line.as_bytes()[3];
        let body = &line[4..];
        if self.code.is_none() {
            self.code = Some(code);
        }
        match separator {
            b'+' => {
                self.lines.push(body.to_string());
                self.data_buf = Some(String::new());
                Ok(ReplyProgress::NeedMore)
            }
            b'-' => {
                self.lines.push(body.to_string());
                Ok(ReplyProgress::NeedMore)
            }
            b' ' => {
                self.lines.push(body.to_string());
                Ok(ReplyProgress::Complete(ControlReply {
                    code,
                    lines: std::mem::take(&mut self.lines),
                    data: self.data.take(),
                }))
            }
            other => Err(TorError::Control(format!(
                "unexpected control separator {:?}",
                other as char
            ))),
        }
    }
}

/// An asynchronous `650` status event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvent {
    /// First keyword, e.g. `NOTICE`, `STATUS_CLIENT`, `NEWCONSENSUS`.
    pub keyword: String,
    /// Remainder of the line.
    pub message: String,
}

impl ControlEvent {
    pub fn parse(line: &str) -> Option<Self> {
        let body = line.strip_prefix("650")?;
        let body = body[1..].trim();
        if body.is_empty() {
            return Some(Self {
                keyword: String::new(),
                message: String::new(),
            });
        }
        let (keyword, message) = match body.split_once(char::is_whitespace) {
            Some((keyword, rest)) => (keyword.to_string(), rest.trim().to_string()),
            None => (body.to_string(), String::new()),
        };
        Some(Self { keyword, message })
    }
}

/// Parsed `PROTOCOLINFO` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolInfo {
    pub methods: Vec<String>,
    pub cookie_file: Option<String>,
    pub tor_version: Option<String>,
}

/// Parse a `PROTOCOLINFO` reply.
pub fn parse_protocol_info(reply: &ControlReply) -> TorResult<ProtocolInfo> {
    let mut info = ProtocolInfo {
        methods: Vec::new(),
        cookie_file: None,
        tor_version: None,
    };
    for line in reply.text().lines() {
        if let Some(rest) = line.strip_prefix("AUTH ") {
            for token in bootstrap::tokenize(rest) {
                if let Some(value) = token.strip_prefix("METHODS=") {
                    info.methods = value
                        .split(',')
                        .filter(|m| !m.is_empty())
                        .map(str::to_string)
                        .collect();
                } else if let Some(value) = token.strip_prefix("COOKIEFILE=") {
                    info.cookie_file = Some(value.to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("VERSION ") {
            for token in bootstrap::tokenize(rest) {
                if let Some(value) = token.strip_prefix("Tor=") {
                    info.tor_version = Some(value.to_string());
                }
            }
        }
    }
    if info.methods.is_empty() && info.tor_version.is_none() {
        return Err(TorError::Control("reply is not PROTOCOLINFO".into()));
    }
    Ok(info)
}

/// Byte-stream abstraction so the client can be tested without a daemon.
#[async_trait]
pub trait ControlTransport: Send {
    async fn send_line(&mut self, line: &str) -> TorResult<()>;
    async fn read_line(&mut self) -> TorResult<Option<String>>;
}

/// Control transport over a TCP connection to the daemon.
pub struct TcpControlTransport {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl TcpControlTransport {
    pub async fn connect(addr: SocketAddr) -> TorResult<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| TorError::Control(format!("connect to control port {addr}: {e}")))?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
        })
    }
}

#[async_trait]
impl ControlTransport for TcpControlTransport {
    async fn send_line(&mut self, line: &str) -> TorResult<()> {
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| TorError::Control(format!("write command: {e}")))?;
        self.writer
            .write_all(b"\r\n")
            .await
            .map_err(|e| TorError::Control(format!("write command: {e}")))?;
        self.writer
            .flush()
            .await
            .map_err(|e| TorError::Control(format!("flush command: {e}")))?;
        Ok(())
    }

    async fn read_line(&mut self) -> TorResult<Option<String>> {
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .await
            .map_err(|e| TorError::Control(format!("read reply: {e}")))?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
    }
}

/// A typed control-port client.
pub struct ControlClient<T: ControlTransport> {
    transport: T,
    events: VecDeque<ControlEvent>,
}

impl<T: ControlTransport> ControlClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            events: VecDeque::new(),
        }
    }

    /// Send one raw command and read exactly one reply, buffering any `650`
    /// events that arrive in the meantime.
    pub async fn command(&mut self, line: &str) -> TorResult<ControlReply> {
        self.transport.send_line(line).await?;
        self.read_reply().await
    }

    async fn read_reply(&mut self) -> TorResult<ControlReply> {
        let mut accumulator = ReplyAccumulator::new();
        loop {
            let line = self.transport.read_line().await?.ok_or_else(|| {
                TorError::Control("control connection closed before reply".into())
            })?;
            if accumulator.is_fresh() && is_async_event(&line) {
                if let Some(event) = ControlEvent::parse(&line) {
                    self.events.push_back(event);
                }
                continue;
            }
            match accumulator.push(&line)? {
                ReplyProgress::NeedMore => continue,
                ReplyProgress::Complete(reply) => return Ok(reply),
            }
        }
    }

    pub async fn authenticate(&mut self, auth: &ControlAuth) -> TorResult<()> {
        self.command(&auth.command()).await?.ensure_ok()?;
        Ok(())
    }

    pub async fn protocol_info(&mut self) -> TorResult<ProtocolInfo> {
        let reply = self.command("PROTOCOLINFO 1").await?;
        reply.ensure_ok()?;
        parse_protocol_info(&reply)
    }

    pub async fn get_info(&mut self, keys: &[&str]) -> TorResult<ControlReply> {
        if keys.is_empty() {
            return Err(TorError::Control("GETINFO needs at least one key".into()));
        }
        let reply = self.command(&format!("GETINFO {}", keys.join(" "))).await?;
        reply.ensure_ok()?;
        Ok(reply)
    }

    pub async fn signal(&mut self, signal: TorSignal) -> TorResult<()> {
        self.command(&format!("SIGNAL {}", signal.token()))
            .await?
            .ensure_ok()?;
        Ok(())
    }

    /// Set configuration options; an empty value resets the option.
    pub async fn set_conf(&mut self, pairs: &[(&str, &str)]) -> TorResult<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        let assignments = pairs
            .iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    (*key).to_string()
                } else {
                    format!("{key}={}", quote_arg(value))
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        self.command(&format!("SETCONF {assignments}"))
            .await?
            .ensure_ok()?;
        Ok(())
    }

    pub async fn get_conf(&mut self, key: &str) -> TorResult<Vec<(String, String)>> {
        let reply = self.command(&format!("GETCONF {key}")).await?;
        reply.ensure_ok()?;
        Ok(reply.values())
    }

    pub async fn add_onion(&mut self, request: &OnionRequest) -> TorResult<OnionResponse> {
        let reply = self.command(&request.to_command()).await?;
        reply.ensure_ok()?;
        OnionResponse::from_pairs(&reply.values())
    }

    pub async fn del_onion(&mut self, service_id: &str) -> TorResult<()> {
        self.command(&format!("DEL_ONION {service_id}"))
            .await?
            .ensure_ok()?;
        Ok(())
    }

    pub async fn bootstrap(&mut self) -> TorResult<BootstrapStatus> {
        let reply = self.get_info(&["status/bootstrap-phase"]).await?;
        let value = reply
            .value("status/bootstrap-phase")
            .unwrap_or_else(|| reply.body());
        bootstrap::parse_phase(&value)
            .ok_or_else(|| TorError::Bootstrap(format!("cannot parse bootstrap phase: {value}")))
    }

    pub async fn circuits(&mut self) -> TorResult<Vec<Circuit>> {
        let reply = self.get_info(&["circuit-status"]).await?;
        Ok(circuit::parse_circuit_status(&reply.body()))
    }

    pub async fn streams(&mut self) -> TorResult<Vec<Stream>> {
        let reply = self.get_info(&["stream-status"]).await?;
        Ok(circuit::parse_stream_status(&reply.body()))
    }

    /// Drain buffered asynchronous events.
    pub fn take_events(&mut self) -> Vec<ControlEvent> {
        self.events.drain(..).collect()
    }
}

fn is_async_event(line: &str) -> bool {
    line.len() >= 4 && line.starts_with("650") && matches!(line.as_bytes()[3], b' ' | b'-' | b'+')
}

/// Quote an argument for the control protocol, escaping only when necessary.
pub fn quote_arg(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\\' || c.is_control());
    if !needs_quotes {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Remove surrounding control-protocol quotes and unescape the body.
pub fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return value.to_string();
    }
    let inner = &value[1..value.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanomite_tor::onion::{OnionKeyInput, OnionPort};

    #[derive(Default)]
    struct MemoryTransport {
        input: VecDeque<String>,
        output: Vec<String>,
    }

    impl MemoryTransport {
        fn with_lines(lines: &[&str]) -> Self {
            Self {
                input: lines.iter().map(|l| (*l).to_string()).collect(),
                output: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl ControlTransport for MemoryTransport {
        async fn send_line(&mut self, line: &str) -> TorResult<()> {
            self.output.push(line.to_string());
            Ok(())
        }

        async fn read_line(&mut self) -> TorResult<Option<String>> {
            Ok(self.input.pop_front())
        }
    }

    #[tokio::test]
    async fn authenticate_uses_cookie_hex() {
        let transport = MemoryTransport::with_lines(&["250 OK"]);
        let mut client = ControlClient::new(transport);
        client
            .authenticate(&ControlAuth::CookieHex("deadbeef".into()))
            .await
            .expect("auth");
        assert_eq!(client.transport.output, vec!["AUTHENTICATE deadbeef"]);
    }

    #[tokio::test]
    async fn authenticate_quotes_password() {
        let transport = MemoryTransport::with_lines(&["250 OK"]);
        let mut client = ControlClient::new(transport);
        client
            .authenticate(&ControlAuth::Password("a b\"c".into()))
            .await
            .expect("auth");
        assert_eq!(client.transport.output, vec![r#"AUTHENTICATE "a b\"c""#]);
    }

    #[tokio::test]
    async fn multiline_and_data_replies_are_framed() {
        let transport = MemoryTransport::with_lines(&[
            "250-PROTOCOLINFO 1",
            "250-AUTH METHODS=COOKIE,SAFECOOKIE COOKIEFILE=\"/tmp/cookie\"",
            "250-VERSION Tor=\"0.4.8.10\"",
            "250 OK",
        ]);
        let mut client = ControlClient::new(transport);
        let info = client.protocol_info().await.expect("info");
        assert_eq!(info.methods, vec!["COOKIE", "SAFECOOKIE"]);
        assert_eq!(info.cookie_file.as_deref(), Some("/tmp/cookie"));
        assert_eq!(info.tor_version.as_deref(), Some("0.4.8.10"));
    }

    #[tokio::test]
    async fn data_block_is_collected() {
        let transport = MemoryTransport::with_lines(&[
            "250+circuit-status=",
            "1 BUILT $AAAA~g,$BBBB~m,$CCCC~e PURPOSE=GENERAL",
            ".",
            "250 OK",
        ]);
        let mut client = ControlClient::new(transport);
        let circuits = client.circuits().await.expect("circuits");
        assert_eq!(circuits.len(), 1);
        assert_eq!(circuits[0].hops.len(), 3);
    }

    #[tokio::test]
    async fn async_events_are_buffered_not_returned_as_replies() {
        let transport = MemoryTransport::with_lines(&[
            "650 STATUS_CLIENT NOTICE Circuit established",
            "250 OK",
        ]);
        let mut client = ControlClient::new(transport);
        client.get_info(&["version"]).await.expect("getinfo");
        let events = client.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].keyword, "STATUS_CLIENT");
        assert_eq!(events[0].message, "NOTICE Circuit established");
    }

    #[tokio::test]
    async fn error_replies_become_errors() {
        let transport = MemoryTransport::with_lines(&["552 Unrecognized configuration key"]);
        let mut client = ControlClient::new(transport);
        let error = client.get_conf("Nope").await.unwrap_err();
        assert!(error.to_string().contains("552"));
    }

    #[tokio::test]
    async fn add_onion_parses_service_id() {
        let transport = MemoryTransport::with_lines(&[
            "250-ServiceID=abcdefghijklmnop",
            "250-PrivateKey=ED25519-V3:AAAA",
            "250 OK",
        ]);
        let mut client = ControlClient::new(transport);
        let request = OnionRequest {
            key: OnionKeyInput::New,
            ports: vec![OnionPort::new(80, "127.0.0.1:8080")],
            flags: Vec::new(),
            max_streams: None,
        };
        let response = client.add_onion(&request).await.expect("add_onion");
        assert_eq!(response.service_id, "abcdefghijklmnop");
        assert_eq!(response.address(), "abcdefghijklmnop.onion");
        assert_eq!(
            client.transport.output,
            vec!["ADD_ONION NEW:ED25519-V3 Port=80,127.0.0.1:8080"]
        );
    }

    #[tokio::test]
    async fn bootstrap_phase_is_parsed() {
        let transport = MemoryTransport::with_lines(&[
            "250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"",
            "250 OK",
        ]);
        let mut client = ControlClient::new(transport);
        let status = client.bootstrap().await.expect("bootstrap");
        assert!(status.done());
        assert_eq!(status.tag, "done");
    }

    #[tokio::test]
    async fn signal_new_maps_to_token() {
        let transport = MemoryTransport::with_lines(&["250 OK"]);
        let mut client = ControlClient::new(transport);
        client.signal(TorSignal::Newnym).await.expect("signal");
        assert_eq!(client.transport.output, vec!["SIGNAL NEWNYM"]);
    }

    #[tokio::test]
    async fn set_conf_quotes_values() {
        let transport = MemoryTransport::with_lines(&["250 OK"]);
        let mut client = ControlClient::new(transport);
        client
            .set_conf(&[("ExitNodes", "{us},{de}"), ("StrictNodes", "1")])
            .await
            .expect("setconf");
        assert_eq!(
            client.transport.output,
            vec!["SETCONF ExitNodes={us},{de} StrictNodes=1"]
        );
    }

    #[test]
    fn accumulator_handles_dot_stuffing() {
        let mut acc = ReplyAccumulator::new();
        assert_eq!(acc.push("250+KEY=").unwrap(), ReplyProgress::NeedMore);
        assert_eq!(acc.push("..leading dot").unwrap(), ReplyProgress::NeedMore);
        assert_eq!(acc.push(".").unwrap(), ReplyProgress::NeedMore);
        let ReplyProgress::Complete(reply) = acc.push("250 OK").unwrap() else {
            panic!("expected completion");
        };
        assert_eq!(reply.code, 250);
        assert_eq!(reply.data.as_deref(), Some(".leading dot\n"));
    }

    #[test]
    fn accumulator_rejects_garbage() {
        let mut acc = ReplyAccumulator::new();
        assert!(acc.push("x").is_err());
        let mut acc = ReplyAccumulator::new();
        assert!(acc.push("abc!nope").is_err());
    }

    #[test]
    fn quote_and_unquote_roundtrip() {
        let value = "a b\"c\\d\ne";
        assert_eq!(unquote(&quote_arg(value)), value);
        assert_eq!(quote_arg("simple"), "simple");
    }
}
