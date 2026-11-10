# nanomite tools — run anywhere, on any computer

The tools under `nanomite/tools/custom/` are designed to be **copy-and-run
portable**. Copy the whole `tools/` folder (or the whole `nanomite/`
repository) to any Linux, macOS, or Windows machine and the tools will work
from any directory — no hard-coded paths, no cwd dependence.

## What "portable" means

| Concern | How it's handled |
|---------|------------------|
| Working directory | All data/output dirs use `XDG_DATA_HOME` (`~/.local/share/nanomite-tools/<tool>/`), never `./` |
| Tool location | `NANOMITE_TOOLS_HOME` env var points to the absolute `tools/` dir |
| Python | `#!/usr/bin/env python3` shebang; stdlib-only where possible |
| Third-party tools | Wrappers (`theharvester`, `spiderfoot`, `recon_ng`, `maltego`) locate their binary via `shutil.which` or PATH |

## Option A: Install shims (recommended)

Run the installer once on each computer. After that, every tool is a
first-class command you can type from any directory.

### Linux / macOS

```bash
cd /path/to/nanomite/tools
./install.sh
# adds shims to ~/.local/bin (or /usr/local/bin as root)

export PATH="$HOME/.local/bin:$PATH"
# add that line to ~/.bashrc, ~/.zshrc, or ~/.profile to make it permanent

whois_rdap example.com --json
spiderfoot -t example.com -m sfp_whois
```

Custom install location:

```bash
./install.sh --portable /opt/nanomite-tools
export PATH="/opt/nanomite-tools/bin:$PATH"
```

### Windows (PowerShell)

Windows 11 blocks unsigned scripts by default. Before running the installer:

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
```

Then run the installer:

```powershell
cd C:\path\to\nanomite\tools
.\install.ps1
# creates shims in %LOCALAPPDATA%\nanomite-tools\bin (or C:\Program Files\nanomite-tools\bin as Admin)

# Add to PATH for this session:
$env:PATH = "$env:LOCALAPPDATA\nanomite-tools\bin;$env:PATH"

# To persist across reboots:
[Environment]::SetEnvironmentVariable("PATH", "$env:LOCALAPPDATA\nanomite-tools\bin;$env:PATH", [EnvironmentVariableTarget]::User)

whois_rdap example.com --json
spiderfoot -t example.com -m sfp_whois
```

The installer auto-detects `python3`, then `python`, then the `py` launcher — install Python from https://www.python.org/ if none are found.

**Tip:** If you can't change the execution policy, use the env loader instead:

```powershell
. C:\path\to\nanomite\tools\nanomite-tools-env.ps1
nanomite whois_rdap example.com --json
```

## Option B: Source the env file (no install)

If you can't install shims, source the env file instead. It defines a
`nanomite` helper function and adds shim dirs to PATH.

### Linux / macOS

```bash
source /path/to/nanomite/tools/nanomite-tools-env.sh
nanomite whois_rdap example.com --json
```

Add to `~/.bashrc` / `~/.zshrc`:

```bash
source /path/to/nanomite/tools/nanomite-tools-env.sh
```

### Windows

```powershell
. C:\path\to\nanomite\tools\nanomite-tools-env.ps1
nanomite whois_rdap example.com --json
```

Add to your PowerShell profile (`$PROFILE`):

```powershell
. 'C:\path\to\nanomite\tools\nanomite-tools-env.ps1'
```

## Option C: Run directly by path

No install, no sourcing. Call each script with its full path:

```bash
python3 /path/to/nanomite/tools/custom/recon/whois_rdap.py example.com --json
python3 /path/to/nanomite/tools/custom/recon/spiderfoot.py -t example.com -m sfp_whois
```

Set `NANOMITE_TOOLS_HOME` if a wrapper needs to locate sibling tools:

```bash
export NANOMITE_TOOLS_HOME=/path/to/nanomite/tools
```

## Third-party dependencies

The following tools are **wrappers** — they require the upstream project
installed separately on the target computer:

| nanomite tool | upstream project | install from |
|---------------|-----------------|--------------|
| `theharvester` | theHarvester | https://github.com/laramies/theHarvester |
| `spiderfoot` | SpiderFoot | https://www.spiderfoot.net/ |
| `recon_ng` | Recon-ng | https://github.com/lanmaster53/recon-ng |
| `maltego` | Maltego CE | https://www.maltego.com/product/maltego-ce/ |

The wrappers themselves are portable — copy them anywhere, they just can't
run until the upstream binary is on PATH.

The remaining tools (`whois_rdap`, `port_scanner`, `subdomain_enum`,
`web_vuln_scan`, `dir_buster`, `tls_audit`, hash tools, etc.) are
**stdlib-only** and run immediately with `python3`.

## Default data directories (cwd-independent)

| Tool | Default data/output dir |
|------|-------------------------|
| `spiderfoot` | `~/.local/share/nanomite-tools/spiderfoot/` |
| `maltego` (GraphML export) | `~/.local/share/nanomite-tools/maltego/maltego_graph.graphml` |

Override with `--data-dir` / `--out` / `--filename` flags as needed.

## Authorization notice

> Use these tools only on systems, networks, and domains you own or have
> explicit written authorization to assess. OSINT gathers public data — it
> cannot reliably identify a specific "hacker" online. Attribution requires
> lawful investigation channels (law enforcement, CERT, subpoenas). Do not
> use to identify or dox individuals.

## File layout after copy

```
nanomite/
├── tools/
│   ├── install.sh              # Linux/macOS installer
│   ├── install.ps1             # Windows installer
│   ├── nanomite-tools-env.sh   # Linux/macOS env loader
│   ├── nanomite-tools-env.ps1  # Windows env loader
│   ├── PORTABLE.md             # this file
│   ├── manifest.json           # tool catalog
│   ├── README.md               # full tool reference
│   └── custom/
│       ├── recon/
│       │   ├── whois_rdap.py        # stdlib-only
│       │   ├── port_scanner.py      # stdlib-only
│       │   ├── subdomain_enum.py    # stdlib-only
│       │   ├── theharvester.py      # wrapper
│       │   ├── spiderfoot.py        # wrapper
│       │   ├── recon_ng.py          # wrapper
│       │   └── maltego.py           # wrapper
│       ├── web/
│       ├── cracking/
│       ├── exploitation/
│       ├── inventory/
│       ├── reporting/
│       └── wireless/
```

Copy that tree to a USB stick, network share, or fresh VM — the tools travel
with it and run from anywhere.