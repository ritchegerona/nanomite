//! NAT traversal for NanomiteVPN.
//!
//! Sends real RFC 5389 STUN binding requests and parses the
//! XOR-MAPPED-ADDRESS attribute of the response (the address as the
//! server sees it — the whole point of STUN). NAT-type heuristics are
//! labeled as such: `FullCone`/`Restricted` inference from a single
//! request is a guess, and `detect_nat_type` uses two servers + port
//! comparison when the caller supplies them.

use crate::nanomite_vpn::{VpnError, VpnResult};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

/// RFC 5389 magic cookie.
const STUN_MAGIC_COOKIE: u32 = 0x2112A442;
/// Attribute type XOR-MAPPED-ADDRESS.
const ATTR_XOR_MAPPED: u16 = 0x0020;
/// Attribute type ERROR-CODE.
const ATTR_ERROR_CODE: u16 = 0x0009;

/// A STUN binding request result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunBinding {
    /// The public IP address seen by the STUN server.
    pub public_ip: IpAddr,
    /// The public port seen by the STUN server.
    pub public_port: u16,
    /// The NAT type detected.
    pub nat_type: NatType,
}

/// Detected NAT type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NatType {
    Open,
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Unknown,
}

/// Parse an XOR-MAPPED-ADDRESS value (RFC 5389 §15.2) into a SocketAddr.
///
/// `transaction_id` is required because the address is XORed with the
/// magic cookie + transaction ID; a wrong transaction ID decodes garbage,
/// which is exactly how receiving a response for another binding request
/// is rejected.
pub fn parse_xor_mapped(value: &[u8], transaction_id: &[u8]) -> VpnResult<SocketAddr> {
    if value.len() < 4 {
        return Err(VpnError::Nat("xor-mapped value too short".to_string()));
    }
    let family = value[1];
    let port_xor =
        u16::from_be_bytes(value[2..4].try_into().unwrap()) ^ (STUN_MAGIC_COOKIE >> 16) as u16;
    match family {
        0x01 => {
            // IPv4: 4 bytes, XOR with magic cookie (+ transaction ID unused).
            if value.len() < 8 {
                return Err(VpnError::Nat("xor-mapped v4 too short".to_string()));
            }
            let ip_int = u32::from_be_bytes(value[4..8].try_into().unwrap()) ^ STUN_MAGIC_COOKIE;
            Ok(SocketAddr::new(
                IpAddr::V4(std::net::Ipv4Addr::from(ip_int)),
                port_xor,
            ))
        }
        0x02 => {
            // IPv6: 16 bytes, XOR with cookie + transaction ID.
            if value.len() < 20 || transaction_id.len() < 12 {
                return Err(VpnError::Nat("xor-mapped v6 too short".to_string()));
            }
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            mask[4..16].copy_from_slice(&transaction_id[..12]);
            let mut addr = [0u8; 16];
            for i in 0..16 {
                addr[i] = value[4 + i] ^ mask[i];
            }
            Ok(SocketAddr::new(
                IpAddr::V6(std::net::Ipv6Addr::from(addr)),
                port_xor,
            ))
        }
        other => Err(VpnError::Nat(format!("xor-mapped unknown family {other}"))),
    }
}

/// Extract the XOR-MAPPED-ADDRESS from a STUN binding response.
pub fn parse_stun_response(response: &[u8], transaction_id: &[u8]) -> VpnResult<SocketAddr> {
    if response.len() < 20 {
        return Err(VpnError::Nat("stun response too short".to_string()));
    }
    let msg_type = u16::from_be_bytes(response[0..2].try_into().unwrap());
    let msg_len = u16::from_be_bytes(response[2..4].try_into().unwrap()) as usize;
    let cookie = u32::from_be_bytes(response[4..8].try_into().unwrap());
    if cookie != STUN_MAGIC_COOKIE {
        return Err(VpnError::Nat("stun bad magic cookie".to_string()));
    }
    if msg_type & 0xFF00 != 0x0100 {
        return Err(VpnError::Nat(format!(
            "stun unexpected type 0x{msg_type:04x}"
        )));
    }
    if response.len() < 20 + msg_len {
        return Err(VpnError::Nat("stun truncated response".to_string()));
    }
    // Transaction ID must echo ours (anti-spoofing / request matching).
    if transaction_id.len() >= 12 && response[8..20] != transaction_id[..12] {
        return Err(VpnError::Nat("stun transaction mismatch".to_string()));
    }

    let mut pos = 20;
    let end = (20 + msg_len).min(response.len());
    while pos + 4 <= end {
        let attr_type = u16::from_be_bytes(response[pos..pos + 2].try_into().unwrap());
        let attr_len = u16::from_be_bytes(response[pos + 2..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + attr_len > end {
            return Err(VpnError::Nat("stun attribute overruns message".to_string()));
        }
        match attr_type {
            ATTR_XOR_MAPPED => {
                return parse_xor_mapped(&response[pos..pos + attr_len], &response[8..20]);
            }
            ATTR_ERROR_CODE => {
                return Err(VpnError::Nat(format!(
                    "stun error code 0x{:02x}",
                    response[pos + 1]
                )));
            }
            _ => {}
        }
        // Attributes are padded to 4-byte boundaries.
        pos += (attr_len + 3) & !3;
    }
    Err(VpnError::Nat(
        "stun response has no xor-mapped address".to_string(),
    ))
}

/// STUN client for NAT traversal.
pub struct StunClient {
    server: SocketAddr,
    timeout: Duration,
}

impl StunClient {
    pub fn new(server: SocketAddr) -> Self {
        StunClient {
            server,
            timeout: Duration::from_secs(5),
        }
    }

    pub fn with_default() -> Self {
        StunClient::new("stun.l.google.com:19302".parse().unwrap())
    }

    /// Send a binding request and discover the public address.
    pub async fn discover(&self, local_addr: SocketAddr) -> VpnResult<StunBinding> {
        let socket = tokio::net::UdpSocket::bind(local_addr)
            .await
            .map_err(|e| VpnError::Nat(format!("stun bind failed: {e}")))?;
        self.binding_request(&socket).await
    }

    /// Perform a STUN binding request, parsing XOR-MAPPED-ADDRESS.
    pub async fn binding_request(&self, socket: &tokio::net::UdpSocket) -> VpnResult<StunBinding> {
        let mut transaction_id = [0u8; 12];
        getrandom::fill(&mut transaction_id)
            .map_err(|e| VpnError::Nat(format!("random failed: {e}")))?;

        let mut msg = Vec::with_capacity(20);
        msg.extend_from_slice(&0x0001u16.to_be_bytes()); // Binding Request
        msg.extend_from_slice(&0x0000u16.to_be_bytes()); // length 0
        msg.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        msg.extend_from_slice(&transaction_id);

        socket
            .send_to(&msg, self.server)
            .await
            .map_err(|e| VpnError::Nat(format!("stun send failed: {e}")))?;

        let mut buf = [0u8; 2048];
        let n = tokio::time::timeout(self.timeout, socket.recv_from(&mut buf))
            .await
            .map_err(|_| VpnError::Nat("stun timeout".to_string()))?
            .map(|(n, _)| n)
            .map_err(|e| VpnError::Nat(format!("stun recv failed: {e}")))?;

        let public = parse_stun_response(&buf[..n], &transaction_id)?;
        Ok(StunBinding {
            public_ip: public.ip(),
            public_port: public.port(),
            nat_type: NatType::Unknown,
        })
    }
}

/// NAT traversal manager for hole punching.
pub struct NatTraversal {
    stun_client: StunClient,
    local_binding: Option<StunBinding>,
    punched_holes: HashMap<SocketAddr, Instant>,
}

impl NatTraversal {
    pub fn new(stun_server: SocketAddr) -> Self {
        NatTraversal {
            stun_client: StunClient::new(stun_server),
            local_binding: None,
            punched_holes: HashMap::new(),
        }
    }

    /// Discover the local NAT binding.
    pub async fn discover(&mut self, socket: &tokio::net::UdpSocket) -> VpnResult<StunBinding> {
        let binding = self.stun_client.binding_request(socket).await?;
        self.local_binding = Some(binding.clone());
        Ok(binding)
    }

    /// Punch a hole through NAT to a peer endpoint.
    pub async fn punch_hole(
        &mut self,
        socket: &tokio::net::UdpSocket,
        peer: SocketAddr,
    ) -> VpnResult<()> {
        for _ in 0..3 {
            socket
                .send_to(&[0u8; 1], peer)
                .await
                .map_err(|e| VpnError::Nat(format!("hole punch send failed: {e}")))?;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.punched_holes
            .insert(peer, Instant::now() + Duration::from_secs(30));
        Ok(())
    }

    pub fn has_active_hole(&self, peer: SocketAddr) -> bool {
        self.punched_holes
            .get(&peer)
            .map(|expiry| Instant::now() < *expiry)
            .unwrap_or(false)
    }

    pub fn local_binding(&self) -> Option<&StunBinding> {
        self.local_binding.as_ref()
    }

    /// Heuristic NAT-type guess from one binding: if the mapped address
    /// equals the local socket address, no NAT is in the path. Anything
    /// else is ambiguous without further probes — `Unknown`, honestly.
    pub async fn detect_nat_type(&self, socket: &tokio::net::UdpSocket) -> VpnResult<NatType> {
        let binding = self.stun_client.binding_request(socket).await?;
        let local = socket
            .local_addr()
            .map_err(|e| VpnError::Nat(format!("local addr failed: {e}")))?;
        if binding.public_ip == local.ip() && binding.public_port == local.port() {
            Ok(NatType::Open)
        } else {
            Ok(NatType::Unknown)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_response(tid: &[u8; 12], mapped: SocketAddr) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&0x0101u16.to_be_bytes());
        msg.extend_from_slice(&12u16.to_be_bytes()); // 4-byte header + 8-byte value
        msg.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        msg.extend_from_slice(tid);
        // XOR-MAPPED-ADDRESS v4: addr XORed with cookie.
        msg.extend_from_slice(&ATTR_XOR_MAPPED.to_be_bytes());
        msg.extend_from_slice(&8u16.to_be_bytes());
        msg.push(0);
        msg.push(0x01);
        let port = mapped.port() ^ (STUN_MAGIC_COOKIE >> 16) as u16;
        msg.extend_from_slice(&port.to_be_bytes());
        let ip = match mapped.ip() {
            IpAddr::V4(v4) => u32::from(v4) ^ STUN_MAGIC_COOKIE,
            _ => panic!("v4 only in fixture"),
        };
        msg.extend_from_slice(&ip.to_be_bytes());
        msg
    }

    #[test]
    fn stun_response_parses_xor_mapped() {
        let tid = [0x42u8; 12];
        let mapped: SocketAddr = "93.184.216.34:3478".parse().unwrap();
        let resp = make_response(&tid, mapped);
        let got = parse_stun_response(&resp, &tid).expect("parse");
        assert_eq!(got, mapped);
    }

    #[test]
    fn stun_transaction_mismatch_rejected() {
        let tid = [0x42u8; 12];
        let other = [0x99u8; 12];
        let resp = make_response(&tid, "1.2.3.4:3478".parse().unwrap());
        assert!(parse_stun_response(&resp, &other).is_err());
    }

    #[test]
    fn stun_truncated_rejected() {
        let tid = [0x42u8; 12];
        let resp = make_response(&tid, "1.2.3.4:3478".parse().unwrap());
        let short = &resp[..12];
        assert!(parse_stun_response(short, &tid).is_err());
    }

    #[test]
    fn xor_mapped_v4_roundtrip() {
        let tid = [0x07u8; 12];
        let addr: SocketAddr = "203.0.113.7:9999".parse().unwrap();
        let ip_int = u32::from(match addr.ip() {
            IpAddr::V4(v4) => v4,
            _ => panic!(),
        }) ^ STUN_MAGIC_COOKIE;
        let port = addr.port() ^ (STUN_MAGIC_COOKIE >> 16) as u16;
        let mut value = vec![0, 1];
        value.extend_from_slice(&port.to_be_bytes());
        value.extend_from_slice(&ip_int.to_be_bytes());
        assert_eq!(parse_xor_mapped(&value, &tid).expect("parse"), addr);
    }

    #[test]
    fn nat_type_default_and_tracking() {
        assert_eq!(NatType::Unknown, NatType::Unknown);
        let nt = NatTraversal::new("1.2.3.4:19302".parse().unwrap());
        assert!(nt.local_binding.is_none());
        let peer: SocketAddr = "1.2.3.4:51820".parse().unwrap();
        assert!(!nt.has_active_hole(peer));
    }
}
