//! Lightweight runtime telemetry: turn and tool-call timings, bounded ring
//! buffer, never blocks the UI thread. All recording is synchronous and O(1).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Maximum number of records kept in the ring buffer (oldest dropped first).
const CAPACITY: usize = 512;

/// One completed tool-call timing.
#[derive(Debug, Clone)]
pub struct ToolRecord {
    pub name: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub at: SystemTime,
}

/// One completed agent turn timing.
#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub tool_calls: usize,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub at: SystemTime,
}

/// Bounded ring buffer of recent telemetry records.
#[derive(Debug, Default)]
pub struct Telemetry {
    tools: VecDeque<ToolRecord>,
    turns: VecDeque<TurnRecord>,
}

impl Telemetry {
    pub fn record_tool(&mut self, name: &str, ok: bool, elapsed: Duration) {
        if self.tools.len() >= CAPACITY {
            self.tools.pop_front();
        }
        self.tools.push_back(ToolRecord {
            name: name.to_string(),
            ok,
            elapsed_ms: elapsed.as_millis() as u64,
            at: SystemTime::now(),
        });
    }

    pub fn record_turn(&mut self, tool_calls: usize, ok: bool, elapsed: Duration) {
        if self.turns.len() >= CAPACITY {
            self.turns.pop_front();
        }
        self.turns.push_back(TurnRecord {
            tool_calls,
            ok,
            elapsed_ms: elapsed.as_millis() as u64,
            at: SystemTime::now(),
        });
    }

    /// Recent tool-call stats: count, failures, p50/p95 latency (ms).
    pub fn tool_stats(&self) -> ToolStats {
        let mut elapsed: Vec<u64> = self.tools.iter().map(|t| t.elapsed_ms).collect();
        elapsed.sort_unstable();
        let count = elapsed.len();
        let failures = self.tools.iter().filter(|t| !t.ok).count();
        let p50 = percentile(&elapsed, 0.50);
        let p95 = percentile(&elapsed, 0.95);
        ToolStats {
            count,
            failures,
            p50_ms: p50,
            p95_ms: p95,
        }
    }

    /// Recent turn stats: count, failures, p50/p95 latency (ms), avg tool calls.
    pub fn turn_stats(&self) -> TurnStats {
        let mut elapsed: Vec<u64> = self.turns.iter().map(|t| t.elapsed_ms).collect();
        elapsed.sort_unstable();
        let count = elapsed.len();
        let failures = self.turns.iter().filter(|t| !t.ok).count();
        let p50 = percentile(&elapsed, 0.50);
        let p95 = percentile(&elapsed, 0.95);
        let avg_tools = if count > 0 {
            self.turns
                .iter()
                .map(|t| t.tool_calls)
                .sum::<usize>()
                .checked_div(count)
                .unwrap_or(0)
        } else {
            0
        };
        TurnStats {
            count,
            failures,
            p50_ms: p50,
            p95_ms: p95,
            avg_tool_calls: avg_tools,
        }
    }

    pub fn recent_tools(&self, limit: usize) -> Vec<ToolRecord> {
        self.tools.iter().rev().take(limit).cloned().collect()
    }

    pub fn recent_turns(&self, limit: usize) -> Vec<TurnRecord> {
        self.turns.iter().rev().take(limit).cloned().collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    pub count: usize,
    pub failures: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TurnStats {
    pub count: usize,
    pub failures: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub avg_tool_calls: usize,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Global telemetry handle: cheap to clone, all recording goes through a
/// single `Mutex`. Recording is O(1) and never does I/O, so it is safe to
/// call from the UI thread or any task.
#[derive(Debug, Clone, Default)]
pub struct TelemetryHandle {
    inner: Arc<Mutex<Telemetry>>,
}

impl TelemetryHandle {
    pub fn record_tool(&self, name: &str, ok: bool, elapsed: Duration) {
        if let Ok(mut tel) = self.inner.lock() {
            tel.record_tool(name, ok, elapsed);
        }
    }

    pub fn record_turn(&self, tool_calls: usize, ok: bool, elapsed: Duration) {
        if let Ok(mut tel) = self.inner.lock() {
            tel.record_turn(tool_calls, ok, elapsed);
        }
    }

    pub fn tool_stats(&self) -> ToolStats {
        self.inner
            .lock()
            .map(|tel| tel.tool_stats())
            .unwrap_or_default()
    }

    pub fn turn_stats(&self) -> TurnStats {
        self.inner
            .lock()
            .map(|tel| tel.turn_stats())
            .unwrap_or_default()
    }

    pub fn recent_tools(&self, limit: usize) -> Vec<ToolRecord> {
        self.inner
            .lock()
            .map(|tel| tel.recent_tools(limit))
            .unwrap_or_default()
    }

    pub fn recent_turns(&self, limit: usize) -> Vec<TurnRecord> {
        self.inner
            .lock()
            .map(|tel| tel.recent_turns(limit))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ring_buffer_drops_oldest() {
        let mut tel = Telemetry::default();
        for i in 0..CAPACITY + 10 {
            tel.record_tool(&format!("tool-{i}"), true, Duration::from_millis(i as u64));
        }
        assert_eq!(tel.tools.len(), CAPACITY);
        // tool-0 should have been dropped (it's the oldest of CAPACITY+10 inserts)
        assert!(!tel.tools.iter().any(|t| t.name == "tool-0"));
        assert!(tel
            .tools
            .iter()
            .any(|t| t.name == format!("tool-{}", CAPACITY + 9)));
    }

    #[test]
    fn tool_stats_percentiles() {
        let mut tel = Telemetry::default();
        for i in 1..=100 {
            tel.record_tool("test", true, Duration::from_millis(i as u64));
        }
        let stats = tel.tool_stats();
        assert_eq!(stats.count, 100);
        assert_eq!(stats.failures, 0);
        assert!(stats.p50_ms > 0);
        assert!(stats.p95_ms >= stats.p50_ms);
    }

    #[test]
    fn turn_stats_counts_failures() {
        let mut tel = Telemetry::default();
        tel.record_turn(3, true, Duration::from_millis(100));
        tel.record_turn(1, false, Duration::from_millis(50));
        let stats = tel.turn_stats();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.avg_tool_calls, 2);
    }
}
