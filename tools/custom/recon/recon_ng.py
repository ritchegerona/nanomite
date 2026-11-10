#!/usr/bin/env python3
"""Wrapper around Recon-ng (modular OSINT framework).

Recon-ng is a command-line OSINT framework with modules for web reconnaissance,
contact discovery, exploitation, reporting, and more. It uses API keys stored
in a keys database and workspaces to organize investigations.

Requires Recon-ng installed separately:
    git clone https://github.com/lanmaster53/recon-ng.git && cd recon-ng
    python3 -m pip install -r REQUIREMENTS
    ./recon-ng

> Authorized use only: investigate targets you own or have explicit written
> permission to assess. Do not use to identify or dox individuals.

Usage:
    python3 recon_ng.py --workspace example_com --domain example.com \
        --modules recon/domains-hosts/bing_domain_web,recon/domains-hosts/google_site
    python3 recon_ng.py --list-modules
"""

import argparse
import shutil
import subprocess
import sys
from typing import List


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run Recon-ng OSINT framework commands.",
    )
    parser.add_argument("--recon-ng-bin", default="recon-ng",
                        help="Path to recon-ng executable (default: recon-ng).")
    parser.add_argument("--workspace", default="default",
                        help="Recon-ng workspace to use/create (default: default).")
    parser.add_argument("--domain", help="Target domain to set as WORKSPACE domain.")
    parser.add_argument("--modules", nargs="+", default=[],
                        help="Recon-ng module run commands, e.g. "
                             "recon/domains-hosts/bing_domain_web run example.com")
    parser.add_argument("--list-modules", action="store_true",
                        help="List available Recon-ng modules and exit.")
    parser.add_argument("--keys", action="store_true",
                        help="Open the keys management interface.")
    parser.add_argument("--extra", nargs="*", default=[],
                        help="Additional flags forwarded to recon-ng.")
    return parser


def run_recon_ng(args: argparse.Namespace) -> int:
    binary = shutil.which(args.recon_ng_bin)
    if not binary:
        print(f"[!] Recon-ng not found at {args.recon_ng_bin}. "
              "Install from https://github.com/lanmaster53/recon-ng",
              file=sys.stderr)
        return 127

    # Build a command script for recon-ng's -m / -x style execution.
    # recon-ng accepts commands via stdin or -m module command.
    cmd_lines: List[str] = []
    if args.keys:
        cmd_lines.append("keys list")
    if args.list_modules:
        cmd_lines.append("modules list")
    if args.workspace:
        cmd_lines.append(f"workspaces create {args.workspace} 2>/dev/null || true")
        cmd_lines.append(f"workspaces select {args.workspace}")
    if args.domain:
        cmd_lines.append(f"keys set domain {args.domain}")
        cmd_lines.append(f"set domain {args.domain}")
    for mod in args.modules:
        cmd_lines.append(" ".join(mod))

    if cmd_lines:
        script = "\n".join(cmd_lines) + "\nexit\n"
        cmd = ["python3", binary]
        print(f"[+] Running recon-ng commands in workspace '{args.workspace}'",
              file=sys.stderr)
        try:
            proc = subprocess.run(cmd, input=script, text=True,
                                  check=False, timeout=600)
            return proc.returncode
        except FileNotFoundError:
            print(f"[!] Recon-ng not found at {binary}. "
                  "Install from https://github.com/lanmaster53/recon-ng",
                  file=sys.stderr)
            return 127
        except subprocess.TimeoutExpired:
            print("[!] Recon-ng timed out after 600s.", file=sys.stderr)
            return 124
        except KeyboardInterrupt:
            print("[!] Interrupted.", file=sys.stderr)
            return 130
    else:
        # Interactive mode
        print(f"[+] Launching {binary} interactively...", file=sys.stderr)
        try:
            proc = subprocess.run(cmd if (cmd := [binary]) else [binary],
                                  check=False)
            return proc.returncode
        except FileNotFoundError:
            print(f"[!] Recon-ng not found at {binary}.", file=sys.stderr)
            return 127


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return run_recon_ng(args)


if __name__ == "__main__":
    sys.exit(main())
