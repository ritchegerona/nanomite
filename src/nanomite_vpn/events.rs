//! Event system for NanomiteVPN.
//!
//! Emits structured events that can be consumed by nanomite-core's event
//! bus, the TUI, or external monitoring.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// A VPN event emitted during tunnel operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VpnEvent {
    /// The VPN engine started.
    Started {
        local_public_key: String,
        local_fingerprint: String,
        listen_port: u16,
    },
    /// The VPN engine stopped.
    Stopped,
    /// A connection attempt to a peer began.
    Connecting { peer: String, endpoint: String },
    /// A connection to a peer was established.
    Connected {
        peer: String,
        fingerprint: String,
        bytes_sent: u64,
        bytes_received: u64,
    },
    /// A connection to a peer was lost.
    Disconnected {
        peer: String,
        reason: Option<String>,
    },
    /// A connection attempt failed.
    ConnectionFailed { peer: String, error: String },
    /// An encrypted packet was sent.
    PacketSent { peer: String, bytes: usize },
    /// An encrypted packet was received.
    PacketReceived { peer: String, bytes: usize },
    /// A keepalive was sent.
    KeepaliveSent { peer: String },
    /// A keepalive was received.
    KeepaliveReceived { peer: String },
    /// A session was rekeyed.
    Rekeyed { peer: String },
    /// A DNS leak was detected.
    DnsLeakDetected { domain: String, resolver: String },
    /// Kill switch was activated.
    KillSwitchActivated,
    /// Kill switch was deactivated.
    KillSwitchDeactivated,
    /// Kill switch blocked a packet.
    KillSwitchBlocked { destination: String, port: u16 },
    /// NAT hole punching succeeded.
    NatHolePunched { peer: String, public_addr: String },
    /// NAT hole punching failed.
    NatHolePunchFailed { peer: String, nat_type: String },
    /// A configuration error occurred.
    ConfigError { message: String },
    /// A crypto error occurred.
    CryptoError { message: String },
}

/// Timestamp for events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTimestamp {
    pub unix_secs: u64,
    pub iso_8601: String,
}

impl EventTimestamp {
    pub fn now() -> Self {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let iso = chrono::Utc::now().to_rfc3339();
        EventTimestamp {
            unix_secs: now,
            iso_8601: iso,
        }
    }
}

/// An event with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnEventEnvelope {
    pub event_id: uuid::Uuid,
    pub timestamp: EventTimestamp,
    pub event: VpnEvent,
}

impl VpnEventEnvelope {
    pub fn new(event: VpnEvent) -> Self {
        VpnEventEnvelope {
            event_id: uuid::Uuid::new_v4(),
            timestamp: EventTimestamp::now(),
            event,
        }
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_roundtrip() {
        let event = VpnEvent::Connected {
            peer: "abc123".to_string(),
            fingerprint: "deadbeef".to_string(),
            bytes_sent: 0,
            bytes_received: 0,
        };
        let envelope = VpnEventEnvelope::new(event);
        let json = envelope.to_json().expect("serialize");
        let _envelope2: VpnEventEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert!(!json.is_empty());
    }

    #[test]
    fn event_variants_serialize() {
        let events = vec![
            VpnEvent::Started {
                local_public_key: "key".to_string(),
                local_fingerprint: "fp".to_string(),
                listen_port: 51820,
            },
            VpnEvent::Stopped,
            VpnEvent::KillSwitchActivated,
            VpnEvent::KillSwitchDeactivated,
        ];

        for event in events {
            let envelope = VpnEventEnvelope::new(event);
            let _ = envelope.to_json().expect("serialize");
        }
    }

    #[test]
    fn timestamp_now() {
        let ts = EventTimestamp::now();
        assert!(ts.unix_secs > 0);
        assert!(!ts.iso_8601.is_empty());
    }
}
