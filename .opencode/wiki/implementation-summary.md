---
title: Central Tool Authorization Engine Implementation Summary
tags: [implementation, tool_policy, security, phase-1]
sourceRefs: [src/agent/policy.rs, src/agent/runtime.rs, src/config.rs, src/commands.rs, src/app.rs, src/ui/panels.rs]
lastReviewed: 2026-10-16
---

# Central Tool Authorization Engine — Phase 1 Implementation

This document summarizes the initial implementation (L1) of the central tool authorization engine for nanomite.

## Overview

Implemented a **single choke point** (`runtime::run_tool`) that authorizes all tool calls based on the configured `ToolPolicy` (auto / plan / readonly). This addresses the genuine security gap where `run_command` and other mutating tools executed with the user's shell permissions without any authorization check.

## Key Features

### 1. Tool Policy Enum (`src/agent/policy.rs`)

```rust
pub enum ToolPolicy {
    Auto,      // Allow everything the user enabled
    Plan,      // Allow reads; deny writes with plan explanation
    Readonly,  // Allow reads; deny writes with read-only explanation
}
```

### 2. Risk Classification

All tools are classified into three risk categories:

- **Safe**: Read-only tools (`read_file`, `list_files`, `search`, `web_*`, `memory_*` lookups)
- **Write**: Mutating tools (`write_file`, `edit_file`, `memory_save/update`, `propose_self_edit`)
- **External**: Machine-leaving tools (`run_command`, `ssh_run`, `memory_delete`)

### 3. Authorization Logic

In **Plan** and **Readonly** modes, any tool with `Write` or `External` risk is refused with a machine-readable JSON error:

```json
{
  "error": "plan mode is active: mutating tools are disabled. Do not attempt them - report the plan and what you would run, and wait for the user.",
  "code": "plan_mode"
}
```

### 4. Configuration Support

#### Environment Variable
```bash
NANOMITE_TOOL_POLICY=plan   # set to auto / plan / readonly
```

#### Stored Configuration
In `~/.config/nanomite/config.toml`:
```toml
tool_policy = "plan"
```

#### Default
`ToolPolicy::Auto` (preserves existing behavior when not configured)

### 5. User Interface

- **Command**: `/policy` (alias `/tool-policy`)
- **Keyboard**: `Ctrl+P`
- **Header Display**: `model · reasoning · [tool_policy]` (shows current mode)
- **Toast Notifications**: "tool policy: auto / plan / readonly"

### 6. Integration Points

#### Runtime (`src/agent/runtime.rs`)

- Added `tool_policy: ToolPolicy` field to `AgentRuntime`
- Updated `run_tool()` to enforce policy after config disabled checks
- All existing call sites updated to pass policy parameter

#### Configuration (`src/config.rs`)

- Added `tool_policy: ToolPolicy` to `Config` struct
- Added `tool_policy: Option<String>` to `Stored` struct
- Added `parse_tool_policy()` helper function
- Added `NANOMITE_TOOL_POLICY` environment variable support
- Updated `stored_text()` to include tool_policy in saved config

#### Commands (`src/commands.rs`)

- Added `Policy` variant to `Command` enum
- Added `/policy` entry in `COMMANDS` array with alias `/tool-policy`

#### App (`src/app.rs`)

- Added `cycle_tool_policy()` method to `App` struct
- Added `Policy` command handler in `handle_command()`
- Added `Ctrl+P` keybinding in `handle_key()`

#### UI (`src/ui/panels.rs`)

- Updated header to display tool policy label alongside model and reasoning
- Shows `auto`, `plan`, or `readonly` based on current policy

## Design Decisions

### Default Policy = `auto`

Preserves backward compatibility: existing user configurations and muscle memory continue to work without changes. Users who want stricter control can explicitly set `plan` or `readonly`.

### Staged Implementation (per user choice)

Implemented **deny-with-explanation** first (Plan/Readonly modes). Interactive approve overlay is planned as Phase 2, maintaining the staged approach requested by the user.

### Single Choke Point

All tool calls go through `runtime::run_tool()`, ensuring consistent enforcement across:
- Memory tools
- File tools
- External commands
- Self-improvement tools

## Testing

All tests pass (376/376):

### Policy Module Tests (`agent/policy.rs`)

- `auto_allows_everything`: Auto mode allows all tool calls
- `restricted_modes_allow_reads_and_refuse_mutations`: Plan/readonly deny mutations
- `plan_and_readonly_carry_distinct_codes`: Different error codes for each mode
- `parse_accepts_known_labels_and_rejects_unknown`: Config parsing
- `cycle_visits_every_mode_once`: Auto → Plan → Readonly → Auto cycle
- `risk_classes_are_stable`: Risk classification is consistent

### Integration Tests

All existing integration tests continue to pass, confirming backward compatibility:
- Memory tool tests
- File tool tests
- External command tests
- Self-improvement tests

## Usage Examples

### Default Behavior (Auto)

```
// User runs a mutating tool — it works as before
tool call write_file {"path": "file.txt", "content": "data"}
```

### Plan Mode

```
// User runs a mutating tool — it's refused with explanation
tool call write_file {"path": "file.txt", "content": "data"}
→ {"error":"plan mode is active: mutating tools are disabled...","code":"plan_mode"}
```

### Readonly Mode

```
// Similar to plan mode, but with read-only explanation
tool call write_file {"path": "file.txt", "content": "data"}
→ {"error":"read-only policy: this tool changes state and is disabled...","code":"read_only_policy"}
```

## Security Impact

### Before

- No authorization for mutating tools
- `run_command` executed with full shell permissions
- Only per-subsystem gates (memory policy, disabled_tools, improve overlay)

### After

- **Centralized** authorization at tool dispatch
- **Policy-driven** control over mutation capabilities
- **Machine-readable** errors for model adaptation
- **Configurable** per-user control (auto / plan / readonly)

## Limitations (Phase 1)

1. **No Interactive Approval**: Plan/readonly modes only refuse, they don't offer approval prompts
2. **No UI for Setting Policy**: Users must edit config file or use `/policy` command
3. **Limited Risk Classification**: Currently coarse-grained; fine-grained control would require more complex policy engine
4. **No Logging**: Currently no audit trail of policy decisions

## Next Steps (Phase 2)

1. **Interactive Approve Overlay**: Add approval prompts for plan mode (per user design choice)
2. **Policy Logging**: Add audit logging for policy decisions
3. **Fine-Grained Policies**: Support for custom risk classifications
4. **Documentation**: Update user documentation and help text

## Files Modified

- `src/agent/policy.rs` (new module)
- `src/agent/runtime.rs` (updated run_tool, AgentRuntime)
- `src/agent/mod.rs` (added policy module)
- `src/config.rs` (added tool_policy field, parsing, storage)
- `src/commands.rs` (added Policy enum variant, command entry)
- `src/app.rs` (added cycle_tool_policy, command handler, keybinding)
- `src/ui/panels.rs` (updated header display)
- `docs/AUDIT-2026-10-14.md` (updated capability gap status)
- `.opencode/wiki/concepts/capability-gaps-l0.md` (updated implementation status)

## Verification

Run all tests to verify implementation:

```bash
cargo test --lib --release
```

Or run specific policy tests:

```bash
cargo test agent::policy
cargo test agent::runtime
```

## Conclusion

Phase 1 successfully implements the central tool authorization engine with three operational modes (auto / plan / readonly). The implementation is minimal, focused, and maintains backward compatibility while providing the requested security controls. The staged approach (deny-with-explanation first, interactive overlay planned) aligns with the user's design decisions and allows for gradual adoption.

The foundation is now in place for Phase 2 enhancements including interactive approvals and more sophisticated policy controls.