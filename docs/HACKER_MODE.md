# HACKER MODE

> Status: **Phase 4+ — advanced capability layer.** HACKER MODE is a
> cybersecurity operations *environment*, separate from `ToolPolicy::Hacker`.
> It adds a **typestate capability-session** (`Discovered → Authorized →
> Active → Closed`), an unforgeable `OpCapability` token, an independent
> authorization gate, an assessment scope, an append-only audit trail, and a
> module registry on top of the existing nanomite agent runtime.
>
> Exploitation is **authorized-only**: the crate plans validation, and proof
> execution is available only through the password+scope-gated operator runner
> (`tools/custom/exploitation/exploit_runner.py`). No payloads, shells,
> persistence or destructive actions are part of the crate.
> HACKER MODE is **disabled by default**.

## Entering it

```
/hacker            # activate and open the workspace (also alt+shift+h)
/hacker status     # one-line status
/hacker modules    # module catalogue (also the workspace overlay)
/hacker scope      # configured authorized scope
/hacker audit      # audit log status
/hacker help       # list executable operations
/hacker run <module> <op> [target] [arg]
/hacker kali list  # installed Kali assessment tools
/hacker kali <tool> <target>
/hacker findings   # persisted findings (also: show <id>, verify <id>)
/hacker report [md|json|sarif]   # export to the hacker dir
/hacker assessment list          # persisted assessments
/hacker assessment use <id>      # resume one (stored scope + window)
/hacker assessment complete [<id>]
/hacker exit       # leave, restore the configured tool policy
```

Convenience aliases for the common operations:

```
/hacker recon <host>                 # recon/dns
/hacker ports <host>                 # network/ports
/hacker web <url|host>               # web/headers
/hacker vuln <component> <version>   # vuln/correlate
/hacker plan <target> <weakness>     # exploitation/plan (guidance only)
```

Activation requires `hacker_enabled = true` and a non-empty `hacker_scope`.
Without either, `/hacker` refuses with a hint and changes nothing.

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
- `NANOMITE_HACKER_DIR=/path` — audit + `findings.db` + `vulndb.json` location
- `NANOMITE_HACKER_VULNDB=/path/db.json` — vulnerability database for `vuln/correlate`

Existing config files load unchanged: the section is optional and every key
has a default.

## Advanced capability layer (typestate)

HACKER MODE sessions use a **typestate capability gate**. Invalid operations
are unrepresentable at compile time (Rust) or runtime-guarded (Python):

```
Discovered  ── authorize() ───▶  Authorized  ── activate() ───▶  Active  ── run_module() ───▶ ...
                                                                    │
                                                                    └──── close() ────────────▶  Closed
```

- **`Discovered`** — session created, no authority yet. Only `authorize()`
  is available.
- **`Authorized`** — capability token validated. Only `activate()` is
  available.
- **`Active`** — operations may run. `run_module()` exists *only* on this
  state. `close()` transitions to `Closed`.
- **`Closed`** — read-only audit/report access only.

### `OpCapability` (unforgeable authority token)

```
OpCapability {
    token: String,          // unforgeable, issued at authorization
    scope_id: String,       // must match the assessment scope
    max_risk: RiskLevel,    // risk ceiling — operations above it are refused
    allowed_modules: Vec<String>,  // module whitelist
    expires_at: SystemTime, // capability expires; session must re-authorize
}
```

Every `authorize_operation()` call on an `Active` session:

1. Checks the capability is present and not expired.
2. Checks the requested module is in `allowed_modules`.
3. Checks the operation risk is at or below `max_risk`.
4. Records a `CapEvent` (granted or denied) in the append-only audit log.

This means a session cannot be used for a module or risk level it was not
explicitly authorized for — even if the code path is reached.

## What the workspace shows

Assessment name/id, status and remaining validity, the authorized scope,
whether permissions and auto-approval are on, the 12-module catalogue with each
module's category/capability count/risk floor, and the executable operation
table with each operation's category and risk floor.

## Operations

| operation | category | floor | target | does |
|---|---|---|---|---|
| `recon/dns` | passive | low | required | resolves a host, reports bounded, de-duplicated addresses |
| `network/ports` | active-discovery | medium | required | bounded TCP reachability over a small default port set |
| `web/headers` | active-discovery | medium | required | one no-redirect fetch + security-header analysis |
| `vuln/correlate` | passive | low | none (component + version) | version→CVE correlation, never a confirmed finding |
| `exploitation/plan` | intrusive | critical | required | structured validation plan + evidence checklist |
| `kali/available` | passive | low | none | lists the catalogued Kali tools found on `PATH` |
| `kali/run` | active-discovery | medium | required (tool + target) | runs one catalogued Kali tool with a fixed argv template |
| `reporting/summary` | passive | low | none | counts findings by severity and status |
| `reporting/export` | passive | low | none (format arg) | writes md/JSON/SARIF, same renderer as `/hacker report` |

All seven go through `hacker::coordinator::run`: unknown operations never
execute, the target is authorized *before* any I/O, anything that connects or
fetches is resolved and every address re-checked (a permitted hostname cannot
become an SSRF into private/metadata space), and every allow/deny/error is
audited with a duration.

Execution properties:

- **Bounded.** At most 8 addresses × 32 ports per network run, 32 DNS answers,
  a single HTTP request, and no response body is read.
- **Passive-first.** DNS and version correlation touch no target; the active
  operations are one connect per port / one request per URL.
- **Honest.** `vuln` reports `Suspected (version-only)` and says "no
  vulnerability database configured" rather than implying there are none; a
  version match is never `Confirmed`.
- **Authorized-only execution.** `exploitation/plan` produces guidance
  (verification steps, evidence checklist) only. Actual proof execution lives in
  `tools/custom/exploitation/exploit_runner.py`, is non-destructive by design
  (reads/evidence only), and requires a scope file plus the local authorization
  password. It contains no shells, payload delivery, persistence or automation
  of compromise.

## The 12 modules

`recon`, `network`, `web`, `vuln`, `forensics`, `osint`, `threat-intel`,
`password`, `wireless`, `exploitation`, `post-exploitation`, `reporting`.

Five modules execute operations (`recon`, `network`, `web`, `vuln`,
`reporting`), `exploitation` provides the plan operation (with authorized-only
execution in the operator tools), and `kali` bridges installed assessment tools.
The rest remain descriptors until their phase lands.

## Not (yet) implemented

- No authenticated enumeration, password auditing, wireless work or
  post-exploitation.
- No exploit execution inside the crate. Proof execution is deliberately
  external: `tools/custom/exploitation/exploit_runner.py` performs curated,
  non-destructive checks only after scope + password verification. Wire your own
  authorized framework in for anything beyond that.

## Kali bridge (`kali/run`)

`kali/run` executes tools already installed on the host (typically Kali Linux)
through a **curated catalogue** with a fixed, conservative argv template and one
authorized target substituted into `{target}`. Arbitrary flags are not
accepted, nothing is run through a shell, and the target is refused if it is
flag-like or contains whitespace/control characters (so it cannot inject extra
arguments).

The catalogue is deliberately limited to **assessment and enumeration**:
`nmap`, `masscan`, `naabu`, `rustscan`, `nikto`, `whatweb`, `httpx`, `wpscan`,
`nuclei`, `sslyze`, `sslscan`, `testssl.sh`, `wafw00f`, `subfinder`,
`assetfinder`, `amass`, `dnsrecon`, `whois`, `dig`, `host`, `nbtscan`,
`enum4linux`. Each run is bounded (default 180 s, output capped) and audited
with the tool's declared risk.

Credential-attack, wireless-attack and exploitation-framework tools
(`hydra`, `medusa`, `john`, `hashcat`, `aircrack-ng`, `wifite`, `responder`,
`msfconsole`, `sqlmap`, `bettercap`, …) are intentionally **not** wired into the
catalogue and are rejected as unknown tools. Use `run_command` or your own
authorized framework for those.

`/hacker kali list` (or `/hacker run kali available`) reports which catalogued
tools are installed without running anything.

## Findings & evidence (Phase 3)

Successful operations turn their structured results into **findings** and
persist them to SQLite at `<hacker dir>/findings.db` (default
`~/.local/share/nanomite/hacker/findings.db`), disabled with
`[hacker] store = false` or `NANOMITE_HACKER_STORE=0`.

- A finding carries module/operation, target, a title, a **severity**
  (the shared informational→critical scale), a **confidence** and a status.
- **Confidence is honest by construction.** Directly observed facts (an open
  port, an absent header, a completed tool run, a DNS inventory) are
  `Confirmed`. Inferences are `Suspected` — a version→CVE match is never
  `Confirmed`.
- Each finding's detail is stored as an **evidence** row with a **SHA-256** so a
  later reader can tell whether what they see is what was captured. Evidence
  text is scrubbed of secrets and bounded before it reaches disk; a `Set-Cookie`
  **value** is never stored — only which flags are missing.
- `/hacker findings` lists the newest first; `show <id>` shows one finding plus
  its evidence digest; `verify <id>` re-computes every evidence SHA-256 and
  flags a mismatch; `triage|accept|close <id>` updates its status (and is
  itself audited). `/hacker report [md|json|sarif]` writes a markdown, JSON or
  SARIF 2.1.0 report to `<hacker dir>/reports/` and prints the path.

Findings are **evidence, not instructions**: `/hacker findings` output is
display-only and is never fed back to a tool or to the model as authorization.

Producers today: `web/headers` (missing headers, disclosure, cookie flags),
`vuln/correlate` (one finding per CVE match, `Suspected`), `network/ports` (one
informational finding per open port), `recon/dns` (one informational DNS
inventory finding) and `kali/run` (one informational finding per run, so the
bounded tool output is retained as hashed evidence). `kali/available` and
`exploitation/plan` emit none.

## Assessment lifecycle (Phase 4)

Assessments are persisted with their authorized scope, so they survive a
restart and can be resumed by id:

- `/hacker assessment list` reads the newest assessments from the store
  (id, name, status, finding count, window, authorization reference).
- `/hacker assessment use <id>` loads the **stored** scope and window and calls
  `HackerSession::resume`, which refuses a disabled session, an empty scope, a
  non-authorized/completed assessment, an expired window or one that is not
  valid yet. The scope always comes from the store/config — never from model
  output — and `validate()` runs before the session switches.
- `/hacker assessment complete [<id>]` marks an assessment completed; if it is
  the active one, HACKER MODE deactivates and the tool policy falls back.

The agent cannot call these: they are local slash commands (metadata + store
reads). Resuming never bypasses the authorization gate, the scope checks or the
audit log.

## Bringing your own tooling

nanomite coordinates and records; it does not embed offensive tooling. To use
an existing authorized framework, register it as an external tool in the tool
manifest and set `risk = "external"` so the general `ToolPolicy` gate keeps it
behind Build/Hacker mode. HACKER MODE never shows the interactive confirmation
prompt (the gate is Plan-only).

See `docs/HACKER_SECURITY.md` for the authorization, scope and audit model.
