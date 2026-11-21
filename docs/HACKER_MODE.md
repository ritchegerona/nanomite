# HACKER MODE

Status: **Phase 4+ — advanced capability layer.** HACKER MODE is a cybersecurity
operations environment, separate from `ToolPolicy::Hacker`. It adds a typestate
capability-session state machine, an assessment scope, an independent
authorization gate, an append-only audit trail, and a module registry on top of
the nanomite agent runtime. Disabled by default.

## Entering it

```
/hacker              # activate and open the workspace overlay
/hacker status       # one-line status
/hacker modules      # module catalogue
/hacker scope        # configured authorized scope
/hacker audit        # audit log status
/hacker run <module> <op> [target] [arg]
/hacker kali <tool> <target>   # curated Kali assessment tools (fixed argv)
/hacker findings     # persisted findings (list / show / verify / triage)
/hacker report [md\|json\|sarif]   # export to the hacker data directory
/hacker assessment list          # persisted assessments
/hacker assessment use <id>      # resume one (stored scope + window)
/hacker assessment complete [<id>]
/hacker exit       # leave, restore the configured tool policy
```

Activation requires `hacker_enabled = true` and a non-empty `hacker_scope`. Both
must be configured before any operation can run.

## Configuration (`[hacker]` in `~/.config/nanomite/config.toml`)

```toml
[hacker]
enabled = false            # opt-in; default off
scope = "10.0.0.0/8,example.com"   # authorized CIDRs/IPs/domains
assessment_id = "local"
session_minutes = 60       # how long an activated assessment stays valid
auto_approve = true        # retained bypass; the confirmation prompt is Plan-only
audit = true               # write the append-only audit log
store = true               # persist findings + evidence to the SQLite store
```

Environment overrides (win over the file, per nanomite convention):

- `NANOMITE_HACKER=1` — enable
- `NANOMITE_HACKER_SCOPE="10.0.0.0/8 example.com"` — authorized scope
- `NANOMITE_HACKER_ASSESSMENT=asm-42`
- `NANOMITE_HACKER_SESSION_MINUTES=120`
- `NANOMITE_HACKER_AUTO_APPROVE=0`
- `NANOMITE_HACKER_AUDIT=0`
- `NANOMITE_HACKER_STORE=0` — disable findings persistence
- `NANOMITE_HACKER_DIR=/path` — audit + findings store location
- `NANOMITE_HACKER_VULNDB=/path/db.json` — vulnerability database for `vuln/correlate`

Existing config files load unchanged: the section is optional and every key has
a default.

## Separation from `ToolPolicy`

`ToolPolicy` (`src/agent/policy.rs`) is the general agent permission ladder
(Plan ⊂ Coder ⊂ Build ⊂ Hacker). It is unchanged.

`HackerSession` / `CapabilitySession` (`src/hacker/`) is a separate environment
with its own `HackerAuthorization` gate and typestate capability gate. Every
security operation must pass **both**.

An AI-generated instruction can never authorize an operation, and an
AI-generated scope can never serve as proof of authorization. The assessment is
built only from `hacker_scope` configuration or the env override — never from
model output.

## Authorization gate

`hacker::authorize(ctx, op, now)` gates every operation against the assessment
scope before any I/O. The gate infrastructure remains (typestate capability
gate, `OpCapability`, audit log, scope checks). Scope + risk ceiling checks run
on every operation, so a session cannot exceed its authorized module set or risk
ceiling even if the code path is reached.

## Typestate capability gate

`CapabilitySession<State>` (`src/hacker/capability.rs`) adds a second, typed
gate on top of `HackerAuthorization`:

- `authorize_operation()` exists **only** on `CapabilitySession<Active>`.
- `OpCapability` is an unforgeable token: `scope_id`, `max_risk`,
  `allowed_modules`, `expires_at`. It cannot be forged by the model or by user
  input — it is issued only through `authorize()`.
- Every attempt (granted or denied) records a `CapEvent` in the audit log.
- Invalid states (e.g., running `network/ports` from `Discovered`, or acting
  from `Closed`) are unrepresentable in the type system.

## Operation execution

Every module operation goes through one entry point, `coordinator::run`, and an
operation that is not in the static `OPERATIONS` table cannot execute at all.
The order is enforced in code:

1. **Authorize** the target against the assessment — before any I/O.
2. **Resolve** and re-check every address only for operations that connect
   (`network/ports`, `web/headers`) — the raw target is checked first, then the
   resolved set.
3. **Execute** the module through injected traits (`Resolver`, `Connector`,
   `Fetcher`, and for the Kali bridge a `BinaryProbe` + `ProcessRunner`),
   never via ambient I/O.
4. **Persist** findings and evidence to the SQLite store with SHA-256 evidence
   digests.
5. **Audit** every attempt (granted or denied) to the append-only JSONL audit
   log with secret scrubbing.

`exploitation` is authorized-only: the module builds the validation plan, and
proof execution is permitted only through the password+scope-gated operator
runner. The crate itself contains no payloads, shells, persistence, or
compromise automation. `kali` is a curated bridge to installed Kali assessment
tools using fixed argv templates via the shared bounded executor;
credential/wireless/exploitation-framework tools are excluded.
