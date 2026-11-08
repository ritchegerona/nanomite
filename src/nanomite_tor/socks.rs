//! SOCKS5 client (RFC 1928) with username/password auth (RFC 1929).
//!
//! Tor's SOCKS port accepts RFC 1929 credentials and uses the username as an
//! isolation key, so distinct credentials map to distinct circuits. Offering
//! only the username/password method when credentials are configured is what
//! makes that isolation reliable.
//!
//! The byte-level encoders/parsers are pure; the handshake is generic over any
//! `AsyncRead + AsyncWrite`, so tests drive it over `tokio::io::duplex`.

use crate::nanomite_tor::{TorError, TorResult};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const SOCKS_VERSION: u8 = 0x05;
pub const AUTH_VERSION: u8 = 0x01;
pub const CMD_CONNECT: u8 = 0x01;

pub const METHOD_NO_AUTH: u8 = 0x00;
pub const METHOD_USERPASS: u8 = 0x02;
pub const METHOD_NONE_ACCEPTABLE: u8 = 0xFF;

pub const ATYP_IPV4: u8 = 0x01;
pub const ATYP_DOMAIN: u8 = 0x03;
pub const ATYP_IPV6: u8 = 0x04;

/// Where a SOCKS connection should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksTarget {
    /// A literal IP address (no local DNS).
    Ip(SocketAddr),
    /// A hostname resolved by the proxy (remote DNS — required to avoid leaks).
    Domain { host: String, port: u16 },
}

impl SocksTarget {
    pub fn domain(host: impl Into<String>, port: u16) -> Self {
        SocksTarget::Domain {
            host: host.into(),
            port,
        }
    }
}

impl fmt::Display for SocksTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocksTarget::Ip(addr) => write!(f, "{addr}"),
            SocksTarget::Domain { host, port } => write!(f, "{host}:{port}"),
        }
    }
}

/// The address the proxy bound on our behalf (from the CONNECT reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksAddr {
    Ip(SocketAddr),
    Domain(String, u16),
}

impl SocksAddr {
    pub fn as_ip(&self) -> Option<SocketAddr> {
        match self {
            SocksAddr::Ip(addr) => Some(*addr),
            SocksAddr::Domain(..) => None,
        }
    }
}

impl fmt::Display for SocksAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocksAddr::Ip(addr) => write!(f, "{addr}"),
            SocksAddr::Domain(host, port) => write!(f, "{host}:{port}"),
        }
    }
}

/// A SOCKS5 client bound to one proxy address.
#[derive(Debug, Clone)]
pub struct Socks5Client {
    proxy: SocketAddr,
    auth: Option<(String, String)>,
}

impl Socks5Client {
    pub fn new(proxy: SocketAddr) -> Self {
        Self { proxy, auth: None }
    }

    pub fn with_auth(
        proxy: SocketAddr,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            proxy,
            auth: Some((user.into(), password.into())),
        }
    }

    pub fn proxy(&self) -> SocketAddr {
        self.proxy
    }

    /// Connect to `target` through the proxy.
    pub async fn connect(&self, target: SocksTarget) -> TorResult<tokio::net::TcpStream> {
        let mut stream = tokio::net::TcpStream::connect(self.proxy)
            .await
            .map_err(|e| TorError::Socks(format!("connect to proxy {}: {e}", self.proxy)))?;
        self.perform(&mut stream, &target).await?;
        Ok(stream)
    }

    /// Run the full handshake on an already-connected stream.
    pub async fn perform<S>(&self, stream: &mut S, target: &SocksTarget) -> TorResult<SocksAddr>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        // 1. Method negotiation. With credentials we offer *only* user/pass so
        //    the proxy cannot silently downgrade to no-auth (losing isolation).
        let greeting = method_greeting(self.auth.is_some());
        stream
            .write_all(&greeting)
            .await
            .map_err(|e| TorError::Socks(format!("write greeting: {e}")))?;
        let mut selection = [0u8; 2];
        stream
            .read_exact(&mut selection)
            .await
            .map_err(|e| TorError::Socks(format!("read method selection: {e}")))?;
        let method = parse_method_selection(&selection)?;
        match method {
            METHOD_NO_AUTH => {}
            METHOD_USERPASS => {
                let (user, password) = self.auth.as_ref().ok_or_else(|| {
                    TorError::Socks("proxy requires authentication but none configured".into())
                })?;
                let request = auth_request(user, password)?;
                stream
                    .write_all(&request)
                    .await
                    .map_err(|e| TorError::Socks(format!("write auth: {e}")))?;
                let mut reply = [0u8; 2];
                stream
                    .read_exact(&mut reply)
                    .await
                    .map_err(|e| TorError::Socks(format!("read auth reply: {e}")))?;
                parse_auth_reply(&reply)?;
            }
            METHOD_NONE_ACCEPTABLE => {
                return Err(TorError::Socks(
                    "proxy rejected all offered authentication methods".into(),
                ))
            }
            other => {
                return Err(TorError::Socks(format!(
                    "proxy selected unsupported authentication method {other:#04x}"
                )))
            }
        }

        // 2. CONNECT.
        let request = connect_request(target)?;
        stream
            .write_all(&request)
            .await
            .map_err(|e| TorError::Socks(format!("write connect: {e}")))?;
        let mut header = [0u8; 4];
        stream
            .read_exact(&mut header)
            .await
            .map_err(|e| TorError::Socks(format!("read connect reply: {e}")))?;
        let mut reply = header.to_vec();
        let extra = match header[3] {
            ATYP_IPV4 => 6,
            ATYP_IPV6 => 18,
            ATYP_DOMAIN => {
                let mut len = [0u8; 1];
                stream
                    .read_exact(&mut len)
                    .await
                    .map_err(|e| TorError::Socks(format!("read domain length: {e}")))?;
                reply.push(len[0]);
                1 + usize::from(len[0]) + 2
            }
            other => {
                return Err(TorError::Socks(format!(
                    "unsupported address type in reply: {other:#04x}"
                )))
            }
        };
        let mut tail = vec![0u8; extra];
        stream
            .read_exact(&mut tail)
            .await
            .map_err(|e| TorError::Socks(format!("read connect address: {e}")))?;
        reply.extend_from_slice(&tail);
        parse_connect_reply(&reply)
    }
}

/// Build the method-negotiation greeting.
pub fn method_greeting(use_auth: bool) -> Vec<u8> {
    if use_auth {
        vec![SOCKS_VERSION, 1, METHOD_USERPASS]
    } else {
        vec![SOCKS_VERSION, 1, METHOD_NO_AUTH]
    }
}

/// Build an RFC 1929 username/password request.
pub fn auth_request(user: &str, password: &str) -> TorResult<Vec<u8>> {
    let user = user.as_bytes();
    let password = password.as_bytes();
    if user.len() > u8::MAX as usize {
        return Err(TorError::Socks("SOCKS username too long".into()));
    }
    if password.len() > u8::MAX as usize {
        return Err(TorError::Socks("SOCKS password too long".into()));
    }
    let mut out = Vec::with_capacity(3 + user.len() + password.len());
    out.push(AUTH_VERSION);
    out.push(user.len() as u8);
    out.extend_from_slice(user);
    out.push(password.len() as u8);
    out.extend_from_slice(password);
    Ok(out)
}

/// Build a CONNECT request.
pub fn connect_request(target: &SocksTarget) -> TorResult<Vec<u8>> {
    let mut out = vec![SOCKS_VERSION, CMD_CONNECT, 0x00];
    match target {
        SocksTarget::Ip(SocketAddr::V4(addr)) => {
            out.push(ATYP_IPV4);
            out.extend_from_slice(&addr.ip().octets());
            out.extend_from_slice(&addr.port().to_be_bytes());
        }
        SocksTarget::Ip(SocketAddr::V6(addr)) => {
            out.push(ATYP_IPV6);
            out.extend_from_slice(&addr.ip().octets());
            out.extend_from_slice(&addr.port().to_be_bytes());
        }
        SocksTarget::Domain { host, port } => {
            let host = host.as_bytes();
            if host.is_empty() {
                return Err(TorError::Socks("empty SOCKS target host".into()));
            }
            if host.len() > u8::MAX as usize {
                return Err(TorError::Socks("SOCKS target host too long".into()));
            }
            out.push(ATYP_DOMAIN);
            out.push(host.len() as u8);
            out.extend_from_slice(host);
            out.extend_from_slice(&port.to_be_bytes());
        }
    }
    Ok(out)
}

/// Parse the two-byte method-selection reply.
pub fn parse_method_selection(reply: &[u8]) -> TorResult<u8> {
    if reply.len() < 2 {
        return Err(TorError::Socks("short method selection".into()));
    }
    if reply[0] != SOCKS_VERSION {
        return Err(TorError::Socks(format!(
            "unexpected SOCKS version {:#04x}",
            reply[0]
        )));
    }
    Ok(reply[1])
}

/// Parse the two-byte RFC 1929 auth reply.
pub fn parse_auth_reply(reply: &[u8]) -> TorResult<()> {
    if reply.len() < 2 {
        return Err(TorError::Socks("short auth reply".into()));
    }
    if reply[0] != AUTH_VERSION {
        return Err(TorError::Socks(format!(
            "unexpected auth version {:#04x}",
            reply[0]
        )));
    }
    if reply[1] != 0 {
        return Err(TorError::Socks(format!(
            "authentication failed (status {:#04x})",
            reply[1]
        )));
    }
    Ok(())
}

/// Parse a complete CONNECT reply (header + bound address).
pub fn parse_connect_reply(reply: &[u8]) -> TorResult<SocksAddr> {
    if reply.len() < 4 {
        return Err(TorError::Socks("short connect reply".into()));
    }
    if reply[0] != SOCKS_VERSION {
        return Err(TorError::Socks(format!(
            "unexpected SOCKS version {:#04x}",
            reply[0]
        )));
    }
    if reply[1] != 0 {
        return Err(TorError::Socks(format!(
            "proxy refused CONNECT: {}",
            reply_error(reply[1])
        )));
    }
    if reply[2] != 0 {
        return Err(TorError::Socks("non-zero reserved byte in reply".into()));
    }
    match reply[3] {
        ATYP_IPV4 => {
            if reply.len() < 10 {
                return Err(TorError::Socks("truncated IPv4 reply".into()));
            }
            let ip = Ipv4Addr::new(reply[4], reply[5], reply[6], reply[7]);
            let port = u16::from_be_bytes([reply[8], reply[9]]);
            Ok(SocksAddr::Ip(SocketAddr::V4(SocketAddrV4::new(ip, port))))
        }
        ATYP_IPV6 => {
            if reply.len() < 22 {
                return Err(TorError::Socks("truncated IPv6 reply".into()));
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&reply[4..20]);
            let port = u16::from_be_bytes([reply[20], reply[21]]);
            Ok(SocksAddr::Ip(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                port,
                0,
                0,
            ))))
        }
        ATYP_DOMAIN => {
            if reply.len() < 5 {
                return Err(TorError::Socks("truncated domain reply".into()));
            }
            let len = usize::from(reply[4]);
            if reply.len() < 5 + len + 2 {
                return Err(TorError::Socks("truncated domain reply".into()));
            }
            let host = String::from_utf8_lossy(&reply[5..5 + len]).to_string();
            let port = u16::from_be_bytes([reply[5 + len], reply[5 + len + 1]]);
            Ok(SocksAddr::Domain(host, port))
        }
        other => Err(TorError::Socks(format!(
            "unsupported address type in reply: {other:#04x}"
        ))),
    }
}

fn reply_error(code: u8) -> &'static str {
    match code {
        0x01 => "general failure",
        0x02 => "connection not allowed by ruleset",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown error",
    }
}

/// Bidirectionally copy between two connected streams, returning byte counts.
pub async fn relay<A, B>(a: &mut A, b: &mut B) -> TorResult<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    tokio::io::copy_bidirectional(a, b)
        .await
        .map_err(|e| TorError::Socks(format!("relay failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn addr() -> SocketAddr {
        "127.0.0.1:9050".parse().unwrap()
    }

    #[test]
    fn greeting_offers_only_configured_method() {
        assert_eq!(method_greeting(false), vec![5, 1, 0]);
        assert_eq!(method_greeting(true), vec![5, 1, 2]);
    }

    #[test]
    fn auth_request_encodes_lengths() {
        let request = auth_request("alice", "s3cret").expect("request");
        assert_eq!(request[0], 1);
        assert_eq!(request[1], 5);
        assert_eq!(&request[2..7], b"alice");
        assert_eq!(request[7], 6);
        assert_eq!(&request[8..], b"s3cret");
    }

    #[test]
    fn auth_request_rejects_oversized_fields() {
        let long = "x".repeat(256);
        assert!(auth_request(&long, "p").is_err());
        assert!(auth_request("u", &long).is_err());
    }

    #[test]
    fn connect_request_ipv4() {
        let request =
            connect_request(&SocksTarget::Ip("192.0.2.1:443".parse().unwrap())).expect("request");
        assert_eq!(request, vec![5, 1, 0, 1, 192, 0, 2, 1, 0x01, 0xbb]);
    }

    #[test]
    fn connect_request_domain_uses_remote_dns() {
        let request = connect_request(&SocksTarget::domain("example.com", 80)).expect("request");
        assert_eq!(request[0..4], [5, 1, 0, 3]);
        assert_eq!(request[4], 11);
        assert_eq!(&request[5..16], b"example.com");
        assert_eq!(&request[16..18], &80u16.to_be_bytes());
    }

    #[test]
    fn connect_request_rejects_empty_host() {
        assert!(connect_request(&SocksTarget::domain("", 80)).is_err());
    }

    #[test]
    fn method_selection_parses_and_rejects() {
        assert_eq!(parse_method_selection(&[5, 0]).unwrap(), 0);
        assert!(parse_method_selection(&[4, 0]).is_err());
        assert!(parse_method_selection(&[5]).is_err());
    }

    #[test]
    fn auth_reply_parses_success_and_failure() {
        assert!(parse_auth_reply(&[1, 0]).is_ok());
        assert!(parse_auth_reply(&[1, 1]).is_err());
        assert!(parse_auth_reply(&[2, 0]).is_err());
    }

    #[test]
    fn connect_reply_parses_ipv4() {
        let reply = [5, 0, 0, 1, 127, 0, 0, 1, 0x1f, 0x90];
        assert_eq!(
            parse_connect_reply(&reply).unwrap(),
            SocksAddr::Ip("127.0.0.1:8080".parse().unwrap())
        );
    }

    #[test]
    fn connect_reply_parses_domain() {
        let mut reply = vec![5, 0, 0, 3, 11];
        reply.extend_from_slice(b"example.com");
        reply.extend_from_slice(&443u16.to_be_bytes());
        assert_eq!(
            parse_connect_reply(&reply).unwrap(),
            SocksAddr::Domain("example.com".into(), 443)
        );
    }

    #[test]
    fn connect_reply_reports_refusal() {
        let reply = [5, 5, 0, 1, 0, 0, 0, 0, 0, 0];
        let err = parse_connect_reply(&reply).unwrap_err();
        assert!(err.to_string().contains("connection refused"));
    }

    #[tokio::test]
    async fn performs_full_handshake_over_duplex() {
        let (client_side, mut server_side) = tokio::io::duplex(256);
        let server = tokio::spawn(async move {
            // greeting
            let mut head = [0u8; 2];
            server_side.read_exact(&mut head).await.unwrap();
            let mut methods = vec![0u8; usize::from(head[1])];
            server_side.read_exact(&mut methods).await.unwrap();
            assert_eq!(head[0], 5);
            assert_eq!(methods, vec![2]);
            server_side.write_all(&[5, 2]).await.unwrap();
            // auth
            let mut auth_head = [0u8; 2];
            server_side.read_exact(&mut auth_head).await.unwrap();
            let mut user = vec![0u8; usize::from(auth_head[1])];
            server_side.read_exact(&mut user).await.unwrap();
            let mut plen = [0u8; 1];
            server_side.read_exact(&mut plen).await.unwrap();
            let mut pass = vec![0u8; usize::from(plen[0])];
            server_side.read_exact(&mut pass).await.unwrap();
            assert_eq!(&user, b"alice");
            assert_eq!(&pass, b"pw");
            server_side.write_all(&[1, 0]).await.unwrap();
            // connect: header + domain len + host + port
            let mut req = [0u8; 4];
            server_side.read_exact(&mut req).await.unwrap();
            assert_eq!(req, [5, 1, 0, 3]);
            let mut len = [0u8; 1];
            server_side.read_exact(&mut len).await.unwrap();
            let mut host = vec![0u8; usize::from(len[0])];
            server_side.read_exact(&mut host).await.unwrap();
            let mut port = [0u8; 2];
            server_side.read_exact(&mut port).await.unwrap();
            assert_eq!(&host, b"example.com");
            assert_eq!(u16::from_be_bytes(port), 80);
            // success reply
            server_side
                .write_all(&[5, 0, 0, 1, 93, 184, 216, 34, 0x01, 0xbb])
                .await
                .unwrap();
        });

        let client = Socks5Client::with_auth(addr(), "alice", "pw");
        let mut stream = client_side;
        let bound = client
            .perform(&mut stream, &SocksTarget::domain("example.com", 80))
            .await
            .expect("handshake");
        assert_eq!(bound.to_string(), "93.184.216.34:443");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn relay_moves_bytes_both_ways() {
        let (mut a1, mut a2) = tokio::io::duplex(64);
        let (mut b1, mut b2) = tokio::io::duplex(64);
        let pump = tokio::spawn(async move { relay(&mut a2, &mut b2).await });
        a1.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        b1.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        b1.write_all(b"pong").await.unwrap();
        let mut buf2 = [0u8; 4];
        a1.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, b"pong");
        drop(a1);
        drop(b1);
        let _ = pump.await;
    }
}
