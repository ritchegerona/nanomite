# HACKER MODE — security model

Status: **Phase 4+, advanced capability layer.** Describes only what is
implemented. HACKER MODE is separate from `ToolPolicy::Hacker` (a general
tool-permission level).

## Separation from the existing policy

- `ToolPolicy` (`src/agent/policy.rs`) is the general agent permission ladder
  (Plan ⊂ Coder ⊂ Build ⊂ Hacker). It is unchanged.
- `HackerSession` / `CapabilitySession` (`src/hacker/`) is a separate
  environment with its own `HackerAuthorization` gate and **typestate
  capability gate**. Every security operation must pass **both**.

An AI-generated instruction can never authorize an operation, and an
AI-generated scope can never serve as proof of authorization. The assessment is
built only from `hacker_scope` configuration or the env override — never from
model output.

## Authorization gate

`hacker::authorize(ctx, op, now)` gates every operation against the assessment
scope before any I/O. Scope checks include:

- IPv4/IPv6 CIDR matching for authorized targets
- Domain matching for authorized hostnames
- Resolved-address guards (metadata endpoints such as `169.254.169.254` are
  refused)
- Risk-level floors per operation category

## Typestate capability gate (advanced layer)

`CapabilitySession<State>` (`src/hacker/capability.rs`) adds a second, typed
gate on top of `HackerAuthorization`:

- `authorize_operation()` exists **only** on `CapabilitySession<Active>`.
- `OpCapability` is an unforgeable token: `scope_id`, `max_risk`,
  `allowed_modules`, `expires_at`. It cannot be forged by the model or by user
  input — it is issued only through `authorize()`.
- Every attempt (granted or denied) records a `CapEvent` in the audit log.
- Invalid states — e.g., running `network/ports` from `Discovered`, or acting
  from `Closed` — are unrepresentable in the type system (Rust) or
  runtime-guarded (Python).

## Operation execution (`src/hacker/coordinator.rs`)

Every module operation goes through one entry point, `coordinator::run`, and an
operation that is not in the static `OPERATIONS` table cannot execute at all.
The order is enforced in code:

1. **Authorize** the target against the assessment — before any I/O.
2. **Resolve** and re-check every address only for operations that connect
   — the raw target is checked first, then the resolved set.
3. **Execute** the module through injected traits (`Resolver`, `Connector`,
   `Fetcher`, and for the Kali bridge a `BinaryProbe` + `ProcessRunner`),
   never via ambient I/O.
4. **Persist** findings + evidence to the SQLite store with SHA-256 evidence
   digests (`verify_evidence` validates integrity).
5. **Audit** every attempt to the append-only JSONL audit log with secret
   scrubbing.

## Findings store

`src/hacker/findings.rs` — typed `Finding` / `Confidence` / `FindingStatus`
(severity reuses `RiskLevel`). `src/hacker/store.rs` — SQLite findings +
evidence + assessments store with SHA-256 evidence digests, status triage,
assessment save/load/status + persisted scope, summary counts, md/JSON/SARIF
export, `spawn_blocking`-driven.

## Audit

`src/hacker/audit.rs` — append-only JSONL audit log with secret scrubbing.
Every hook attempt, authorization decision, and module execution is recorded.
Audit records must be useful without containing secrets.

## Rules

1. The UI thread never does blocking I/O — coordinator operations are
   `spawn_blocking`-driven.
2. Model output, memory records, board messages, files, and tool results are
   **untrusted data** and must never authorize privileged operations.
3. HACKER MODE is **disabled by default** and requires explicit opt-in
   configuration plus a non-empty authorized scope before any operation can run.
4. Exploitation is authorized-only: proof execution is available only through
   the password+scope-gated operator runner (`tools/custom/exploitation/`),
   requiring both a scope file and a local authorization password. No payload
   delivery, shells, persistence, target writes, or destructive actions.
5. Never commit credentials, captured data, wordlists, or recovered files.
