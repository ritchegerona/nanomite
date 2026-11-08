//! Circuit and stream introspection.
//!
//! `GETINFO circuit-status` / `GETINFO stream-status` return a block of
//! whitespace-separated records. The parsers here turn that text into typed
//! values and are deliberately total: an unparseable line is skipped, never a
//! panic, because the input comes from another process.

use std::time::{Duration, Instant};

/// Tor refuses NEWNYM more often than once every ten seconds.
pub const NEWNYM_MIN_INTERVAL: Duration = Duration::from_secs(10);

/// One relay in a circuit path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterHop {
    /// Relay fingerprint (hex) — or a bridge identifier.
    pub identity: String,
    /// Optional relay name.
    pub nickname: Option<String>,
}

impl RouterHop {
    pub fn new(identity: impl Into<String>, nickname: Option<String>) -> Self {
        Self {
            identity: identity.into(),
            nickname,
        }
    }
}

/// Lifecycle of a circuit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitStatus {
    Building,
    Built,
    Extended,
    Failed,
    Closed,
    GuardWait,
    Other(String),
}

impl CircuitStatus {
    pub fn parse(value: &str) -> Self {
        match value {
            "LAUNCHED" => CircuitStatus::Building,
            "BUILDING" => CircuitStatus::Building,
            "BUILT" => CircuitStatus::Built,
            "EXTENDED" => CircuitStatus::Extended,
            "FAILED" => CircuitStatus::Failed,
            "CLOSED" => CircuitStatus::Closed,
            "GUARD_WAIT" => CircuitStatus::GuardWait,
            other => CircuitStatus::Other(other.to_string()),
        }
    }

    pub fn is_built(&self) -> bool {
        matches!(self, CircuitStatus::Built)
    }
}

/// One entry from `circuit-status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Circuit {
    pub id: u32,
    pub status: CircuitStatus,
    pub hops: Vec<RouterHop>,
    pub purpose: Option<String>,
    pub time_created: Option<String>,
    pub build_flags: Option<String>,
}

/// Lifecycle of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamStatus {
    New,
    NewResolve,
    Remap,
    SentConnect,
    Succeeded,
    Failed,
    Closed,
    Detached,
    Other(String),
}

impl StreamStatus {
    pub fn parse(value: &str) -> Self {
        match value {
            "NEW" => StreamStatus::New,
            "NEWRESOLVE" => StreamStatus::NewResolve,
            "REMAP" => StreamStatus::Remap,
            "SENTCONNECT" => StreamStatus::SentConnect,
            "SUCCEEDED" => StreamStatus::Succeeded,
            "FAILED" => StreamStatus::Failed,
            "CLOSED" => StreamStatus::Closed,
            "DETACHED" => StreamStatus::Detached,
            other => StreamStatus::Other(other.to_string()),
        }
    }
}

/// One entry from `stream-status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub id: u32,
    pub status: StreamStatus,
    /// `0` is rendered as `None` (no circuit yet).
    pub circuit_id: Option<u32>,
    /// `host:port` target, when one is known.
    pub target: Option<String>,
    pub purpose: Option<String>,
}

/// Aggregate health of the current circuits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CircuitHealth {
    pub total: usize,
    pub built: usize,
    pub failed: usize,
    pub building: usize,
}

impl CircuitHealth {
    /// True when at least one circuit is usable.
    pub fn is_usable(&self) -> bool {
        self.built > 0
    }
}

/// Parse a `circuit-status` block (one record per line).
pub fn parse_circuit_status(text: &str) -> Vec<Circuit> {
    let mut circuits = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line == "." {
            continue;
        }
        let Some(circuit) = parse_circuit_line(line) else {
            continue;
        };
        circuits.push(circuit);
    }
    circuits
}

fn parse_circuit_line(line: &str) -> Option<Circuit> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }
    let id: u32 = tokens[0].parse().ok()?;
    let status = CircuitStatus::parse(tokens[1]);
    let mut index = 2;
    let mut hops = Vec::new();
    if let Some(path) = tokens.get(2) {
        if path.starts_with('$') {
            hops = parse_hops(path);
            index = 3;
        }
    }
    let mut purpose = None;
    let mut time_created = None;
    let mut build_flags = None;
    for token in &tokens[index..] {
        if let Some((key, value)) = token.split_once('=') {
            match key {
                "PURPOSE" => purpose = Some(value.to_string()),
                "TIME_CREATED" => time_created = Some(value.to_string()),
                "BUILD_FLAGS" => build_flags = Some(value.to_string()),
                _ => {}
            }
        }
    }
    Some(Circuit {
        id,
        status,
        hops,
        purpose,
        time_created,
        build_flags,
    })
}

fn parse_hops(path: &str) -> Vec<RouterHop> {
    path.split(',')
        .filter(|hop| !hop.is_empty())
        .map(|hop| {
            let hop = hop.strip_prefix('$').unwrap_or(hop);
            let (identity, nickname) = match hop.split_once('=') {
                Some((id, name)) => (id, Some(name.to_string())),
                None => match hop.split_once('~') {
                    Some((id, name)) => (id, Some(name.to_string())),
                    None => (hop, None),
                },
            };
            RouterHop::new(identity, nickname)
        })
        .collect()
}

/// Parse a `stream-status` block (one record per line).
pub fn parse_stream_status(text: &str) -> Vec<Stream> {
    let mut streams = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line == "." {
            continue;
        }
        let Some(stream) = parse_stream_line(line) else {
            continue;
        };
        streams.push(stream);
    }
    streams
}

fn parse_stream_line(line: &str) -> Option<Stream> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let id: u32 = tokens[0].parse().ok()?;
    let status = StreamStatus::parse(tokens[1]);
    let circuit_id = match tokens[2].parse::<u32>().ok() {
        Some(0) | None => None,
        Some(id) => Some(id),
    };
    let target = tokens.get(3).and_then(|t| match *t {
        "0" => None,
        other => Some(other.to_string()),
    });
    let mut purpose = None;
    for token in tokens.iter().skip(4) {
        if let Some((key, value)) = token.split_once('=') {
            if key == "PURPOSE" {
                purpose = Some(value.to_string());
            }
        }
    }
    Some(Stream {
        id,
        status,
        circuit_id,
        target,
        purpose,
    })
}

/// Summarise a parsed circuit list.
pub fn circuit_health(circuits: &[Circuit]) -> CircuitHealth {
    let mut health = CircuitHealth {
        total: circuits.len(),
        ..CircuitHealth::default()
    };
    for circuit in circuits {
        match circuit.status {
            CircuitStatus::Built => health.built += 1,
            CircuitStatus::Failed | CircuitStatus::Closed => health.failed += 1,
            CircuitStatus::Building | CircuitStatus::Extended | CircuitStatus::GuardWait => {
                health.building += 1
            }
            CircuitStatus::Other(_) => {}
        }
    }
    health
}

/// Local rate limiter for `SIGNAL NEWNYM`, which tor throttles to one per ten
/// seconds. Enforcing it client-side avoids a stream of `5xx` errors.
#[derive(Debug, Clone)]
pub struct NewnymLimiter {
    last: Option<Instant>,
    min_interval: Duration,
}

impl Default for NewnymLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl NewnymLimiter {
    pub fn new() -> Self {
        Self {
            last: None,
            min_interval: NEWNYM_MIN_INTERVAL,
        }
    }

    pub fn with_interval(min_interval: Duration) -> Self {
        Self {
            last: None,
            min_interval,
        }
    }

    /// Returns `true` and records `now` when a NEWNYM may be sent.
    pub fn try_acquire(&mut self, now: Instant) -> bool {
        match self.last {
            Some(last) if now.saturating_duration_since(last) < self.min_interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }

    /// Time left before the next NEWNYM is allowed.
    pub fn remaining(&self, now: Instant) -> Duration {
        match self.last {
            Some(last) => self
                .min_interval
                .saturating_sub(now.saturating_duration_since(last)),
            None => Duration::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCUITS: &str = "\
1 BUILT $AAAA1111~guard,$BBBB2222~middle,$CCCC3333=exit BUILD_FLAGS=NEED_CAPACITY PURPOSE=GENERAL TIME_CREATED=2026-01-01T00:00:00.000000
2 BUILDING $DDDD4444~guard PURPOSE=GENERAL
3 FAILED $EEEE5555~guard,$FFFF6666~middle PURPOSE=GENERAL
4 CLOSED $AAAA1111~guard
";

    #[test]
    fn parses_circuits_and_hops() {
        let circuits = parse_circuit_status(CIRCUITS);
        assert_eq!(circuits.len(), 4);
        let first = &circuits[0];
        assert_eq!(first.id, 1);
        assert_eq!(first.status, CircuitStatus::Built);
        assert_eq!(first.hops.len(), 3);
        assert_eq!(first.hops[0].identity, "AAAA1111");
        assert_eq!(first.hops[0].nickname.as_deref(), Some("guard"));
        assert_eq!(first.hops[2].nickname.as_deref(), Some("exit"));
        assert_eq!(first.purpose.as_deref(), Some("GENERAL"));
        assert_eq!(first.build_flags.as_deref(), Some("NEED_CAPACITY"));
        assert!(first.time_created.is_some());
    }

    #[test]
    fn parses_circuit_without_path() {
        let circuits = parse_circuit_status("9 LAUNCHED PURPOSE=GENERAL\n");
        assert_eq!(circuits.len(), 1);
        assert_eq!(circuits[0].status, CircuitStatus::Building);
        assert!(circuits[0].hops.is_empty());
        assert_eq!(circuits[0].purpose.as_deref(), Some("GENERAL"));
    }

    #[test]
    fn skips_malformed_circuit_lines() {
        let circuits = parse_circuit_status("not-a-circuit\n.\n\n5 BUILT $AA~g");
        assert_eq!(circuits.len(), 1);
    }

    const STREAMS: &str = "\
11 SUCCEEDED 1 example.com:443 PURPOSE=USER
12 SENTCONNECT 2 198.51.100.7:80 PURPOSE=USER
13 NEW 0 0 PURPOSE=USER
14 FAILED 0 resolver.example:53 PURPOSE=DNS
";

    #[test]
    fn parses_streams() {
        let streams = parse_stream_status(STREAMS);
        assert_eq!(streams.len(), 4);
        assert_eq!(streams[0].status, StreamStatus::Succeeded);
        assert_eq!(streams[0].circuit_id, Some(1));
        assert_eq!(streams[0].target.as_deref(), Some("example.com:443"));
        assert_eq!(streams[2].circuit_id, None);
        assert_eq!(streams[2].target, None);
        assert_eq!(streams[3].purpose.as_deref(), Some("DNS"));
    }

    #[test]
    fn health_counts_states() {
        let health = circuit_health(&parse_circuit_status(CIRCUITS));
        assert_eq!(health.total, 4);
        assert_eq!(health.built, 1);
        assert_eq!(health.failed, 2);
        assert_eq!(health.building, 1);
        assert!(health.is_usable());
    }

    #[test]
    fn newnym_limiter_enforces_ten_seconds() {
        let base = Instant::now();
        let mut limiter = NewnymLimiter::new();
        assert!(limiter.try_acquire(base));
        assert!(!limiter.try_acquire(base + Duration::from_secs(5)));
        assert_eq!(
            limiter.remaining(base + Duration::from_secs(5)),
            Duration::from_secs(5)
        );
        assert!(limiter.try_acquire(base + Duration::from_secs(10)));
    }
}
