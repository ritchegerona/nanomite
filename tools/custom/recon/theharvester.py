#!/usr/bin/env python3
"""Wrapper around theHarvester (email/subdomain/host harvesting).

theHarvester gathers emails, subdomains, hosts, employee names, open ports,
and banners from public sources (search engines, PGP key servers, Shodan,
Censys, etc.).

Requires theHarvester installed separately:
    git clone https://github.com/laramies/theHarvester.git && cd theHarvester
    pip install -r requirements.txt

> Authorized use only: enumerate assets you own or have explicit written
> permission to assess. Do not use to identify or dox individuals.

Usage:
    python3 theharvester.py -d example.com -b google,bing -l 500
    python3 theharvester.py -d example.com -b all -l 500 -f output.html
"""

import argparse
import shutil
import subprocess
import sys
from typing import List


SOURCES = (
    "baidu", "bing", "bingapi", "brave", "censys", "crtsh", "dnsdumpster",
    "duckduckgo", "github-code", "hackertarget", "hunter", "intelx",
    "netlas", "otx", "pentesttools", "publicwww", "qwant", "raventrack",
    "securitytrails", "shodan", "snus", "spyse", "sublist3r", "urlscan",
    "virustotal", "yahoo", "zoom"
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run theHarvester for OSINT harvesting (emails, subdomains, hosts).",
    )
    parser.add_argument("-d", "--domain", required=True,
                        help="Target domain to harvest.")
    parser.add_argument("-b", "--source", default="google",
                        help=f"Data source(s), comma-separated or 'all'. "
                             f"Common: {', '.join(SOURCES[:12])} ...")
    parser.add_argument("-l", "--limit", type=int, default=500,
                        help="Number of results to retrieve (default: 500).")
    parser.add_argument("-f", "--filename",
                        help="Save results to HTML file (theHarvester -f).")
    parser.add_argument("-s", "--shodan", action="store_true",
                        help="Use Shodan (requires SHODAN_API_KEY).")
    parser.add_argument("--theharvester-bin", default="theHarvester.py",
                        help="Path to theHarvester.py (default: theHarvester.py).")
    parser.add_argument("--extra", nargs="*", default=[],
                        help="Additional flags forwarded to theHarvester.")
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    binary = shutil.which(args.theharvester_bin)
    if not binary:
        print(f"[!] theHarvester not found at {args.theharvester_bin}. "
              "Install it first: https://github.com/laramies/theHarvester",
              file=sys.stderr)
        return 127

    cmd: List[str] = ["python3", binary, "-d", args.domain,
                      "-b", args.source, "-l", str(args.limit)]
    if args.filename:
        cmd += ["-f", args.filename]
    if args.shodan:
        cmd.append("-s")
    cmd += args.extra

    print(f"[+] Running: {' '.join(cmd)}", file=sys.stderr)
    try:
        proc = subprocess.run(cmd, check=False)
        return proc.returncode
    except FileNotFoundError:
        print(f"[!] theHarvester not found at {binary}. "
              "Install it first: https://github.com/laramies/theHarvester",
              file=sys.stderr)
        return 127
    except KeyboardInterrupt:
        print("[!] Interrupted.", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
