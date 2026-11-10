#!/usr/bin/env python3
"""Wrapper around SpiderFoot (automated OSINT with 200+ modules).

SpiderFoot queries public APIs, search engines, DNS, WHOIS, shodan, censys,
virus total, and many other sources to enumerate infrastructure, identities,
leaked credentials, and exposed services around a target.

Requires SpiderFoot installed separately:
    https://www.spiderfoot.net/

> Authorized use only: scan targets you own or have explicit written
> permission to assess. Do not use to identify or dox individuals.

Usage:
    python3 spiderfoot.py -t example.com -m sfp_dnsresolve,sfp_whois,sfp_shodan
    python3 spiderfoot.py -t 1.2.3.4 -m sfp_netblockwhois,sfp_shodan -o json
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import urllib.request
import urllib.error
from pathlib import Path
from typing import List, Optional

DEFAULT_MODULES = "sfp_dnsresolve,sfp_whois,sfp_netblockwhois,sfp_shodan,sfp_censys"


def default_data_dir() -> Path:
    """XDG-compliant default data dir, independent of the caller's cwd."""
    xdg = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(xdg) / "nanomite-tools" / "spiderfoot"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run SpiderFoot OSINT automation against a target.",
    )
    parser.add_argument("-t", "--target", required=True,
                        help="Target: domain, IP, email, ASN, etc.")
    parser.add_argument("-m", "--modules", default=DEFAULT_MODULES,
                        help="Comma-separated SpiderFoot module names "
                             f"(default: {DEFAULT_MODULES}).")
    parser.add_argument("--spiderfoot-bin", default="sf.py",
                        help="Path to SpiderFoot sf.py (default: sf.py).")
    parser.add_argument("--data-dir", default=str(default_data_dir()),
                        help="SpiderFoot data/scan directory "
                             "(default: XDG_DATA_HOME/nanomite-tools/spiderfoot).")
    parser.add_argument("--output", choices=["json", "text"], default="json",
                        help="Output format for this wrapper's summary.")
    parser.add_argument("--extra", nargs="*", default=[],
                        help="Additional flags forwarded to SpiderFoot.")
    return parser


def run_spiderfoot(args: argparse.Namespace) -> Optional[dict]:
    """Execute SpiderFoot via its CLI and return parsed JSON results."""
    binary = shutil.which(args.spiderfoot_bin)
    if not binary:
        print(f"[!] SpiderFoot not found at {args.spiderfoot_bin}. "
              "Install from https://www.spiderfoot.net/", file=sys.stderr)
        return None

    # SpiderFoot CLI: sf.py -s <target> -m <modules> -o json -q
    cmd = [
        "python3", binary, "-s", args.target, "-m", args.modules,
        "-o", "json", "-q", "-d", args.data_dir,
    ] + args.extra

    print(f"[+] Running: {' '.join(cmd)}", file=sys.stderr)
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True,
                              check=False, timeout=300)
    except FileNotFoundError:
        print(f"[!] SpiderFoot not found at {binary}. "
              "Install from https://www.spiderfoot.net/", file=sys.stderr)
        return None
    except subprocess.TimeoutExpired:
        print("[!] SpiderFoot timed out after 300s.", file=sys.stderr)
        return None
    except KeyboardInterrupt:
        print("[!] Interrupted.", file=sys.stderr)
        return None

    if proc.returncode != 0:
        print(f"[!] SpiderFoot exited {proc.returncode}", file=sys.stderr)
        if proc.stderr:
            print(proc.stderr[:2000], file=sys.stderr)
        return None

    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        # Some builds print status text rather than pure JSON
        return {"raw": proc.stdout, "stderr": proc.stderr}


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    result = run_spiderfoot(args)
    if result is None:
        return 1

    if args.output == "json":
        print(json.dumps(result, indent=2))
    else:
        print(json.dumps(result, indent=2) if isinstance(result, dict) else result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
