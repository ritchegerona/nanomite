# tools/

Standalone operator tools used alongside NANOMITE. Nothing here is part of the
Rust crate, the `nanomite` Python package, or the test suite.

Every tool is categorized by function under `tools/custom/<category>/`. Tools
are self-contained and stdlib-only where possible. They are **not** wired into
the agent tool catalog (`src/tools/`) — the agent reaches them through
`run_command`/`ssh_run`, or you run them directly.

> Use these only on systems and networks you own or have explicit written
> authorization to test.

## Categories

| Category | Purpose |
|----------|---------|
| `inventory/` | Host discovery and baseline drift detection |
| `recon/` | Port scanning, DNS enumeration, WHOIS/RDAP, and OSINT harvesting |
| `web/` | Web misconfiguration scanning, content discovery, TLS audit |
| `cracking/` | Hash identification, offline cracking, wordlist generation |
| `exploitation/` | Authorized-only exploitation (runner + planner) and default-cred hygiene |
| `wireless/` | Wi-Fi rogue-AP detection and wifite wrapper |
| `reporting/` | Report rendering (Markdown → PDF/DOCX) |

## Inventory

### inventory/
| Tool | Origin | Purpose |
|------|--------|---------|
| `passive_network_inventory.py` | built here | Passive ARP inventory of the local subnet (scapy optional; ARP-cache fallback for non-root). |
| `baseline_comparator.py` | built here | Diff a current inventory against a saved baseline; flags new hosts, MAC changes (possible spoofing), and removals. |

### recon/
| Tool | Origin | Purpose |
|------|--------|---------|
| `port_scanner.py` | built here | Concurrent TCP connect scan with optional banner grabbing; CIDR and target-file input, JSON output. |
| `subdomain_enum.py` | built here | DNS brute-force subdomain enumeration with wildcard detection. |
| `whois_rdap.py` | built here | WHOIS (port 43) and RDAP lookup with IANA referral following; JSON output. |
| `theharvester.py` | third-party wrapper | Wrapper for theHarvester email, subdomain, and host harvesting. |
| `spiderfoot.py` | third-party wrapper | Wrapper for SpiderFoot (200+ OSINT modules). |
| `recon_ng.py` | third-party wrapper | Wrapper for Recon-ng modular OSINT framework. |
| `maltego.py` | third-party wrapper | Wrapper for Maltego CE; GraphML export for visual link analysis. |

### web/
| Tool | Origin | Purpose |
|------|--------|---------|
| `web_vuln_scan.py` | built here | Read-only scan: security headers, cookie flags, CORS, HTTP methods, exposed paths, directory listing, version disclosure, TLS. |
| `dir_buster.py` | built here | Content discovery with wildcard/soft-404 filtering and extension expansion. |
| `tls_audit.py` | built here | Protocol-version support, negotiated cipher, certificate subject/SAN/expiry. |

### cracking/
| Tool | Origin | Purpose |
|------|--------|---------|
| `hash_identifier.py` | built here | Identify hash formats from prefix/shape (bcrypt, crypt, Office, Kerberos, raw digests, …). |
| `hash_cracker.py` | built here | Multiprocessing dictionary cracker (MD5/SHA*/NTLM/crypt) with mutation rules. |
| `wordlist_generator.py` | built here | Case/leet/suffix wordlist mutations bounded by length and count. |

### exploitation/
| Tool | Origin | Purpose |
|------|--------|---------|
| `authorize.py` | built here | Verify the local root (sudo) password via sudo. Nothing is stored or logged. |
| `exploit_runner.py` | built here | **Authorized-only execution.** Curated, non-destructive proof-of-access checks (traversal read, exposed files, default creds, unauthenticated Redis, anonymous FTP) with evidence metadata. Requires scope file + root (sudo) password. |
| `exploit_planner.py` | built here | Planning surface: maps services/findings to techniques, prerequisites, safe detection steps, evidence and references. Requires scope file + root (sudo) password. |
| `default_cred_check.py` | built here | Small fixed default-credential list against HTTP Basic / FTP, rate-limited, masked output. Requires scope file + root (sudo) password. |

### wireless/
| Tool | Origin | Purpose |
|------|--------|---------|
| `rogue_ap_detector.py` | built here | Compare nearby APs against an authorized SSID→BSSID allow-list (unknown APs, evil twins, spoofing). |
| `wifite_py3_minimal.py` | third-party wrapper (stub) | Minimal Python 3 placeholder for `wifite`; points to upstream. |

### reporting/
| Tool | Origin | Purpose |
|------|--------|---------|
| `generate_report.py` | built here | Render a Markdown report to PDF/DOCX (scenario-specific; hardcoded paths, adjust before use). |

## Usage

```bash
# inventory
python3 tools/custom/inventory/passive_network_inventory.py -o inventory.json
python3 tools/custom/inventory/baseline_comparator.py --current inventory.json --baseline baseline.json

# recon
python3 tools/custom/recon/port_scanner.py 192.168.1.0/24 --top --banners
python3 tools/custom/recon/subdomain_enum.py --domain example.com

# web
python3 tools/custom/web/web_vuln_scan.py --url https://app.example.com
python3 tools/custom/web/dir_buster.py --url https://app.example.com --ext php,html,bak
python3 tools/custom/web/tls_audit.py --host app.example.com

# cracking
python3 tools/custom/cracking/hash_identifier.py 5f4dcc3b5aa765d61d8327deb882cf99
python3 tools/custom/cracking/hash_cracker.py --hash <digest> --wordlist words.txt --rules basic
python3 tools/custom/cracking/wordlist_generator.py --base acme,company2026 -o words.txt

# exploitation (authorized-only)
python3 tools/custom/exploitation/authorize.py check      # verify root password
python3 tools/custom/exploitation/exploit_planner.py --target 10.0.0.5 --scope scope.txt \
    --service "http/Apache/2.4.49"
python3 tools/custom/exploitation/exploit_runner.py --url http://10.0.0.5 \
    --scope scope.txt --all --evidence-dir ./evidence
python3 tools/custom/exploitation/default_cred_check.py --url http://10.0.0.5 \
    --scope scope.txt
```

`scope.txt` holds one CIDR or domain per line (comments with `#` allowed).

## Root-password gate

The exploitation tools refuse to run until the machine's root (sudo) password
verifies. There is nothing to set up — sudo does the checking:

```bash
python3 tools/custom/exploitation/authorize.py check      # verify (prompts)
python3 tools/custom/exploitation/authorize.py status
python3 tools/custom/exploitation/authorize.py clear      # clear sudo timestamp
```

- Verification is delegated to `sudo -S -v`; the password is never stored,
  hashed, or logged by these tools — there is no digest file to protect.
- Supply the password for automation with `NANOMITE_ROOT_PASSWORD`
  (legacy `NANOMITE_TOOLS_AUTH_PASSWORD` still accepted) or pipe it via
  `--password-stdin`; avoid `--root-password`, which can appear in process
  listings.
- Failed attempts are rate-limited only by sudo's own policy.

Execution is **authorized-only** and non-destructive: the runner and
default-cred check perform reads and proof-of-access checks only — no payload
delivery, shells, persistence, target writes, or destructive actions.

## Adding a tool

1. Put it in the category folder that matches its function (create one only if
   none fits).
2. Keep it self-contained, stdlib-only where possible, with a docstring
   (purpose, authorization note, usage).
3. Add a row to the matching table above and to `tools/manifest.json`.
4. Never commit credentials, captured data, wordlists, or recovered files.
