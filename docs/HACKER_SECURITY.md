# HACKER MODE — security model

> Status: **Phase 4+, advanced capability layer.** Describes only what is
> implemented. HACKER MODE is separate from `ToolPolicy::Hacker` (a general
> tool-permission level).

## Separation from the existing policy

- `ToolPolicy` (`src/agent/policy.rs`) is the general agent permission ladder
  (Plan ⊂ Coder ⊂ Build ⊂ Hacker). It is unchanged.
- `HackerSession` / `CapabilitySession` (`src/hacker/`) is a separate
  environment with its own `HackerAuthorization` gate and **typestate
  capability gate**. Every security operation must pass **both**.

An AI-generated instruction can never authorize an operation, and an
AI-generated scope can never serve as proof of authorization. The assessment
is built only from `hacker_scope` configuration or the env override — never
from model output.

## Authorization gate

`hacker::authorize(ctx, op, now)` returns `Allowed{risk}` or
`Denied{code, reason}`. A call is refused when any of these fail:

1. no assessment is loaded (`hacker_no_assessment`);
2. the assessment fails validation (`hacker_invalid_assessment`);
3. it is not authorized or is outside its window (`hacker_assessment_inactive`);
4. the module is not permitted (`hacker_module_not_permitted`);
5. the operation category is not permitted (`hacker_operation_not_permitted`);
6. the target is out of scope (`hacker_target_out_of_scope`);
7. a resolved address is out of scope (`hacker_resolved_out_of_scope`) —
   a permitted hostname whose DNS resolves outside the allowed CIDRs is refused;
8. an address is unresolvable (`hacker_target_unresolved`) or resolves into
   private/loopback/link-local/metadata space not covered by an allowed CIDR
   (`hacker_resolved_internal`) — the DNS-rebinding / SSRF guard for anything
   that connects or fetches;
9. an active operation names no target (`hacker_target_required`);
10. a Kali run target is flag-like or contains whitespace/control characters
    (`hacker_target_invalid`) — refused before authorization so it can never be
    parsed as an extra argument by the tool.

## Typestate capability gate (advanced layer)

`CapabilitySession<State>` (`src/hacker/capability.rs`) adds a second,
typed gate on top of `HackerAuthorization`:

- `authorize_operation()` exists **only** on `CapabilitySession<Active>`.
- `OpCapability` is an unforgeable token: `scope_id`, `max_risk`,
  `allowed_modules`, `expires_at`. It cannot be forged by the model or by
  user input — it is issued only through `authorize()`.
- Every attempt (granted or denied) records a `CapEvent` in the audit log.
- Scope + risk ceiling checks run on every operation, so a session cannot
  exceed its authorized module set or risk ceiling even if the code path is
  reached.

This means invalid states — e.g., running `network/ports` from `Discovered`,
or exfiltrating from `Closed` — are unrepresentable in the type system.

## Operation execution (`src/hacker/coordinator.rs`)

Every module operation goes through one entry point, `coordinator::run`, and
an operation that is not in the static `OPERATIONS` table cannot execute at
all. The order is enforced in code:

1. **authorize** the target against the assessment — before any I/O;
2. **resolve** and re-check every address only for operations that connect
   (`network/ports`, `web/headers`) — the raw target is checked first, then
   the resolved set;
3. **execute** the module through injected traits (`Resolver`, `Connector`,
   `Fetcher`, and for the Kali bridge a `BinaryProbe` + `ProcessRunner`),
   never via ambient I/O;
4. **audit** allowed/denied/error with a duration.

Bounds: 8 addresses × 32 ports per network run, 32 DNS answers, one HTTP
request, and response bodies are never read. Redirects are **not** followed, so
a 30x cannot move the request to another host.

`web/headers` and `network/ports` resolve through the system resolver before
authorizing the address, but the fetch/connect is later done by the HTTP client
/ socket, which resolves again — a full TOCTOU guarantee would require pinning
the connection to the checked address, which reqwest does not expose here.

### Exploitation is plan-only

`exploitation/plan` is a guidance generator: verification steps and an evidence
checklist for a named weakness class. It contains **no exploit code, payloads,
shells, persistence, credential attacks, or automation of compromise**, and its
tests assert those terms are absent from the output. It is still classified
`Intrusive`/`critical`, so it needs a target inside the assessment.

## Kali bridge (`kali/run`)

The Kali bridge (`src/hacker/modules/kali.rs`) runs installed tools without a
shell, so shell metacharacters have no effect. Two further controls:

- **Fixed templates only.** The catalogue stores a conservative argv template
  per tool; the model supplies a tool name and a target, never flags. The target
  is substituted into `{target}` and validated by `safe_target` (non-empty,
  ≤253 chars, no leading `-`, no whitespace/control characters).
- **Bounded + audited.** Runs use the shared bounded executor (default 180 s,
  output capped) and are audited with the tool's declared risk, raised to the
  `ActiveDiscovery` floor.

The catalogue excludes credential-attack, wireless-attack and
exploitation-framework tools (`hydra`, `john`, `hashcat`, `aircrack-ng`,
`responder`, `msfconsole`, `sqlmap`, …); they are rejected as unknown tools. A
test asserts the exclusions.

## Scope

Typed targets and rules in `src/hacker/scope.rs`:

- IPv4 and IPv6 CIDR parsing and matching (bare IP = host route).
- Domain matching is exact-or-subdomain, never substring
  (`evil-example.com` is not inside `example.com`).
- Every resolved address must be in scope; empty resolution is
  `Ambiguous`, never assumed in scope (DNS-rebinding / round-robin guard).
- The cloud metadata address `169.254.169.254` is only in scope when an
  allowed CIDR literally covers it.

## Risk model

`RiskLevel` = informational < low < medium < high < critical. Each
`OperationCategory` sets a **floor** (`effective_risk`), so a module cannot
under-declare an intrusive operation as informational.

`OpCapability.max_risk` sets the ceiling for a session — no operation above
that ceiling can run on that session, regardless of module permissions.

## Approval — Plan-only confirmation gate (owner decision 2026-11-02)

The interactive confirmation prompt for `Risk::External` tool calls
(`run_command`, `ssh_run`, `memory_delete`, the browser tools) is consulted
only in **Plan** mode; Coder, Build and Hacker auto-approve, so a working
session is never interrupted. Plan mode's own policy still refuses command
execution.

HACKER MODE therefore never prompts. This does not weaken its controls: the
authorization gate, the scope checks, the typestate capability gate and the
audit log run for every operation regardless of tool mode.
`hacker.auto_approve` and the shared bypass flag are retained as
defense-in-depth; with the Plan-only gate the confirmer is already skipped
while HACKER MODE is in force.

## Audit

`src/hacker/audit.rs` writes one JSONL record per operation to
`NANOMITE_HACKER_DIR/audit.jsonl` (default `~/.local/share/nanomite/hacker`),
separate from conversational memory and the `/improve` audit log. Records
carry metadata only; free-text fields are additionally scrubbed
(`api_key`, `token`, `password`, `secret`, `authorization` → value redacted)
so credentials cannot reach disk. A test asserts a `password=hunter2` detail is
never written verbatim.

`CapEvent`s from the typestate capability gate are appended to the same audit
log, so every `authorize_operation()` attempt (granted or denied) is recorded
with the session id, module, operation, risk, and result.

## Findings & evidence store

`src/hacker/store.rs` is a SQLite store (synchronous connection behind a
`Mutex`, driven from `spawn_blocking`, exactly like `board`/`memory`) at
`<hacker dir>/findings.db`. It is not a second source of authorization: it only
records what an already-authorized operation produced.

- **Untrusted data on the way in.** Finding titles/details come from HTTP
  headers, tool output and CVE metadata. Every stored string is scrubbed of
  secrets (`scrub_secrets`) and bounded before it is written. A `Set-Cookie`
  value is never stored — only the names of missing flags.
- **Evidence integrity.** Each finding's detail is stored as an evidence row
  with a SHA-256 (`sha2`) and byte count, so a reader can verify the excerpt is
  unchanged. `/hacker findings verify <id>` re-computes every digest and
  reports a mismatch instead of trusting the row.
- **Display-only on the way out.** `/hacker findings` and `/hacker report`
  render text for the operator. That output is never fed back to a tool and can
  never serve as proof of authorization (the assessment alone is).
- **No elevation.** Persistence is gated by `[hacker] store`; disabling it
  stops writes but changes no authorization decision.
- **Triage is audited.** `/hacker findings triage|accept|close <id>` records an
  audit entry for the status change; it is a local metadata operation and needs
  no assessment. `/hacker report [md|json|sarif]` only renders what is already
  stored.
- **Reporting reads the store only.** `reporting/summary` and
  `reporting/export` are passive operations with no target: they call the same
  store reads as `/hacker report` and never touch a target.

## Assessment lifecycle

Assessments are persisted with their authorized scope and can be resumed by id:

- `/hacker assessment list` reads persisted assessments (metadata only).
- `/hacker assessment use <id>` loads the **stored** scope and window and calls
  `HackerSession::resume`, which refuses a disabled session, an empty scope, a
  non-authorized/completed assessment, an expired window or one that is not yet
  valid. The scope is never taken from model output, and `validate()` runs
  before the session switches.
- `/hacker assessment complete [<id>]` updates the status row; if it is the
  active assessment, HACKER MODE deactivates and the tool policy falls back.

None of these commands can authorize an operation by themselves; resuming still
passes the full authorization gate, scope checks and audit on every operation.

## External tools

User-defined TOML tools previously defaulted to `Risk::Safe` by name, which let
them run in Plan mode and bypass the confirmation gate. Now:

- `ExternalTool::risk()` parses the manifest `risk` (`safe|write|external`);
- **an absent or unrecognised `risk` defaults to `External`** — never `Safe`;
- `run_tool` refuses an `External`-class external tool unless the mode is Build
  or Hacker (`external_tool_mode`);
- the interactive gate consults the external tool's declared risk.

Manifests that omit `risk` therefore change behavior (they now need Build+
mode); add `risk = "safe"` to keep a read-only helper usable in Plan mode.

## Cross-platform

Process-group spawn/kill is `#[cfg(unix)]`; on Windows the direct-process kill
is used (grandchildren are not guaranteed to be reaped). `sh -c`, `ssh_run`,
sudo and the browser tools remain Unix-oriented; on Windows those tools fail
cleanly at runtime rather than at compile time.
