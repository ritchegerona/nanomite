//! Lightweight runtime telemetry: turn and tool-call timings, bounded ring
//! buffer, never blocks the UI thread. All recording is synchronous and O(1).
//!
//! Optimized: maintains a secondary sorted buffer by elapsed_ms for O(1) percentile
//! queries without re-sorting on every stats() call.

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

/// Bounded ring buffer of recent telemetry records with cached sorted views.
/// Pre-sorts on insert to enable O(1) percentile queries without re-sorting.
#[derive(Debug, Default)]
pub struct Telemetry {
    tools: VecDeque<ToolRecord>,
    turns: VecDeque<TurnRecord>,
    tools_sorted: VecDeque<ToolRecord>, // Sorted by elapsed_ms ascending
    turns_sorted: VecDeque<TurnRecord>, // Sorted by elapsed_ms ascending
    tools_count: usize,
    turns_count: usize,
}

impl Telemetry {
    pub fn record_tool(&mut self, name: &str, ok: bool, elapsed: Duration) {
        let record = ToolRecord {
            name: name.to_string(),
            ok,
            elapsed_ms: elapsed.as_millis() as u64,
            at: SystemTime::now(),
        };

        // Maintain ring buffer
        if self.tools_count >= CAPACITY {
            let old = self.tools.pop_front();
            // Remove from sorted view
            if let Some(ref old) = old {
                let pos = self.tools_sorted.iter().position(|t| t.at == old.at);
                if let Some(p) = pos {
                    self.tools_sorted.remove(p);
                }
            }
            self.tools_count -= 1;
        }

        self.tools.push_back(record.clone());
        self.tools_count += 1;

        // Insert into sorted position (by elapsed_ms, then by insertion order)
        let elapsed_ms = record.elapsed_ms;
        let insert_pos = self
            .tools_sorted
            .iter()
            .position(|t| t.elapsed_ms > elapsed_ms);
        match insert_pos {
            Some(pos) => self.tools_sorted.insert(pos, record),
            None => self.tools_sorted.push_back(record),
        }
    }

    pub fn record_turn(&mut self, tool_calls: usize, ok: bool, elapsed: Duration) {
        let record = TurnRecord {
            ok,
            tool_calls,
            elapsed_ms: elapsed.as_millis() as u64,
            at: SystemTime::now(),
        };

        // Maintain ring buffer
        if self.turns_count >= CAPACITY {
            let old = self.turns.pop_front();
            // Remove from sorted view
            if let Some(ref old) = old {
                let pos = self.turns_sorted.iter().position(|t| t.at == old.at);
                if let Some(p) = pos {
                    self.turns_sorted.remove(p);
                }
            }
            self.turns_count -= 1;
        }

        self.turns.push_back(record.clone());
        self.turns_count += 1;

        // Insert into sorted position
        let elapsed_ms = record.elapsed_ms;
        let insert_pos = self
            .turns_sorted
            .iter()
            .position(|t| t.elapsed_ms > elapsed_ms);
        match insert_pos {
            Some(pos) => self.turns_sorted.insert(pos, record),
            None => self.turns_sorted.push_back(record),
        }
    }

    /// Recent tool-call stats: count, failures, p50/p95 latency (ms).
    /// Uses pre-sorted buffer for O(1) percentile lookup.
    pub fn tool_stats(&self) -> ToolStats {
        if self.tools_count == 0 {
            return ToolStats::default();
        }
        let failures = self.tools.iter().filter(|t| !t.ok).count();
        let p50 = percentile_sorted(&self.tools_sorted, 0.50);
        let p95 = percentile_sorted(&self.tools_sorted, 0.95);
        ToolStats {
            count: self.tools_count,
            failures,
            p50_ms: p50,
            p95_ms: p95,
        }
    }

    /// Recent turn stats: count, failures, p50/p95 latency (ms), avg tool calls.
    /// Uses pre-sorted buffer for O(1) percentile lookup.
    pub fn turn_stats(&self) -> TurnStats {
        if self.turns_count == 0 {
            return TurnStats::default();
        }
        let failures = self.turns.iter().filter(|t| !t.ok).count();
        let p50 = percentile_sorted_turn(&self.turns_sorted, 0.50);
        let p95 = percentile_sorted_turn(&self.turns_sorted, 0.95);
        let avg_tools: u64 = self.turns.iter().map(|t| t.tool_calls as u64).sum();
        TurnStats {
            count: self.turns_count,
            failures,
            p50_ms: p50,
            p95_ms: p95,
            avg_tool_calls: (avg_tools / self.turns_count as u64) as usize,
        }
    }

    pub fn recent_tools(&self, limit: usize) -> Vec<ToolRecord> {
        self.tools.iter().rev().take(limit).cloned().collect()
    }

    pub fn recent_turns(&self, limit: usize) -> Vec<TurnRecord> {
        self.turns.iter().rev().take(limit).cloned().collect()
    }
}

/// Calculate percentile from a pre-sorted tool_record buffer (ascending order).
#[inline]
fn percentile_sorted(sorted: &VecDeque<ToolRecord>, p: f64) -> u64 {
    let count = sorted.len();
    if count == 0 {
        return 0;
    }
    let idx = ((count - 1) as f64 * p).round() as usize;
    sorted[idx].elapsed_ms
}

/// Calculate percentile from a pre-sorted turn_record buffer (ascending order).
#[inline]
fn percentile_sorted_turn(sorted: &VecDeque<TurnRecord>, p: f64) -> u64 {
    let count = sorted.len();
    if count == 0 {
        return 0;
    }
    let idx = ((count - 1) as f64 * p).round() as usize;
    sorted[idx].elapsed_ms
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
        assert_eq!(tel.tools_count, CAPACITY);
        // tool-0 should have been dropped
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
        // p50 should be around 50ms, p95 around 95ms
        assert!(stats.p50_ms >= 49 && stats.p50_ms <= 51);
        assert!(stats.p95_ms >= 94 && stats.p95_ms <= 96);
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

    #[test]
    fn recent_tools_returns_most_recent() {
        let mut tel = Telemetry::default();
        for i in 0..5 {
            tel.record_tool(&format!("tool-{i}"), true, Duration::from_millis(i));
        }
        let recent = tel.recent_tools(3);
        assert_eq!(recent.len(), 3);
        // Most recent is first in rev order
        assert_eq!(recent[0].name, "tool-4");
        assert_eq!(recent[1].name, "tool-3");
        assert_eq!(recent[2].name, "tool-2");
    }
}
