//! Memory policy: configuration, limits and the subsystem's authorization layer.
//!
//! nanomite has no global authorization engine today, so every memory entry
//! point — tools, the public API and the agent runtime — funnels through
//! [`MemoryPolicy::authorize`]. That is the memory subsystem's trust boundary,
//! and it is deliberately boring: no path in this module can grant a tool
//! permission or change how another tool behaves.

use super::model::{MemoryError, Sensitivity};
use std::env;

/// What a caller wants to do. Read is always allowed; writes can be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOp {
    Read,
    Write,
    Update,
    Delete,
    Import,
    /// Handing retrieved memories to a model.
    Inject,
}

impl MemoryOp {
    fn mutating(self) -> bool {
        matches!(
            self,
            MemoryOp::Write | MemoryOp::Update | MemoryOp::Delete | MemoryOp::Import
        )
    }
}

/// Limits, retention and safety switches. Every field has an environment
/// override so the defaults can be tightened without a rebuild.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPolicy {
    /// Master switch: `NANOMITE_MEMORY=0` turns the whole subsystem off.
    pub enabled: bool,
    /// Write gate for writes and deletes: `NANOMITE_MEMORY_READONLY=1`.
    pub writable: bool,
    /// Automatic post-task capture: `NANOMITE_MEMORY_AUTO=0`.
    pub auto_capture: bool,
    /// Highest sensitivity that may ever be injected into a model prompt.
    pub inject_ceiling: Sensitivity,
    /// Highest sensitivity the tools may read.
    pub tool_ceiling: Sensitivity,
    /// Operations affecting more records than this need an explicit confirmation.
    pub bulk_confirm_threshold: usize,
    /// Records injected before one model request (Stage 1 retrieval budget).
    pub max_inject_records: usize,
    /// Characters injected before one model request.
    pub max_inject_chars: usize,
    /// Hard ceiling on rows returned by any single query.
    pub max_result_rows: usize,
    /// Page size used when the caller does not ask for one.
    pub default_limit: usize,
    /// Soft cap on stored records; the oldest unpinned expired rows are pruned past it.
    pub max_records: usize,
    /// Default lifetime applied to automatically captured episodic memories.
    pub auto_retention_days: Option<u64>,
    /// Directory permissions for the memory root.
    pub mirror_markdown: bool,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            writable: true,
            auto_capture: true,
            inject_ceiling: Sensitivity::Internal,
            tool_ceiling: Sensitivity::Confidential,
            bulk_confirm_threshold: 10,
            max_inject_records: 8,
            max_inject_chars: 4_000,
            max_result_rows: 50,
            default_limit: 20,
            max_records: 5_000,
            auto_retention_days: Some(180),
            mirror_markdown: true,
        }
    }
}

impl MemoryPolicy {
    /// Reads `NANOMITE_MEMORY_*`. `use_env` is false in tests so a developer's
    /// shell cannot change test behaviour.
    pub fn from_env(use_env: bool) -> Self {
        let mut policy = Self::default();
        if !use_env {
            return policy;
        }
        if let Some(flag) = flag("NANOMITE_MEMORY") {
            policy.enabled = flag;
        }
        if let Some(flag) = flag("NANOMITE_MEMORY_READONLY") {
            policy.writable = !flag;
        }
        if let Some(flag) = flag("NANOMITE_MEMORY_AUTO") {
            policy.auto_capture = flag;
        }
        if let Some(flag) = flag("NANOMITE_MEMORY_MARKDOWN") {
            policy.mirror_markdown = flag;
        }
        if let Ok(value) = env::var("NANOMITE_MEMORY_INJECT_CEILING") {
            if let Some(parsed) = Sensitivity::parse(&value) {
                policy.inject_ceiling = parsed;
            }
        }
        if let Ok(value) = env::var("NANOMITE_MEMORY_TOOL_CEILING") {
            if let Some(parsed) = Sensitivity::parse(&value) {
                policy.tool_ceiling = parsed;
            }
        }
        policy.bulk_confirm_threshold = num_env(
            "NANOMITE_MEMORY_BULK_CONFIRM",
            policy.bulk_confirm_threshold,
        );
        policy.max_inject_records =
            num_env("NANOMITE_MEMORY_INJECT_RECORDS", policy.max_inject_records);
        policy.max_inject_chars = num_env("NANOMITE_MEMORY_INJECT_CHARS", policy.max_inject_chars);
        policy.max_result_rows = num_env("NANOMITE_MEMORY_MAX_ROWS", policy.max_result_rows);
        policy.default_limit = num_env("NANOMITE_MEMORY_LIMIT", policy.default_limit);
        policy.max_records = num_env("NANOMITE_MEMORY_MAX_RECORDS", policy.max_records);
        if let Ok(days) = env::var("NANOMITE_MEMORY_RETENTION_DAYS") {
            let days = days.trim();
            policy.auto_retention_days = match days.to_ascii_lowercase().as_str() {
                "" | "0" | "never" | "off" => None,
                other => other.parse::<u64>().ok(),
            };
        }
        policy
    }

    /// The single gate every memory operation passes through.
    ///
    /// `count` is how many records the operation will touch and `confirmed` is the
    /// caller's explicit acknowledgement of a bulk or destructive request.
    pub fn authorize(
        &self,
        op: MemoryOp,
        count: usize,
        ceiling: Sensitivity,
        confirmed: bool,
    ) -> Result<(), MemoryError> {
        if !self.enabled {
            return Err(MemoryError::policy(
                "memory is disabled (set NANOMITE_MEMORY=1 to enable it)",
            ));
        }
        if op.mutating() && !self.writable {
            return Err(MemoryError::policy(
                "memory is read-only (NANOMITE_MEMORY_READONLY=1)",
            ));
        }
        if !ceiling.within(match op {
            MemoryOp::Inject => self.inject_ceiling,
            _ => self.tool_ceiling,
        }) {
            return Err(MemoryError::policy(format!(
                "{ceiling} memories exceed the permitted ceiling for {}",
                match op {
                    MemoryOp::Inject => "prompt injection",
                    _ => "tool access",
                }
            )));
        }
        if matches!(op, MemoryOp::Delete | MemoryOp::Import)
            && count > self.bulk_confirm_threshold
            && !confirmed
        {
            return Err(MemoryError::policy(format!(
                "{} affects {count} records, above the limit of {}. \
                 Repeat with confirm=true once the user has approved it.",
                match op {
                    MemoryOp::Delete => "delete",
                    _ => "import",
                },
                self.bulk_confirm_threshold
            )));
        }
        Ok(())
    }

    /// Clamps a requested page size into `[1, max_result_rows]`.
    pub fn clamp_limit(&self, requested: Option<usize>) -> usize {
        let limit = requested.unwrap_or(self.default_limit);
        limit.clamp(1, self.max_result_rows)
    }

    /// Clamps the injection budget. Never returns more than `max_inject_records`.
    pub fn clamp_inject_records(&self, requested: usize) -> usize {
        requested.clamp(1, self.max_inject_records)
    }
}

fn flag(key: &str) -> Option<bool> {
    env::var(key).ok().map(|v| {
        let v = v.trim().to_ascii_lowercase();
        !matches!(v.as_str(), "" | "0" | "false" | "no" | "off")
    })
}

fn num_env(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_bounded() {
        let policy = MemoryPolicy::default();
        assert!(policy.max_inject_records > 0 && policy.max_inject_records <= 32);
        assert!(policy.max_inject_chars >= 500 && policy.max_inject_chars <= 32_000);
        assert_eq!(policy.clamp_limit(Some(10_000)), policy.max_result_rows);
        assert_eq!(policy.clamp_limit(Some(0)), 1);
        assert_eq!(policy.clamp_limit(None), policy.default_limit);
    }

    #[test]
    fn disabled_store_refuses_everything_including_reads() {
        let policy = MemoryPolicy {
            enabled: false,
            ..MemoryPolicy::default()
        };
        assert!(policy
            .authorize(MemoryOp::Read, 1, Sensitivity::Public, false)
            .is_err());
    }

    #[test]
    fn bulk_delete_needs_confirmation() {
        let policy = MemoryPolicy {
            bulk_confirm_threshold: 3,
            ..MemoryPolicy::default()
        };
        assert!(policy
            .authorize(MemoryOp::Delete, 2, Sensitivity::Internal, false)
            .is_ok());
        let err = policy
            .authorize(MemoryOp::Delete, 9, Sensitivity::Internal, false)
            .unwrap_err();
        assert!(err.to_string().contains("confirm=true"), "{err}");
        assert!(policy
            .authorize(MemoryOp::Delete, 9, Sensitivity::Internal, true)
            .is_ok());
    }

    #[test]
    fn read_only_blocks_mutations_but_not_reads() {
        let policy = MemoryPolicy {
            writable: false,
            ..MemoryPolicy::default()
        };
        assert!(policy
            .authorize(MemoryOp::Read, 1, Sensitivity::Internal, false)
            .is_ok());
        assert!(policy
            .authorize(MemoryOp::Write, 1, Sensitivity::Internal, false)
            .is_err());
    }

    #[test]
    fn injection_respects_a_stricter_ceiling() {
        let policy = MemoryPolicy {
            inject_ceiling: Sensitivity::Public,
            tool_ceiling: Sensitivity::Confidential,
            ..MemoryPolicy::default()
        };
        assert!(policy
            .authorize(MemoryOp::Read, 1, Sensitivity::Confidential, false)
            .is_ok());
        assert!(policy
            .authorize(MemoryOp::Inject, 1, Sensitivity::Internal, false)
            .is_err());
        assert!(policy
            .authorize(MemoryOp::Inject, 1, Sensitivity::Public, false)
            .is_ok());
    }
}
