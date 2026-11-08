//! VPN engine — the main tunnel lifecycle and packet routing.
//!
//! Wire format for tunnel packets (all integers little-endian):
//!
//! ```text
//! +--------+-------------+-----------+--------+----------+
//! | "NMVP" | sender_idx  | counter   | ciphertext || tag (AEAD)
//! | 4B     | 4B          | 8B        |
//! +--------+-------------+-----------+--------+----------+
//! ```
//!
//! - Nonce = `sender_index || counter` (see `crypto::wire_nonce`), so the
//!   receiver reconstructs the AEAD nonce from the header alone — no RNG,
//!   no nonce bytes on the wire beyond the counter.
//! - Incoming packets pass a `ReplayWindow` before decryption fails count:
//!   duplicates and out-of-window replays are dropped cheaply.
//! - Handshake/control messages are the JSON protocol messages from
//!   `protocol.rs` (they start with `{`, binary packets with `NMVP`).
//!
//! The engine is runtime state only: platform concerns (TUN, sockets,
//! kill switch, DoH) plug in through the seams already here.

use crate::nanomite_vpn::{
    crypto::{self, wire_nonce, KeyPair, ReplayWindow, SessionKey, REKEY_PACKET_LIMIT},
    protocol::{HandshakeInit, HandshakeProtocol, HandshakeResponse, Keepalive, SessionKeys},
    tun::{TunConfig, TunDevice},
    VpnError, VpnResult,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration, Instant};
use uuid::Uuid;

/// The overall state of a VPN tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Established,
    Disconnecting,
    Failed(String),
}

impl TunnelState {
    /// Stable numeric code for CLI display.
    pub fn code(&self) -> u32 {
        match self {
            TunnelState::Disconnected => 0,
            TunnelState::Connecting => 1,
            TunnelState::Established => 2,
            TunnelState::Disconnecting => 3,
            TunnelState::Failed(_) => 4,
        }
    }
}

/// Events emitted by the tunnel engine.
#[derive(Debug, Clone)]
pub enum TunnelEvent {
    Connecting { peer: String },
    Established { peer: String, fingerprint: String },
    Disconnected { peer: String },
    Failed { peer: String, error: String },
    PacketSent { bytes: usize },
    PacketReceived { bytes: usize },
    KeepaliveSent,
    KeepaliveReceived,
    Rekeyed { peer: String },
}

/// Configuration for a VPN peer.
#[derive(Debug, Clone)]
pub struct PeerConfig {
    /// Peer's Ed25519 identity public key.
    pub public_key: [u8; 32],
    /// Endpoint address (host:port) of the peer.
    pub endpoint: SocketAddr,
    /// Allowed IPs for this peer (CIDR notation).
    pub allowed_ips: Vec<String>,
    /// Persistent keepalive interval in seconds.
    pub persistent_keepalive: Option<u32>,
    /// Pre-shared key (optional), mixed into the handshake KDF.
    pub preshared_key: Option<[u8; 32]>,
}

/// Magic prefix for binary tunnel packets ("Nanomite VPN Packet").
pub const PACKET_MAGIC: &[u8; 4] = b"NMVP";

/// Length of the tunnel packet header: magic ‖ index ‖ counter.
pub const PACKET_HEADER_LEN: usize = 16;

/// Build a tunnel datagram: header ‖ seal(plaintext).
fn build_packet(
    sender_index: u32,
    counter: u64,
    send_key: &SessionKey,
    plaintext: &[u8],
) -> VpnResult<Vec<u8>> {
    let ciphertext = send_key.seal(&wire_nonce(sender_index, counter), plaintext, b"vpn-tunnel")?;
    let mut out = Vec::with_capacity(PACKET_HEADER_LEN + ciphertext.len());
    out.extend_from_slice(PACKET_MAGIC);
    out.extend_from_slice(&sender_index.to_le_bytes());
    out.extend_from_slice(&counter.to_le_bytes());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Main VPN engine struct.
pub struct VpnEngine {
    /// Unique tunnel ID. Kept for future event correlation/debugging.
    #[allow(dead_code)]
    id: Uuid,
    /// Local Ed25519 identity key pair.
    keypair: KeyPair,
    /// Local listening socket.
    socket: Option<Arc<UdpSocket>>,
    /// TUN device (if created).
    tun: Option<Arc<Mutex<TunDevice>>>,
    /// Connected peers, keyed by their Ed25519 identity public key.
    peers: RwLock<HashMap<[u8; 32], PeerSession>>,
    /// Tunnel configuration.
    config: VpnConfig,
    /// Current tunnel state.
    state: RwLock<TunnelState>,
    /// Event callback.
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<TunnelEvent>>,
    /// Next sender index to allocate (monotonically increasing).
    next_index: AtomicU32,
}

/// A session with a connected peer.
struct PeerSession {
    /// Peer's Ed25519 identity public key.
    public_key: [u8; 32],
    /// Peer's endpoint address (updated on roam).
    endpoint: SocketAddr,
    /// Optional pre-shared key for this peer.
    psk: Option<[u8; 32]>,
    /// Handshake protocol state.
    handshake: HandshakeProtocol,
    /// Established transport keys.
    session_keys: Option<SessionKeys>,
    /// Outgoing packet counter (monotonic per session).
    send_counter: u64,
    /// Packets sent since the last (re)key.
    sent_since_rekey: u64,
    /// Anti-replay window over incoming counters.
    replay: ReplayWindow,
    /// Last time we received a packet from this peer.
    last_received: Instant,
    /// Tunnel state for this peer.
    state: TunnelState,
}

/// VPN configuration.
#[derive(Debug, Clone)]
pub struct VpnConfig {
    pub listen_port: u16,
    pub tun_config: TunConfig,
    /// Maximum session age before rekey (seconds).
    pub max_session_age: u64,
    /// Keepalive interval (seconds).
    pub keepalive_interval: u64,
    /// Handshake timeout (seconds).
    pub handshake_timeout: u64,
    /// MTU for encrypted packets.
    pub mtu: u16,
}

impl Default for VpnConfig {
    fn default() -> Self {
        VpnConfig {
            listen_port: 51820,
            tun_config: TunConfig::default(),
            max_session_age: 7200, // 2 hours
            keepalive_interval: 15,
            handshake_timeout: 90,
            mtu: 1400,
        }
    }
}

impl VpnEngine {
    /// Create a new VPN engine with a random identity key pair.
    pub async fn new(config: VpnConfig) -> VpnResult<Self> {
        Ok(Self::with_keypair(config, KeyPair::generate()?))
    }

    /// Create a new VPN engine with a specific identity key pair.
    pub fn with_keypair(config: VpnConfig, keypair: KeyPair) -> Self {
        VpnEngine {
            id: Uuid::new_v4(),
            keypair,
            socket: None,
            tun: None,
            peers: RwLock::new(HashMap::new()),
            config,
            state: RwLock::new(TunnelState::Disconnected),
            event_tx: None,
            next_index: AtomicU32::new(1),
        }
    }

    fn alloc_index(&self) -> u32 {
        self.next_index.fetch_add(1, Ordering::Relaxed)
    }

    /// Get the local identity public key.
    pub fn public_key(&self) -> [u8; 32] {
        self.keypair.public
    }

    /// Get the local fingerprint.
    pub fn fingerprint(&self) -> String {
        crypto::peer_fingerprint(&self.keypair.public)
    }

    /// Set an event channel for tunnel events.
    pub fn set_event_channel(&mut self, tx: tokio::sync::mpsc::UnboundedSender<TunnelEvent>) {
        self.event_tx = Some(tx);
    }

    /// Emit a tunnel event if a channel is set.
    fn emit_event(&self, event: TunnelEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    /// Get the current tunnel state.
    pub async fn state(&self) -> TunnelState {
        self.state.read().await.clone()
    }

    /// Bind to a UDP port and start listening.
    pub async fn bind(&mut self, addr: SocketAddr) -> VpnResult<()> {
        let socket = UdpSocket::bind(addr)
            .await
            .map_err(|e| VpnError::Network(format!("bind failed: {e}")))?;
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    /// Create a TUN device for this tunnel.
    pub async fn create_tun(&mut self) -> VpnResult<()> {
        let tun = TunDevice::new(
            &self.config.tun_config.name,
            &self.config.tun_config.ip,
            &self.config.tun_config.netmask,
        )
        .await?;
        self.tun = Some(Arc::new(Mutex::new(tun)));
        Ok(())
    }

    /// Add a peer to connect to.
    pub async fn add_peer(&self, peer: PeerConfig) -> VpnResult<()> {
        let mut peers = self.peers.write().await;
        let handshake = HandshakeProtocol::new(
            self.alloc_index(),
            self.keypair.clone(),
            peer.public_key,
            peer.preshared_key,
        )?;
        peers.insert(
            peer.public_key,
            PeerSession {
                public_key: peer.public_key,
                endpoint: peer.endpoint,
                psk: peer.preshared_key,
                handshake,
                session_keys: None,
                send_counter: 0,
                sent_since_rekey: 0,
                replay: ReplayWindow::new(),
                last_received: Instant::now(),
                state: TunnelState::Disconnected,
            },
        );
        Ok(())
    }

    /// Connect to a peer (initiator side: sends a signed HandshakeInit).
    pub async fn connect(&self, public_key: &[u8; 32]) -> VpnResult<()> {
        let endpoint = {
            let peers = self.peers.read().await;
            let session = peers
                .get(public_key)
                .ok_or_else(|| VpnError::PeerNotFound(format!("{public_key:?}")))?;
            session.endpoint
        };

        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| VpnError::Network("socket not bound".to_string()))?;

        let init: HandshakeInit = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(public_key)
                .ok_or_else(|| VpnError::PeerNotFound(format!("{public_key:?}")))?;
            session.state = TunnelState::Connecting;
            session.handshake.create_initiation()?
        };
        self.state
            .write()
            .await
            .clone_from(&TunnelState::Connecting);
        self.emit_event(TunnelEvent::Connecting {
            peer: hex::encode(public_key),
        });

        let data = init.serialize()?;
        socket
            .send_to(&data, endpoint)
            .await
            .map_err(|e| VpnError::Network(format!("send initiation failed: {e}")))?;
        Ok(())
    }

    /// Handle an incoming UDP datagram. Dispatches binary tunnel packets
    /// by magic and JSON protocol messages by leading `{`.
    pub async fn handle_udp_packet(
        &self,
        data: &[u8],
        src: SocketAddr,
    ) -> VpnResult<Option<Vec<u8>>> {
        if data.first() == Some(&b'{') {
            if let Ok(ka) = serde_json::from_slice::<Keepalive>(data) {
                return self.handle_keepalive(&ka).await;
            }
            if let Ok(init) = serde_json::from_slice::<HandshakeInit>(data) {
                return self.handle_initiation(&init, src).await;
            }
            if let Ok(resp) = serde_json::from_slice::<HandshakeResponse>(data) {
                return self.handle_response(&resp).await;
            }
            return Err(VpnError::Protocol(
                "unrecognized protocol message".to_string(),
            ));
        }
        self.handle_encrypted_packet(data).await
    }

    /// Handle an incoming handshake initiation.
    async fn handle_initiation(
        &self,
        init: &HandshakeInit,
        src: SocketAddr,
    ) -> VpnResult<Option<Vec<u8>>> {
        let mut peers = self.peers.write().await;
        let Some(session) = peers.get_mut(&init.sender_identity) else {
            return Err(VpnError::Protocol(
                "initiation from unknown identity".to_string(),
            ));
        };
        // Roam: the initiator's current endpoint is its source address.
        session.endpoint = src;
        let response = session.handshake.handle_initiation(init)?;
        session.session_keys = session.handshake.session_keys();
        session.state = TunnelState::Established;
        session.last_received = Instant::now();
        let data = response.serialize()?;
        Ok(Some(data))
    }

    /// Handle an incoming handshake response.
    async fn handle_response(&self, response: &HandshakeResponse) -> VpnResult<Option<Vec<u8>>> {
        let mut peers = self.peers.write().await;
        let Some(session) = peers.get_mut(&response.sender_identity) else {
            return Err(VpnError::Protocol(
                "response from unknown identity".to_string(),
            ));
        };
        session.handshake.handle_response(response)?;
        session.session_keys = session.handshake.session_keys();
        session.state = TunnelState::Established;
        session.last_received = Instant::now();
        let peer_hex = hex::encode(session.public_key);
        self.emit_event(TunnelEvent::Established {
            peer: peer_hex,
            fingerprint: session
                .session_keys
                .as_ref()
                .map(|k| crypto::peer_fingerprint(&k.peer_public))
                .unwrap_or_default(),
        });
        drop(peers);
        self.state
            .write()
            .await
            .clone_from(&TunnelState::Established);
        Ok(None)
    }

    /// Handle an incoming keepalive (NAT binding refresh + liveness).
    async fn handle_keepalive(&self, ka: &Keepalive) -> VpnResult<Option<Vec<u8>>> {
        let _ = ka;
        self.emit_event(TunnelEvent::KeepaliveReceived);
        Ok(None)
    }

    /// Handle an incoming binary tunnel packet.
    async fn handle_encrypted_packet(&self, data: &[u8]) -> VpnResult<Option<Vec<u8>>> {
        if data.len() < PACKET_HEADER_LEN || &data[..4] != PACKET_MAGIC {
            return Err(VpnError::Protocol("bad tunnel packet header".to_string()));
        }
        let sender_index = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let counter = u64::from_le_bytes(data[8..16].try_into().unwrap());
        let ciphertext = &data[PACKET_HEADER_LEN..];

        let mut peers = self.peers.write().await;
        let Some(session) = peers
            .values_mut()
            .find(|s| s.handshake.their_index == Some(sender_index))
        else {
            return Err(VpnError::Protocol(
                "packet from unknown sender index".to_string(),
            ));
        };
        if session.state != TunnelState::Established {
            return Err(VpnError::Protocol(
                "packet for unestablished session".to_string(),
            ));
        }
        if !session.replay.accept(counter) {
            return Err(VpnError::Protocol(
                "replay / out-of-window packet".to_string(),
            ));
        }
        let keys = session
            .session_keys
            .as_ref()
            .ok_or_else(|| VpnError::Protocol("no session keys".to_string()))?;
        let plaintext = keys.recv_key.open(
            &wire_nonce(sender_index, counter),
            ciphertext,
            b"vpn-tunnel",
        )?;
        session.last_received = Instant::now();
        drop(peers);

        if let Some(tun) = &self.tun {
            let mut tun = tun.lock().await;
            tun.write(&plaintext).await?;
        }
        self.emit_event(TunnelEvent::PacketReceived {
            bytes: plaintext.len(),
        });
        Ok(Some(plaintext))
    }

    /// Send an encrypted tunnel packet to a peer, starting a rekey
    /// handshake if the session is past its rotation policy.
    pub async fn send_tunnel_packet(
        &self,
        public_key: &[u8; 32],
        plaintext: &[u8],
    ) -> VpnResult<()> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| VpnError::Network("socket not bound".to_string()))?;

        let (datagram, endpoint) = {
            let mut peers = self.peers.write().await;
            let session = peers
                .get_mut(public_key)
                .ok_or_else(|| VpnError::PeerNotFound(format!("{public_key:?}")))?;

            // Rotate the session when policy demands it. The old keys keep
            // working until the new handshake completes.
            let keys = session
                .session_keys
                .as_ref()
                .ok_or_else(|| VpnError::Protocol("session not established".to_string()))?;
            let expired = keys.is_expired(self.config.max_session_age);
            if expired || session.sent_since_rekey > REKEY_PACKET_LIMIT {
                self.rekey_session(session).await?;
            }

            let keys = session
                .session_keys
                .as_ref()
                .ok_or_else(|| VpnError::Protocol("session not established".to_string()))?
                .clone();
            session.send_counter += 1;
            session.sent_since_rekey += 1;
            let sender_index = keys.sending_index;
            let endpoint = session.endpoint;
            let datagram = build_packet(
                sender_index,
                session.send_counter,
                &keys.send_key,
                plaintext,
            )?;
            drop(peers);
            (datagram, endpoint)
        };

        socket
            .send_to(&datagram, endpoint)
            .await
            .map_err(|e| VpnError::Network(format!("send failed: {e}")))?;
        self.emit_event(TunnelEvent::PacketSent {
            bytes: datagram.len(),
        });
        Ok(())
    }

    /// Start a fresh one-shot handshake to rotate transport keys.
    /// Called with the peers write lock held; old keys stay valid until
    /// `handle_response` installs the new ones.
    async fn rekey_session(&self, session: &mut PeerSession) -> VpnResult<()> {
        let index = session.handshake.our_index;
        let mut fresh =
            HandshakeProtocol::new(index, self.keypair.clone(), session.public_key, session.psk)?;
        let init = fresh.create_initiation()?;
        session.handshake = fresh;
        session.sent_since_rekey = 0;
        let data = init.serialize()?;
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| VpnError::Network("socket not bound".to_string()))?;
        socket
            .send_to(&data, session.endpoint)
            .await
            .map_err(|e| VpnError::Network(format!("rekey initiation failed: {e}")))?;
        self.emit_event(TunnelEvent::Rekeyed {
            peer: hex::encode(session.public_key),
        });
        Ok(())
    }

    /// Read a packet from the TUN device and send it to the peer.
    pub async fn forward_from_tun(&self, public_key: &[u8; 32]) -> VpnResult<usize> {
        let tun = self
            .tun
            .as_ref()
            .ok_or_else(|| VpnError::Tun("TUN device not created".to_string()))?;
        let mut tun = tun.lock().await;
        let mut buf = [0u8; 65535];
        let n = tun.read(&mut buf).await?;
        let packet = buf[..n].to_vec();
        drop(tun);
        self.send_tunnel_packet(public_key, &packet).await?;
        Ok(n)
    }

    /// Run the main tunnel loop: read from UDP, forward, keepalive.
    pub async fn run(self: Arc<Self>) -> VpnResult<()> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| VpnError::Network("socket not bound".to_string()))?
            .clone();

        let mut keepalive_timer = interval(Duration::from_secs(self.config.keepalive_interval));
        // Drain pending keepalive tick.
        keepalive_timer.tick().await;

        let engine = self.clone();
        let mut udp_buf = [0u8; 65535];

        loop {
            tokio::select! {
                result = socket.recv_from(&mut udp_buf) => {
                    let (n, src) = result.map_err(|e| VpnError::Network(format!("udp recv failed: {e}")))?;
                    // Own the bytes: the next iteration overwrites udp_buf,
                    // so handing the borrow to a spawned task was a
                    // use-after-overwrite race (this was E0502/E0597).
                    let data = udp_buf[..n].to_vec();
                    let engine = engine.clone();
                    tokio::spawn(async move {
                        let _ = engine.handle_udp_packet(&data, src).await;
                    });
                }
                _ = keepalive_timer.tick() => {
                    let peers = engine.peers.read().await;
                    for session in peers.values() {
                        if let Some(keys) = &session.session_keys {
                            if session.state == TunnelState::Established {
                                if keys.is_expired(engine.config.handshake_timeout) {
                                    continue; // established but silent: rekey path handles it
                                }
                                let ka = session.handshake.create_keepalive().unwrap();
                                let data = ka.serialize().unwrap();
                                let _ = socket.send_to(&data, session.endpoint).await;
                                engine.emit_event(TunnelEvent::KeepaliveSent);
                            }
                        }
                    }
                    drop(peers);
                }
            }
        }
    }

    /// Disconnect from a peer.
    pub async fn disconnect(&self, public_key: &[u8; 32]) -> VpnResult<()> {
        let mut peers = self.peers.write().await;
        if let Some(session) = peers.get_mut(public_key) {
            session.state = TunnelState::Disconnected;
            session.session_keys = None;
        }
        let mut state = self.state.write().await;
        *state = TunnelState::Disconnected;
        drop(state);
        self.emit_event(TunnelEvent::Disconnected {
            peer: hex::encode(public_key),
        });
        Ok(())
    }

    /// List all connected peers and their states.
    pub async fn peers(&self) -> Vec<([u8; 32], TunnelState)> {
        let peers = self.peers.read().await;
        peers.iter().map(|(pk, s)| (*pk, s.state.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanomite_vpn::protocol::Role;

    #[test]
    fn tunnel_state_codes() {
        assert_eq!(TunnelState::Disconnected.code(), 0);
        assert_eq!(TunnelState::Established.code(), 2);
        assert_eq!(TunnelState::Failed("x".to_string()).code(), 4);
    }

    #[test]
    fn config_defaults() {
        let config = VpnConfig::default();
        assert_eq!(config.listen_port, 51820);
        assert_eq!(config.mtu, 1400);
    }

    #[test]
    fn peer_config_fields() {
        let peer = PeerConfig {
            public_key: [0u8; 32],
            endpoint: "127.0.0.1:51820".parse().unwrap(),
            allowed_ips: vec!["0.0.0.0/0".to_string()],
            persistent_keepalive: Some(15),
            preshared_key: None,
        };
        assert_eq!(peer.allowed_ips, vec!["0.0.0.0/0".to_string()]);
        assert_eq!(peer.persistent_keepalive, Some(15));
    }

    #[test]
    fn packet_roundtrip_with_wire_header() {
        // Wire a real initiator/responder pair through the protocol and
        // verify the exact engine packet path: header + seal/open + replay.
        let id_i = KeyPair::generate().expect("I");
        let id_r = KeyPair::generate().expect("R");
        let id_i_pub = id_i.public;
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I proto");
        let mut r = HandshakeProtocol::new(7, id_r, id_i_pub, None).expect("R proto");
        let init = i.create_initiation().expect("init");
        let resp = r.handle_initiation(&init).expect("resp");
        i.handle_response(&resp).expect("handle");
        let keys_i = i.session_keys().expect("I keys");
        let keys_r = r.session_keys().expect("R keys");

        let datagram = build_packet(1, 42, &keys_i.send_key, b"hello").expect("packet");
        assert_eq!(&datagram[..4], PACKET_MAGIC);
        let sender_index = u32::from_le_bytes(datagram[4..8].try_into().unwrap());
        let counter = u64::from_le_bytes(datagram[8..16].try_into().unwrap());
        assert_eq!((sender_index, counter), (1, 42));

        let mut replay = ReplayWindow::new();
        assert!(replay.accept(counter));
        assert!(!replay.accept(counter), "replayed packet must be rejected");
        let pt = keys_r
            .recv_key
            .open(
                &wire_nonce(sender_index, counter),
                &datagram[16..],
                b"vpn-tunnel",
            )
            .expect("open");
        assert_eq!(pt, b"hello");
        let _ = Role::Initiator; // keep import exercised
    }
}
