//! Structured Tor events for logging, UI and JSONL audit trails.
//!
//! Mirrors the VPN module's envelope: a stable UUID, a wall-clock timestamp and
//! the typed event. No I/O here.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A notable Tor lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TorEvent {
    /// A managed daemon was launched.
    Started { socks_port: u16, control_port: u16 },
    /// Bootstrap progress changed.
    Bootstrapped { percent: u8, tag: String },
    /// The SOCKS proxy is ready for traffic.
    Connected { socks_addr: String },
    /// A new circuit was requested (`SIGNAL NEWNYM`).
    Newnym,
    /// A v3 onion service was published.
    OnionPublished { service_id: String },
    /// A circuit was built.
    CircuitBuilt { circuit_id: u32, hops: usize },
    /// The daemon stopped.
    Stopped,
    /// Something failed.
    Error { message: String },
}

/// Wall-clock timestamp in both machine and human form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTimestamp {
    pub unix_secs: u64,
    pub iso_8601: String,
}

impl EventTimestamp {
    pub fn now() -> Self {
        let unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            unix_secs,
            iso_8601: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// An event with identity and time metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorEventEnvelope {
    pub event_id: uuid::Uuid,
    pub timestamp: EventTimestamp,
    pub event: TorEvent,
}

impl TorEventEnvelope {
    pub fn new(event: TorEvent) -> Self {
        Self {
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
    fn envelope_roundtrips_as_json() {
        let envelope = TorEventEnvelope::new(TorEvent::Bootstrapped {
            percent: 100,
            tag: "done".into(),
        });
        let json = envelope.to_json().expect("serialize");
        let parsed: TorEventEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, envelope);
        assert!(parsed.timestamp.unix_secs > 0);
        assert!(!parsed.timestamp.iso_8601.is_empty());
    }

    #[test]
    fn every_variant_serializes() {
        let events = vec![
            TorEvent::Started {
                socks_port: 9050,
                control_port: 9051,
            },
            TorEvent::Connected {
                socks_addr: "127.0.0.1:9050".into(),
            },
            TorEvent::Newnym,
            TorEvent::OnionPublished {
                service_id: "abc".into(),
            },
            TorEvent::CircuitBuilt {
                circuit_id: 3,
                hops: 3,
            },
            TorEvent::Stopped,
            TorEvent::Error {
                message: "boom".into(),
            },
        ];
        for event in events {
            let json = TorEventEnvelope::new(event).to_json().expect("serialize");
            assert!(json.contains("timestamp"));
        }
    }
}
