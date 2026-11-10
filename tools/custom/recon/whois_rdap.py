#!/usr/bin/env python3
"""WHOIS (port 43) and RDAP (HTTP/JSON) lookup tool.

Native stdlib-only implementation. Queries WHOIS servers for domain/IP
registration data and falls back to RDAP (Registration Data Access Protocol)
where WHOIS is unavailable or returns redacted records.

> Authorized use only: run against domains/IPs you own or have explicit
> written permission to assess. Do not use to identify or dox individuals.

Usage:
    python3 whois_rdap.py example.com
    python3 whois_rdap.py 1.2.3.4 --rdap
    python3 whois_rdap.py example.com --whois-server whois.verisign-grs.com --json
"""

import argparse
import json
import socket
import sys
import urllib.request
import urllib.error
from typing import Optional, Dict, Any


def whois_lookup(query: str, server: str = "whois.iana.org", port: int = 43,
                 timeout: int = 15) -> str:
    """Perform a raw WHOIS TCP query against the given server."""
    sock = socket.create_connection((server, port), timeout=timeout)
    try:
        sock.sendall((query + "\r\n").encode("ascii", "ignore"))
        chunks = []
        while True:
            try:
                data = sock.recv(4096)
            except socket.timeout:
                break
            if not data:
                break
            chunks.append(data.decode("utf-8", "replace"))
        return "".join(chunks)
    finally:
        sock.close()


def whois_referral_server(text: str) -> Optional[str]:
    """Extract a referral WHOIS server from an IANA WHOIS response."""
    for line in text.splitlines():
        if line.lower().startswith("whois:"):
            return line.split(":", 1)[1].strip()
    return None


def rdap_lookup(query: str, timeout: int = 20) -> Dict[str, Any]:
    """Query RDAP for an IP address or domain name."""
    # IANA RDAP bootstrap: https://rdap.org (helps locate the correct server)
    bootstrap_url = f"https://rdap.org/{query}"
    req = urllib.request.Request(
        bootstrap_url,
        headers={"Accept": "application/rdap+json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError as exc:
        # rdap.org may not have a direct record; try common RDAP endpoints
        fallback = f"https://rdap.apnic.net/{query}"
        try:
            req = urllib.request.Request(
                fallback,
                headers={"Accept": "application/rdap+json"},
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return json.loads(resp.read().decode("utf-8", "replace"))
        except Exception as err2:
            return {"error": str(exc), "fallback_error": str(err2)}
    except Exception as exc:  # noqa: BLE001
        return {"error": str(exc)}


def main() -> int:
    parser = argparse.ArgumentParser(
        description="WHOIS/RDAP lookup for domains and IP addresses.",
    )
    parser.add_argument("target", help="Domain name or IP address to look up.")
    parser.add_argument("--whois-server", default="whois.iana.org",
                        help="WHOIS server to query (default: whois.iana.org).")
    parser.add_argument("--port", type=int, default=43,
                        help="WHOIS server port (default: 43).")
    parser.add_argument("--rdap", action="store_true",
                        help="Use RDAP (HTTP/JSON) instead of WHOIS.")
    parser.add_argument("--json", action="store_true",
                        help="Emit machine-readable JSON output.")
    parser.add_argument("--timeout", type=int, default=20,
                        help="Network timeout in seconds (default: 20).")
    args = parser.parse_args()

    if args.rdap:
        result = rdap_lookup(args.target, timeout=args.timeout)
        if args.json:
            print(json.dumps(result, indent=2))
        else:
            for key in ("query", "handle", "name", "status", "entities",
                        "events", "network", "autnum"):
                if key in result:
                    print(f"{key}: {json.dumps(result[key], indent=2) if not isinstance(result[key], str) else result[key]}")
            if "error" in result:
                print(f"error: {result['error']}", file=sys.stderr)
        return 0

    # WHOIS path: start at IANA, follow referral if needed
    text = whois_lookup(args.target, server=args.whois_server,
                        port=args.port, timeout=args.timeout)
    referral = whois_referral_server(text)
    if referral and referral != args.whois_server:
        text += f"\n--- Referral to {referral} ---\n"
        text += whois_lookup(args.target, server=referral,
                             port=args.port, timeout=args.timeout)

    if args.json:
        print(json.dumps({"target": args.target, "raw": text}, indent=2))
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
