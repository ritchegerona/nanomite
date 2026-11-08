//! Tor v3 onion services.
//!
//! Implements the parts that can be done safely offline: ED25519-V3 key
//! handling, `.onion` v3 address derivation (rend-spec-v3), and building
//! `ADD_ONION` / `DEL_ONION` control commands. The daemon itself publishes the
//! service and returns the authoritative `ServiceID`.
//!
//! Address derivation: `base32(pubkey ‖ checksum ‖ 0x03)` where
//! `checksum = SHA3-256(".onion checksum" ‖ pubkey ‖ 0x03)[..2]`.

use crate::nanomite_tor::config;
use crate::nanomite_tor::{TorError, TorResult};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use sha3::{Digest, Sha3_256};
use std::path::Path;

/// The only private-key algorithm tor exposes for v3 services.
pub const ONION_KEY_ALG: &str = "ED25519-V3";

/// Version byte folded into the address and checksum.
const ONION_V3_VERSION: u8 = 0x03;

/// Domain-separation string for the address checksum.
const ONION_CHECKSUM_PREFIX: &[u8] = b".onion checksum";

const BASE32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// An ED25519 key pair used for a v3 onion service.
///
/// The seed is secret; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct OnionKey {
    seed: [u8; 32],
    public: [u8; 32],
}

impl std::fmt::Debug for OnionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnionKey")
            .field("address", &self.address())
            .finish_non_exhaustive()
    }
}

impl OnionKey {
    /// Generate a fresh key pair using the OS CSPRNG.
    pub fn generate() -> TorResult<Self> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)
            .map_err(|e| TorError::Onion(format!("entropy unavailable: {e}")))?;
        Self::from_seed(&seed)
    }

    /// Rebuild from a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> TorResult<Self> {
        let pair = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|e| TorError::Onion(format!("invalid seed: {e}")))?;
        let mut public = [0u8; 32];
        public.copy_from_slice(pair.public_key().as_ref());
        Ok(Self {
            seed: *seed,
            public,
        })
    }

    /// The 32-byte public key.
    pub fn public(&self) -> &[u8; 32] {
        &self.public
    }

    /// The `.onion` v3 address (without a scheme or port).
    pub fn address(&self) -> String {
        onion_v3_address(&self.public)
    }

    /// Tor's `ED25519-V3:<base64>` key blob (`seed ‖ public`, 64 bytes).
    pub fn to_key_blob(&self) -> String {
        let mut raw = Vec::with_capacity(64);
        raw.extend_from_slice(&self.seed);
        raw.extend_from_slice(&self.public);
        format!("{ONION_KEY_ALG}:{}", BASE64.encode(raw))
    }

    /// Parse a tor key blob, verifying that the embedded public key matches the
    /// seed (a mismatched blob would silently yield the wrong service id).
    pub fn from_key_blob(blob: &str) -> TorResult<Self> {
        let (alg, encoded) = blob
            .split_once(':')
            .ok_or_else(|| TorError::Onion("key blob missing algorithm prefix".into()))?;
        if alg.trim() != ONION_KEY_ALG {
            return Err(TorError::Onion(format!(
                "unsupported onion key algorithm: {alg}"
            )));
        }
        let raw = BASE64
            .decode(encoded.trim())
            .map_err(|e| TorError::Onion(format!("key blob is not base64: {e}")))?;
        if raw.len() != 64 {
            return Err(TorError::Onion(format!(
                "key blob must be 64 bytes, got {}",
                raw.len()
            )));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&raw[..32]);
        let mut public = [0u8; 32];
        public.copy_from_slice(&raw[32..]);
        let key = Self::from_seed(&seed)?;
        if key.public != public {
            return Err(TorError::Onion(
                "key blob public key does not match its seed".into(),
            ));
        }
        Ok(key)
    }

    /// Write the key blob to `path` with owner-only permissions.
    pub fn write_key_file(&self, path: &Path) -> TorResult<()> {
        config::write_private_file(path, self.to_key_blob().as_bytes())
            .map_err(|e| TorError::Onion(format!("cannot write key file: {e}")))
    }
}

/// Forward one virtual port to a local target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnionPort {
    pub virtual_port: u16,
    /// `None` forwards to the same port on localhost.
    pub target: Option<String>,
}

impl OnionPort {
    pub fn new(virtual_port: u16, target: impl Into<String>) -> Self {
        Self {
            virtual_port,
            target: Some(target.into()),
        }
    }

    /// Forward to the same port on localhost.
    pub fn local(virtual_port: u16) -> Self {
        Self {
            virtual_port,
            target: None,
        }
    }

    fn token(&self) -> String {
        match self.target.as_deref().map(str::trim) {
            Some(target) if !target.is_empty() => format!("{},{}", self.virtual_port, target),
            _ => self.virtual_port.to_string(),
        }
    }
}

/// `ADD_ONION` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnionFlag {
    /// Keep the service after the control connection closes.
    Detach,
    /// Do not return the private key.
    DiscardPk,
    /// Require client authorization.
    BasicAuth,
    /// Do not use entry guards for this service.
    NonAnonymous,
}

impl OnionFlag {
    fn token(self) -> &'static str {
        match self {
            OnionFlag::Detach => "Detach",
            OnionFlag::DiscardPk => "DiscardPK",
            OnionFlag::BasicAuth => "BasicAuth",
            OnionFlag::NonAnonymous => "NonAnonymous",
        }
    }
}

/// Which key the service should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnionKeyInput {
    /// Let tor generate a fresh ED25519-V3 key.
    New,
    /// Publish with a caller-supplied key.
    Existing(OnionKey),
}

/// A full `ADD_ONION` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnionRequest {
    pub key: OnionKeyInput,
    pub ports: Vec<OnionPort>,
    pub flags: Vec<OnionFlag>,
    pub max_streams: Option<u32>,
}

impl OnionRequest {
    /// A detached service on a fresh key.
    pub fn new(ports: Vec<OnionPort>) -> Self {
        Self {
            key: OnionKeyInput::New,
            ports,
            flags: vec![OnionFlag::Detach],
            max_streams: None,
        }
    }

    /// Build the control command line (without a trailing newline).
    pub fn to_command(&self) -> String {
        let key = match &self.key {
            OnionKeyInput::New => format!("NEW:{ONION_KEY_ALG}"),
            OnionKeyInput::Existing(key) => key.to_key_blob(),
        };
        let mut parts = vec!["ADD_ONION".to_string(), key];
        for port in &self.ports {
            parts.push(format!("Port={}", port.token()));
        }
        if !self.flags.is_empty() {
            let flags = self
                .flags
                .iter()
                .map(|f| f.token())
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("Flags={flags}"));
        }
        if let Some(max) = self.max_streams {
            parts.push(format!("MaxStreams={max}"));
        }
        parts.join(" ")
    }
}

/// What tor returned for an `ADD_ONION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnionResponse {
    /// The 56-character service id (no `.onion` suffix).
    pub service_id: String,
    /// Present unless `DiscardPK` was requested.
    pub private_key: Option<String>,
    /// Client auth descriptors, if requested.
    pub client_auth: Vec<String>,
}

impl OnionResponse {
    /// Build from control reply key/value pairs.
    pub fn from_pairs(pairs: &[(String, String)]) -> TorResult<Self> {
        let mut service_id = None;
        let mut private_key = None;
        let mut client_auth = Vec::new();
        for (key, value) in pairs {
            match key.to_ascii_lowercase().as_str() {
                "serviceid" => service_id = Some(value.clone()),
                "privatekey" => private_key = Some(value.clone()),
                "clientauth" => client_auth.push(value.clone()),
                _ => {}
            }
        }
        let service_id = service_id
            .ok_or_else(|| TorError::Onion("ADD_ONION reply missing ServiceID".into()))?;
        Ok(Self {
            service_id,
            private_key,
            client_auth,
        })
    }

    /// The full `…​.onion` address.
    pub fn address(&self) -> String {
        format!("{}.onion", self.service_id)
    }
}

/// Derive the v3 `.onion` address for a 32-byte ED25519 public key.
pub fn onion_v3_address(public: &[u8; 32]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(ONION_CHECKSUM_PREFIX);
    hasher.update(public);
    hasher.update([ONION_V3_VERSION]);
    let digest = hasher.finalize();
    let checksum = &digest[..2];

    let mut raw = Vec::with_capacity(35);
    raw.extend_from_slice(public);
    raw.extend_from_slice(checksum);
    raw.push(ONION_V3_VERSION);
    format!("{}.onion", base32_lower(&raw))
}

/// RFC 4648 base32, lowercase, no padding (the only form tor uses).
pub fn base32_lower(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut buffer: u16 = 0;
    let mut bits: u8 = 0;
    for &byte in data {
        buffer = (buffer << 8) | u16::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0x1f) as usize;
            out.push(BASE32_ALPHABET[index] as char);
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(BASE32_ALPHABET[index] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_rfc4648_vectors() {
        assert_eq!(base32_lower(b""), "");
        assert_eq!(base32_lower(b"f"), "my");
        assert_eq!(base32_lower(b"fo"), "mzxq");
        assert_eq!(base32_lower(b"foo"), "mzxw6");
        assert_eq!(base32_lower(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn onion_address_matches_independent_vector() {
        // Generated independently with Python's hashlib.sha3_256 + base64.b32encode.
        let public: [u8; 32] = std::array::from_fn(|i| i as u8);
        assert_eq!(
            onion_v3_address(&public),
            "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead.onion"
        );
    }

    #[test]
    fn generated_key_has_valid_address_shape() {
        let key = OnionKey::generate().expect("key");
        let address = key.address();
        let service_id = address.strip_suffix(".onion").expect("suffix");
        assert_eq!(service_id.len(), 56);
        assert!(service_id.bytes().all(|b| BASE32_ALPHABET.contains(&b)));
    }

    #[test]
    fn key_blob_roundtrips() {
        let key = OnionKey::generate().expect("key");
        let blob = key.to_key_blob();
        assert!(blob.starts_with("ED25519-V3:"));
        let parsed = OnionKey::from_key_blob(&blob).expect("parse");
        assert_eq!(parsed.public(), key.public());
        assert_eq!(parsed.address(), key.address());
    }

    #[test]
    fn key_blob_rejects_mismatched_public_key() {
        let key = OnionKey::generate().expect("key");
        let other = OnionKey::generate().expect("other");
        let mut raw = Vec::new();
        raw.extend_from_slice(&key.seed);
        raw.extend_from_slice(other.public());
        let blob = format!("ED25519-V3:{}", BASE64.encode(raw));
        assert!(OnionKey::from_key_blob(&blob).is_err());
    }

    #[test]
    fn key_blob_rejects_wrong_algorithm() {
        assert!(OnionKey::from_key_blob("RSA1024:AAAA").is_err());
        assert!(OnionKey::from_key_blob("no-colon").is_err());
    }

    #[test]
    fn debug_does_not_leak_seed() {
        let key = OnionKey::generate().expect("key");
        let text = format!("{key:?}");
        assert!(text.contains("OnionKey"));
        assert!(!text.contains(&hex_fragment(key.seed)));
    }

    fn hex_fragment(bytes: [u8; 32]) -> String {
        bytes[..4].iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn add_onion_command_for_new_key() {
        let request = OnionRequest {
            key: OnionKeyInput::New,
            ports: vec![OnionPort::new(80, "127.0.0.1:8080"), OnionPort::local(22)],
            flags: vec![OnionFlag::Detach, OnionFlag::DiscardPk],
            max_streams: Some(4),
        };
        assert_eq!(
            request.to_command(),
            "ADD_ONION NEW:ED25519-V3 Port=80,127.0.0.1:8080 Port=22 Flags=Detach,DiscardPK MaxStreams=4"
        );
    }

    #[test]
    fn add_onion_command_for_existing_key() {
        let key = OnionKey::from_seed(&[7u8; 32]).expect("key");
        let request = OnionRequest {
            key: OnionKeyInput::Existing(key.clone()),
            ports: vec![OnionPort::new(443, "127.0.0.1:8443")],
            flags: Vec::new(),
            max_streams: None,
        };
        let command = request.to_command();
        assert!(command.starts_with(&format!("ADD_ONION {}", key.to_key_blob())));
        assert!(command.contains("Port=443,127.0.0.1:8443"));
        assert!(!command.contains("Flags="));
    }

    #[test]
    fn parses_add_onion_reply() {
        let response = OnionResponse::from_pairs(&[
            ("ServiceID".into(), "abc".into()),
            ("PrivateKey".into(), "ED25519-V3:xyz".into()),
        ])
        .expect("response");
        assert_eq!(response.service_id, "abc");
        assert_eq!(response.address(), "abc.onion");
        assert_eq!(response.private_key.as_deref(), Some("ED25519-V3:xyz"));
    }

    #[test]
    fn add_onion_reply_requires_service_id() {
        assert!(OnionResponse::from_pairs(&[("PrivateKey".into(), "k".into())]).is_err());
    }
}
