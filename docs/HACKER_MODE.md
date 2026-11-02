# HACKER MODE

> Status: **Phase 3 — findings & evidence store.** Phase 2 operations run
> against an active assessment, plus a curated bridge to installed Kali
> assessment tools; successful operations now persist structured findings and
> hashed evidence to a SQLite store and can export a markdown report.
> Exploitation is **plan-only** (no payloads, no compromise automation).
> HACKER MODE is a cybersecurity operations *environment*. It is **not**
> `ToolPolicy::Hacker`, which is a general tool-permission level.

HACKER MODE layers an independent authorization gate, an assessment scope, an
append-only audit trail, and a module registry on top of the existing nanomite
agent runtime. It reuses the provider, runtime, tool dispatch, TUI and
persistence infrastructure unchanged, and is **disabled by default**.

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
/hacker findings   # persisted findings (also: findings show <id>)
/hacker report     # export a markdown report to the hacker dir
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
scope = "10.0.0.0/8,example.com"   # authorized CIDRs/IPs/domains (comma- or space-separated)
assessment_id = "local"
session_minutes = 60       # how long an activated assessment stays valid
auto_approve = true        # skip the approval prompt while an authorized assessment is active
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

## What the workspace shows

Assessment name/id, status and remaining validity, the authorized scope,
whether permissions and auto-approval are on, the 12-module catalogue with each
module's category/capability count/risk floor, and the executable operation
table with each operation's category and risk floor.

## Operations (Phase 2)

| operation | category | floor | target | does |
|---|---|---|---|---|
| `recon/dns` | passive | low | required | resolves a host, reports bounded, de-duplicated addresses |
| `network/ports` | active-discovery | medium | required | bounded TCP reachability over a small default port set |
| `web/headers` | active-discovery | medium | required | one no-redirect fetch + security-header analysis |
| `vuln/correlate` | passive | low | none (component + version) | version→CVE correlation, never a confirmed finding |
| `exploitation/plan` | intrusive | critical | required | structured validation plan + evidence checklist |
| `kali/available` | passive | low | none | lists the catalogued Kali tools found on `PATH` |
| `kali/run` | active-discovery | medium | required (tool + target) | runs one catalogued Kali tool with a fixed argv template |

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
- **No weaponization.** `exploitation/plan` produces guidance (verification
  steps, evidence checklist) only. It contains no exploit code, payloads,
  shells, persistence, credential attacks or automation of compromise.

## The 12 modules

`recon`, `network`, `web`, `vuln`, `forensics`, `osint`, `threat-intel`,
`password`, `wireless`, `exploitation`, `post-exploitation`, `reporting`.

Four modules execute their first operation (recon, network, web, vuln),
`exploitation` provides the plan operation, and `kali` bridges installed
assessment tools. The rest remain descriptors until their phase lands.

## Not (yet) implemented

- No finding triage/edit commands, no multi-asset assessment manager, and no
  SARIF/JSON export — `/hacker report` writes markdown only.
- No authenticated enumeration, password auditing, wireless work or
  post-exploitation.
- No exploit execution of any kind. Confirmation, PoC tooling and any
  potentially disruptive validation stay outside this crate; wire your own
  authorized framework in as an external tool instead (see the note below).

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

Detection is by binary name only, so a same-named tool from another project
(e.g. the Python `httpx` CLI versus ProjectDiscovery `httpx`) is reported as
installed and may reject the template's flags — check the run output if a tool
errors unexpectedly.

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
  its evidence digest; `triage|accept|close <id>` updates its status (and is
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

## Bringing your own tooling

nanomite coordinates and records; it does not embed offensive tooling. To use
an existing authorized framework, register it as an external tool in the tool
manifest and set `risk = "external"` so the general `ToolPolicy` gate keeps it
behind Build/Hacker mode. With an active assessment, the gated auto-approve
already applies.

See `docs/HACKER_SECURITY.md` for the authorization, scope and audit model.
