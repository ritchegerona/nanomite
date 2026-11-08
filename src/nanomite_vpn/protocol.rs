//! WireGuard-inspired handshake and session protocol for NanomiteVPN.
//!
//! A two-message handshake (WireGuard's shape, signatures instead of
//! static X25519 DH):
//!
//! ```text
//! Initiator                              Responder
//!     |-- HandshakeInit ---------------->| verify sig + timestamp
//!     |   {Ei, Ki, ts, sig_i}            | gens (Er, Kr), derives keys
//!     |<-- HandshakeResponse -------------| verify sig + timestamp
//!     |   {Er, Kr, ts, sig_r}            | derives keys
//!        both derive: s1=DH(Ei,Kr), s2=DH(Ki,Er)
//!        transport keys from transcript(s1||s2)
//! ```
//!
//! - `Ei` is the ephemeral, `Ki` the one-shot "static" (fresh per
//!   handshake; ring 0.17 cannot do reusable static X25519 DH — see
//!   `crypto.rs` design notes). Both are burned by the DH, so every
//!   handshake is forward-secret.
//! - Authentication is Ed25519: each side signs its transcript so far and
//!   the peer verifies against the pinned identity public key in config.
//! - Timestamps are checked for freshness (±120s) and monotonicity
//!   (strictly increasing per peer), so captured handshakes cannot be
//!   replayed.
//! - An optional pre-shared key can be mixed into the KDF (config
//!   `preshared_key`) for defense in depth.
//!
//! All message handling is pure and offline-testable: callers transport
//! the serialized messages over the UDP socket.

use crate::nanomite_vpn::{
    crypto::{transcript_mix, EphemeralKey, KeyPair, SessionKey},
    VpnError, VpnResult,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Freshness window for handshake timestamps, in seconds.
pub const TIMESTAMP_FRESHNESS_SECS: u64 = 120;

/// Handshake message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandshakeType {
    Initiation = 1,
    Response = 2,
    Keepalive = 3,
}

fn now_secs() -> VpnResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| VpnError::Protocol(format!("time error: {e}")))
}

/// Freshness + monotonicity gate for handshake timestamps.
fn check_timestamp(ts: u64, last_seen: Option<u64>) -> VpnResult<()> {
    let now = now_secs()?;
    let age = now.abs_diff(ts);
    if age > TIMESTAMP_FRESHNESS_SECS {
        return Err(VpnError::Protocol(format!(
            "handshake timestamp {ts} is {age}s away (limit {TIMESTAMP_FRESHNESS_SECS}s) — replay or clock skew"
        )));
    }
    if let Some(prev) = last_seen {
        if ts <= prev {
            return Err(VpnError::Protocol(
                "handshake timestamp not strictly increasing — replay".to_string(),
            ));
        }
    }
    Ok(())
}

/// Which side of the handshake we are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

/// Directional session keys + peer metadata. `Debug` is redacted
/// (key material must never reach logs).
#[derive(Clone)]
pub struct SessionKeys {
    pub send_key: SessionKey,
    pub recv_key: SessionKey,
    pub sending_index: u32,
    pub receiving_index: u32,
    pub established_at: u64,
    pub peer_public: [u8; 32],
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionKeys")
            .field("sending_index", &self.sending_index)
            .field("receiving_index", &self.receiving_index)
            .field("established_at", &self.established_at)
            .field("peer_public", &self.peer_public)
            .finish()
    }
}

impl SessionKeys {
    /// Derive transport keys from the handshake shared secret + transcript.
    ///
    /// Directional labels are role-aware so the initiator's `send_key`
    /// equals the responder's `recv_key` (and vice versa) without any
    /// negotiation: both sides run the same KDF with role-swapped labels.
    pub fn new(
        shared_secret: &[u8],
        transcript: &[u8; 32],
        role: Role,
        sending_index: u32,
        receiving_index: u32,
        peer_public: [u8; 32],
    ) -> VpnResult<Self> {
        let (slabel, rlabel): (&[u8], &[u8]) = match role {
            Role::Initiator => (b"vpn-i2r", b"vpn-r2i"),
            Role::Responder => (b"vpn-r2i", b"vpn-i2r"),
        };
        let send_material = transcript_mix(transcript, &[slabel, shared_secret]);
        let recv_material = transcript_mix(transcript, &[rlabel, shared_secret]);
        Ok(SessionKeys {
            send_key: SessionKey::derive(&send_material)?,
            recv_key: SessionKey::derive(&recv_material)?,
            sending_index,
            receiving_index,
            established_at: now_secs()?,
            peer_public,
        })
    }

    /// Check if the session has expired (e.g. past the rekey age).
    pub fn is_expired(&self, max_age_secs: u64) -> bool {
        now_secs()
            .unwrap_or_default()
            .saturating_sub(self.established_at)
            > max_age_secs
    }
}

/// Fold two DH outputs + the transcript into the shared secret, then mix
/// the optional pre-shared key on top (WG's PSK round).
fn derive_shared(
    s1: &[u8; 32],
    s2: &[u8; 32],
    transcript: &[u8; 32],
    psk: Option<[u8; 32]>,
) -> [u8; 32] {
    let mut shared = transcript_mix(transcript, &[s1, s2]);
    if let Some(psk) = psk {
        shared = transcript_mix(&shared, &[b"vpn-psk", &psk]);
    }
    shared
}

/// The transcript seed every handshake starts its hash chain from.
fn initial_transcript() -> [u8; 32] {
    transcript_mix(&[0u8; 32], &[b"nanomite-vpn/handshake/v1"])
}

/// A WireGuard-inspired handshake initiation message sent by the initiator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeInit {
    pub handshake_type: HandshakeType,
    pub sender_index: u32,
    /// Sender's Ed25519 identity public key (lets the receiver look the
    /// peer up by pinned identity before verifying).
    pub sender_identity: [u8; 32],
    /// One-shot X25519 ephemeral public key (Ei).
    pub ephemeral_public: [u8; 32],
    /// One-shot X25519 "static" public key (Ki) — fresh per handshake.
    pub static_public: [u8; 32],
    pub timestamp: u64,
    /// Ed25519 signature over the domain-sep bytes below.
    pub signature: Vec<u8>,
}

impl HandshakeInit {
    /// Bytes covered by the signature: domain, sender index, identity,
    /// both public keys, and timestamp. Any field substitution breaks
    /// verification. (Many fields by design: every wire field is signed.)
    #[allow(clippy::too_many_arguments)]
    fn signed_bytes(
        sender_index: u32,
        sender_identity: &[u8; 32],
        ephemeral_public: &[u8; 32],
        static_public: &[u8; 32],
        timestamp: u64,
    ) -> Vec<u8> {
        let mut buf = b"nanomite-vpn/init/v1".to_vec();
        buf.extend_from_slice(&sender_index.to_le_bytes());
        buf.extend_from_slice(sender_identity);
        buf.extend_from_slice(ephemeral_public);
        buf.extend_from_slice(static_public);
        buf.extend_from_slice(&timestamp.to_le_bytes());
        buf
    }

    pub fn serialize(&self) -> VpnResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| VpnError::Protocol(format!("serialize: {e}")))
    }

    pub fn deserialize(data: &[u8]) -> VpnResult<Self> {
        serde_json::from_slice(data).map_err(|e| VpnError::Protocol(format!("deserialize: {e}")))
    }
}

/// A handshake response message sent by the responder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub handshake_type: HandshakeType,
    pub sender_index: u32,
    pub receiver_index: u32,
    /// Responder Ed25519 identity (lets the initiator look up the peer).
    pub sender_identity: [u8; 32],
    /// Responder ephemeral public key (Er).
    pub ephemeral_public: [u8; 32],
    /// Responder "static" public key (Kr) — fresh per handshake.
    pub static_public: [u8; 32],
    pub timestamp: u64,
    pub signature: Vec<u8>,
}

impl HandshakeResponse {
    /// Signed bytes: domain + both sides' indexes/identities/keys +
    /// timestamp. Binds the response to the exact initiation it answers.
    /// (Many fields by design: every wire field is signed.)
    #[allow(clippy::too_many_arguments)]
    fn signed_bytes(
        sender_index: u32,
        receiver_index: u32,
        sender_identity: &[u8; 32],
        ephemeral_public: &[u8; 32],
        static_public: &[u8; 32],
        timestamp: u64,
        init_ephemeral: &[u8; 32],
        init_static: &[u8; 32],
    ) -> Vec<u8> {
        let mut buf = b"nanomite-vpn/resp/v1".to_vec();
        buf.extend_from_slice(&sender_index.to_le_bytes());
        buf.extend_from_slice(&receiver_index.to_le_bytes());
        buf.extend_from_slice(sender_identity);
        buf.extend_from_slice(ephemeral_public);
        buf.extend_from_slice(static_public);
        buf.extend_from_slice(&timestamp.to_le_bytes());
        buf.extend_from_slice(init_ephemeral);
        buf.extend_from_slice(init_static);
        buf
    }

    pub fn serialize(&self) -> VpnResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| VpnError::Protocol(format!("serialize: {e}")))
    }

    pub fn deserialize(data: &[u8]) -> VpnResult<Self> {
        serde_json::from_slice(data).map_err(|e| VpnError::Protocol(format!("deserialize: {e}")))
    }

    /// Wire discriminant so the receive path can dispatch before
    /// deserializing (`handshake_type` is JSON field 0 in practice; used
    /// for logging only).
    pub fn wire_type(&self) -> HandshakeType {
        self.handshake_type
    }
}

/// A keepalive message to maintain NAT bindings and detect liveness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keepalive {
    pub handshake_type: HandshakeType,
    pub sender_index: u32,
    pub timestamp: u64,
}

impl Keepalive {
    pub fn new(sender_index: u32) -> Self {
        Keepalive {
            handshake_type: HandshakeType::Keepalive,
            sender_index,
            timestamp: now_secs().unwrap_or_default(),
        }
    }

    pub fn serialize(&self) -> VpnResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| VpnError::Protocol(format!("serialize: {e}")))
    }

    pub fn deserialize(data: &[u8]) -> VpnResult<Self> {
        serde_json::from_slice(data).map_err(|e| VpnError::Protocol(format!("deserialize: {e}")))
    }
}

/// The state of a handshake session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    None,
    InitiationSent,
    ResponseSent,
    Established,
    Failed,
}

/// Handshake protocol handler for a single peer.
///
/// Invariants:
/// - State transitions are linear; any out-of-order message fails closed.
/// - Our ephemeral halves are consumed by the DH and leave `None` behind —
///   a session can never be re-derived accidentally.
/// - Timestamps must be fresh and strictly increasing per peer.
pub struct HandshakeProtocol {
    pub state: HandshakeState,
    /// Our sender index (u32) for tunnel packets.
    pub our_index: u32,
    pub their_index: Option<u32>,
    /// Our Ed25519 identity (durable). Cloned from config.
    our_identity: KeyPair,
    /// Peer's pinned Ed25519 public key; signatures verify against it.
    expected_peer_identity: [u8; 32],
    /// Optional pre-shared key mixed into the KDF.
    psk: Option<[u8; 32]>,
    /// Initiator side: our one-shot halves, consumed by `handle_response`.
    our_ephemeral: Option<EphemeralKey>,
    our_static: Option<EphemeralKey>,
    /// Their one-shot publics on the initiator side.
    their_ephemeral_public: Option<[u8; 32]>,
    their_static_public: Option<[u8; 32]>,
    last_peer_timestamp: Option<u64>,
    /// The timestamp we put in our own initiation (initiator side) —
    /// part of the canonical transcript; stored explicitly rather than
    /// borrowed from `last_activity`, which other paths also update.
    pending_init_ts: Option<u64>,
    pub last_activity: u64,
    session_keys: Option<SessionKeys>,
}

impl HandshakeProtocol {
    pub fn new(
        our_index: u32,
        our_identity: KeyPair,
        expected_peer_identity: [u8; 32],
        psk: Option<[u8; 32]>,
    ) -> VpnResult<Self> {
        Ok(HandshakeProtocol {
            state: HandshakeState::None,
            our_index,
            their_index: None,
            our_identity,
            expected_peer_identity,
            psk,
            our_ephemeral: None,
            our_static: None,
            their_ephemeral_public: None,
            their_static_public: None,
            last_peer_timestamp: None,
            pending_init_ts: None,
            last_activity: now_secs()?,
            session_keys: None,
        })
    }

    /// Create a signed initiation message to send to a peer.
    pub fn create_initiation(&mut self) -> VpnResult<HandshakeInit> {
        if self.state != HandshakeState::None {
            return Err(VpnError::Protocol(
                "handshake already in progress".to_string(),
            ));
        }
        let ephemeral = EphemeralKey::generate()?;
        let static_ = EphemeralKey::generate()?;
        let timestamp = now_secs()?;
        let my_identity = self.our_identity.public;
        let signed = HandshakeInit::signed_bytes(
            self.our_index,
            &my_identity,
            &ephemeral.public_bytes(),
            &static_.public_bytes(),
            timestamp,
        );
        let signature = self.our_identity.sign(&signed)?.to_vec();

        let init = HandshakeInit {
            handshake_type: HandshakeType::Initiation,
            sender_index: self.our_index,
            sender_identity: my_identity,
            ephemeral_public: ephemeral.public_bytes(),
            static_public: static_.public_bytes(),
            timestamp,
            signature,
        };
        self.our_ephemeral = Some(ephemeral);
        self.our_static = Some(static_);
        self.pending_init_ts = Some(timestamp);
        self.state = HandshakeState::InitiationSent;
        self.last_activity = timestamp;
        Ok(init)
    }

    /// Handle an incoming initiation and create a signed response.
    /// Derives this side's transport keys eagerly (DH halves burned here).
    pub fn handle_initiation(&mut self, init: &HandshakeInit) -> VpnResult<HandshakeResponse> {
        if self.state != HandshakeState::None {
            return Err(VpnError::Protocol("out-of-order initiation".to_string()));
        }
        if init.sender_identity != self.expected_peer_identity {
            return Err(VpnError::Protocol(
                "initiation from unknown identity".to_string(),
            ));
        }
        let signed = HandshakeInit::signed_bytes(
            init.sender_index,
            &init.sender_identity,
            &init.ephemeral_public,
            &init.static_public,
            init.timestamp,
        );
        if !KeyPair::verify_signature(&self.expected_peer_identity, &signed, &init.signature[..]) {
            return Err(VpnError::Protocol(
                "initiation signature invalid".to_string(),
            ));
        }
        check_timestamp(init.timestamp, self.last_peer_timestamp)?;
        self.last_peer_timestamp = Some(init.timestamp);

        let ephemeral = EphemeralKey::generate()?;
        let static_ = EphemeralKey::generate()?;
        let timestamp = now_secs()?;
        let resp_eph_pub = ephemeral.public_bytes();
        let resp_sta_pub = static_.public_bytes();
        let signed_resp = HandshakeResponse::signed_bytes(
            self.our_index,
            init.sender_index,
            &self.our_identity.public,
            &resp_eph_pub,
            &resp_sta_pub,
            timestamp,
            &init.ephemeral_public,
            &init.static_public,
        );
        let signature = self.our_identity.sign(&signed_resp)?.to_vec();

        // s1 = DH(Ei, Kr): this side agrees (Kr, Ei) — burns Kr.
        let s1 = static_.agree(&init.ephemeral_public)?;
        // s2 = DH(Ki, Er): this side agrees (Er, Ki) — burns Er.
        let s2 = ephemeral.agree(&init.static_public)?;

        // Canonical transcript both sides must fold identically:
        // [I_index, Ei, Ki, I_ts, R_index, Er, Kr, R_ts].
        let transcript = transcript_mix(
            &initial_transcript(),
            &[
                &init.sender_index.to_le_bytes(),
                &init.ephemeral_public,
                &init.static_public,
                &init.timestamp.to_le_bytes(),
                &self.our_index.to_le_bytes(),
                &resp_eph_pub,
                &resp_sta_pub,
                &timestamp.to_le_bytes(),
            ],
        );
        let shared = derive_shared(&s1, &s2, &transcript, self.psk);
        self.session_keys = Some(SessionKeys::new(
            &shared,
            &transcript,
            Role::Responder,
            self.our_index,
            init.sender_index,
            self.expected_peer_identity,
        )?);

        self.their_index = Some(init.sender_index);
        self.their_ephemeral_public = Some(init.ephemeral_public);
        self.their_static_public = Some(init.static_public);
        self.state = HandshakeState::ResponseSent;
        self.last_activity = timestamp;

        Ok(HandshakeResponse {
            handshake_type: HandshakeType::Response,
            sender_index: self.our_index,
            receiver_index: init.sender_index,
            sender_identity: self.our_identity.public,
            ephemeral_public: resp_eph_pub,
            static_public: resp_sta_pub,
            timestamp,
            signature,
        })
    }

    /// Handle an incoming response and derive session keys.
    pub fn handle_response(&mut self, response: &HandshakeResponse) -> VpnResult<()> {
        if self.state != HandshakeState::InitiationSent {
            return Err(VpnError::Protocol("out-of-order response".to_string()));
        }
        if response.receiver_index != self.our_index {
            return Err(VpnError::Protocol(
                "response addressed to another index".to_string(),
            ));
        }
        let eph_i = self
            .our_ephemeral
            .as_ref()
            .map(|k| k.public_bytes())
            .ok_or_else(|| VpnError::Protocol("missing initiator ephemeral".to_string()))?;
        let sta_i = self
            .our_static
            .as_ref()
            .map(|k| k.public_bytes())
            .ok_or_else(|| VpnError::Protocol("missing initiator static".to_string()))?;
        if response.sender_identity != self.expected_peer_identity {
            return Err(VpnError::Protocol(
                "response from unknown identity".to_string(),
            ));
        }
        let signed = HandshakeResponse::signed_bytes(
            response.sender_index,
            response.receiver_index,
            &response.sender_identity,
            &response.ephemeral_public,
            &response.static_public,
            response.timestamp,
            &eph_i,
            &sta_i,
        );
        if !KeyPair::verify_signature(
            &self.expected_peer_identity,
            &signed,
            &response.signature[..],
        ) {
            return Err(VpnError::Protocol("response signature invalid".to_string()));
        }
        check_timestamp(response.timestamp, self.last_peer_timestamp)?;
        self.last_peer_timestamp = Some(response.timestamp);

        // s1 = DH(Ei, Kr): agree (Ei, Kr) — burns Ei.
        let s1 = self
            .our_ephemeral
            .take()
            .ok_or_else(|| VpnError::Protocol("ephemeral replay".to_string()))?
            .agree(&response.static_public)?;
        // s2 = DH(Ki, Er): agree (Ki, Er) — burns Ki.
        let s2 = self
            .our_static
            .take()
            .ok_or_else(|| VpnError::Protocol("static replay".to_string()))?
            .agree(&response.ephemeral_public)?;

        // Identical canonical sequence as the responder:
        // [I_index, Ei, Ki, I_ts, R_index, Er, Kr, R_ts].
        let init_timestamp = self
            .pending_init_ts
            .ok_or_else(|| VpnError::Protocol("missing initiation timestamp".to_string()))?;
        let transcript = transcript_mix(
            &initial_transcript(),
            &[
                &self.our_index.to_le_bytes(),
                &eph_i,
                &sta_i,
                &init_timestamp.to_le_bytes(),
                &response.sender_index.to_le_bytes(),
                &response.ephemeral_public,
                &response.static_public,
                &response.timestamp.to_le_bytes(),
            ],
        );
        let shared = derive_shared(&s1, &s2, &transcript, self.psk);
        self.session_keys = Some(SessionKeys::new(
            &shared,
            &transcript,
            Role::Initiator,
            self.our_index,
            response.sender_index,
            self.expected_peer_identity,
        )?);

        self.their_index = Some(response.sender_index);
        self.their_ephemeral_public = Some(response.ephemeral_public);
        self.their_static_public = Some(response.static_public);
        self.state = HandshakeState::Established;
        self.last_activity = response.timestamp;
        Ok(())
    }

    /// Session keys once established (cloned; engine holds the reference).
    pub fn session_keys(&self) -> Option<SessionKeys> {
        self.session_keys.clone()
    }

    /// Create a keepalive message.
    pub fn create_keepalive(&self) -> VpnResult<Keepalive> {
        Ok(Keepalive::new(self.our_index))
    }

    /// Check if the handshake has timed out (no activity for `timeout_secs`).
    pub fn is_timed_out(&self, timeout_secs: u64) -> bool {
        now_secs()
            .unwrap_or_default()
            .saturating_sub(self.last_activity)
            > timeout_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanomite_vpn::crypto;

    fn identities() -> (KeyPair, KeyPair) {
        (
            KeyPair::generate().expect("I"),
            KeyPair::generate().expect("R"),
        )
    }

    #[test]
    fn full_handshake_derives_matching_keys() {
        let (id_i, id_r) = identities();
        let id_i_pub = id_i.public;
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I proto");
        let mut r = HandshakeProtocol::new(7, id_r, id_i_pub, None).expect("R proto");

        let init = i.create_initiation().expect("init");
        let resp = r.handle_initiation(&init).expect("response");
        i.handle_response(&resp).expect("handle response");

        assert_eq!(i.state, HandshakeState::Established);
        assert_eq!(r.state, HandshakeState::ResponseSent);
        assert_eq!(i.their_index, Some(7));
        assert_eq!(r.their_index, Some(1));

        let keys_i = i.session_keys().expect("I keys");
        let keys_r = r.session_keys().expect("R keys");
        // Directional equality: initiator send == responder recv.
        let msg = b"tunnel traffic";
        let ct = keys_i
            .send_key
            .seal(&crypto::wire_nonce(1, 1), msg, b"tunnel")
            .expect("seal I");
        let pt = keys_r
            .recv_key
            .open(&crypto::wire_nonce(1, 1), &ct, b"tunnel")
            .expect("open R");
        assert_eq!(pt, msg);
        let ct_r = keys_r
            .send_key
            .seal(&crypto::wire_nonce(7, 1), msg, b"tunnel")
            .expect("seal R");
        let pt_i = keys_i
            .recv_key
            .open(&crypto::wire_nonce(7, 1), &ct_r, b"tunnel")
            .expect("open I");
        assert_eq!(pt_i, msg);
    }

    #[test]
    fn tampered_initiation_rejected() {
        let (id_i, id_r) = identities();
        let id_i_pub = id_i.public;
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I");
        let mut r = HandshakeProtocol::new(7, id_r, id_i_pub, None).expect("R");
        let mut init = i.create_initiation().expect("init");
        init.static_public[0] ^= 1;
        assert!(r.handle_initiation(&init).is_err());
    }

    #[test]
    fn wrong_identity_rejected() {
        let (id_i, id_r) = identities();
        let attacker = KeyPair::generate().expect("attacker");
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I");
        // Responder pins a different identity: signature must fail.
        let mut r = HandshakeProtocol::new(7, id_r, attacker.public, None).expect("R");
        let init = i.create_initiation().expect("init");
        assert!(r.handle_initiation(&init).is_err());
    }

    #[test]
    fn stale_timestamp_rejected() {
        let (id_i, id_r) = identities();
        let id_i_pub = id_i.public;
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I");
        let mut r = HandshakeProtocol::new(7, id_r, id_i_pub, None).expect("R");
        let mut init = i.create_initiation().expect("init");
        init.timestamp = init.timestamp.saturating_sub(10_000);
        // Re-sign over the stale timestamp so only freshness can reject it.
        init.signature = i
            .our_identity
            .sign(&HandshakeInit::signed_bytes(
                init.sender_index,
                &init.sender_identity,
                &init.ephemeral_public,
                &init.static_public,
                init.timestamp,
            ))
            .expect("sign")
            .to_vec();
        assert!(r.handle_initiation(&init).is_err());
    }

    #[test]
    fn out_of_order_response_rejected() {
        let (id_i, id_r) = identities();
        let id_i_pub = id_i.public;
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i.clone(), id_r_pub, None).expect("I");
        let mut r = HandshakeProtocol::new(7, id_r, id_i_pub, None).expect("R");
        let init = i.create_initiation().expect("init");
        let resp = r.handle_initiation(&init).expect("response");
        // A second initiation with no fresh state must fail closed.
        assert!(r.handle_initiation(&init).is_err());
        // Response to a session that never started.
        let mut fresh = HandshakeProtocol::new(1, id_i.clone(), id_r_pub, None).expect("fresh");
        assert!(fresh.handle_response(&resp).is_err());
    }

    #[test]
    fn psk_mixing_changes_keys() {
        let (id_i, id_r) = identities();
        let psk1 = Some([0x01u8; 32]);
        let psk2 = Some([0x02u8; 32]);
        let run = |psk| {
            let id_i_pub2 = id_i.public;
            let id_r_pub2 = id_r.public;
            let mut i = HandshakeProtocol::new(1, id_i.clone(), id_r_pub2, psk).expect("I");
            let mut r = HandshakeProtocol::new(7, id_r.clone(), id_i_pub2, psk).expect("R");
            let init = i.create_initiation().expect("init");
            let resp = r.handle_initiation(&init).expect("resp");
            i.handle_response(&resp).expect("handle");
            i.session_keys().expect("keys")
        };
        let k1 = run(psk1);
        let k2 = run(psk2);
        let msg = b"x";
        let ct = k1
            .send_key
            .seal(&crypto::wire_nonce(1, 1), msg, b"t")
            .expect("seal");
        // Different PSK => different transport keys => opening must fail.
        assert!(k2
            .recv_key
            .open(&crypto::wire_nonce(1, 1), &ct, b"t")
            .is_err());
    }

    #[test]
    fn keepalive_roundtrip() {
        let ka = Keepalive::new(42);
        let data = ka.serialize().expect("serialize");
        let ka2 = Keepalive::deserialize(&data).expect("deserialize");
        assert_eq!(ka2.sender_index, 42);
    }

    #[test]
    fn message_serde_roundtrip() {
        let (id_i, id_r) = identities();
        let id_r_pub = id_r.public;
        let mut i = HandshakeProtocol::new(1, id_i, id_r_pub, None).expect("I");
        let init = i.create_initiation().expect("init");
        let bytes = init.serialize().expect("serialize");
        let init2 = HandshakeInit::deserialize(&bytes).expect("deserialize");
        assert_eq!(init2.sender_index, init.sender_index);
        assert_eq!(init2.ephemeral_public, init.ephemeral_public);
        assert_eq!(init2.signature, init.signature);
    }
}
