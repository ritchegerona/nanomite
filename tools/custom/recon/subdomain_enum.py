#!/usr/bin/env python3
"""Subdomain enumerator via DNS brute force with wildcard detection.

Resolves candidate labels against a domain and reports those that resolve.
A random label is probed first: if it also resolves, the zone is treated as
wildcard and only IPs outside the wildcard set are reported. For assets you
own or are explicitly authorized to enumerate.

Usage:
  python3 subdomain_enum.py --domain example.com
  python3 subdomain_enum.py --domain example.com --wordlist subs.txt --json
"""

import argparse
import concurrent.futures
import json
import random
import socket
import string
import sys
from datetime import datetime, timezone

DEFAULT_WORDLIST = [
    "www", "mail", "ftp", "localhost", "webmail", "smtp", "pop", "ns1",
    "ns2", "dns", "dns1", "dns2", "mx", "email", "blog", "dev", "staging",
    "stage", "test", "testing", "qa", "uat", "admin", "portal", "app",
    "api", "cdn", "static", "assets", "img", "images", "media", "files",
    "download", "downloads", "docs", "wiki", "shop", "store", "cart",
    "pay", "payment", "billing", "vpn", "remote", "git", "gitlab",
    "github", "ci", "jenkins", "jira", "confluence", "grafana", "monitor",
    "status", "db", "mysql", "sql", "redis", "mongo", "elastic", "kibana",
    "search", "intranet", "internal", "office", "owa", "autodiscover",
    "exchange", "sip", "voip", "crm", "erp", "hr", "helpdesk", "support",
    "help", "news", "forum", "community", "beta", "alpha", "demo",
    "sandbox", "lab", "backup", "old", "new", "v2", "app1", "app2",
    "web1", "web2", "node1", "node2", "server1", "server2", "proxy",
    "gw", "gateway", "fw", "firewall", "ntp", "ldap", "ad", "sso",
    "auth", "login", "id", "sso1", "mobile", "m", "wap", "iot",
]


def resolve(host, timeout):
    old = socket.getdefaulttimeout()
    socket.setdefaulttimeout(timeout)
    try:
        infos = socket.getaddrinfo(host, None)
        return sorted({info[4][0] for info in infos})
    except (socket.gaierror, OSError, socket.herror):
        return []
    finally:
        socket.setdefaulttimeout(old)


def wildcard_ips(domain, timeout):
    label = "wcr" + "".join(random.choices(string.ascii_lowercase + string.digits, k=12))
    return set(resolve(f"{label}.{domain}", timeout))


def load_wordlist(path):
    if not path:
        return DEFAULT_WORDLIST
    with open(path) as fh:
        return [line.strip() for line in fh if line.strip() and not line.startswith("#")]


def main():
    ap = argparse.ArgumentParser(description="DNS subdomain brute-force enumerator")
    ap.add_argument("--domain", required=True, help="target domain, e.g. example.com")
    ap.add_argument("--wordlist", help="file with one label per line (default: built-in)")
    ap.add_argument("--timeout", type=float, default=3.0, help="DNS timeout seconds")
    ap.add_argument("--workers", type=int, default=32, help="parallel lookups")
    ap.add_argument("--json", action="store_true", help="emit JSON")
    ap.add_argument("--show-wildcard", action="store_true",
                    help="include results that only match the wildcard set")
    args = ap.parse_args()

    domain = args.domain.strip().lower().rstrip(".")
    labels = load_wordlist(args.wordlist)
    if not args.json:
        print(f"[*] zone: {domain} — probing wildcard ...")

    wc = wildcard_ips(domain, args.timeout)
    if not args.json:
        print(f"[*] wildcard: {'yes (' + ', '.join(sorted(wc)) + ')' if wc else 'no'}")
        print(f"[*] trying {len(labels)} labels ...")

    found = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {
            pool.submit(resolve, f"{lbl}.{domain}", args.timeout): lbl for lbl in labels
        }
        for fut in concurrent.futures.as_completed(futures):
            lbl = futures[fut]
            ips = fut.result()
            if not ips:
                continue
            ip_set = set(ips)
            is_wc = bool(wc) and ip_set <= wc
            if is_wc and not args.show_wildcard:
                continue
            found.append({
                "host": f"{lbl}.{domain}",
                "ips": ips,
                "wildcard_only": is_wc,
            })

    found.sort(key=lambda r: r["host"])

    if args.json:
        print(json.dumps({
            "tool": "subdomain_enum",
            "domain": domain,
            "scanned_at": datetime.now(timezone.utc).isoformat(),
            "wildcard": sorted(wc),
            "found": found,
        }, indent=2))
    else:
        for r in found:
            tag = " (wildcard)" if r["wildcard_only"] else ""
            print(f"{r['host']:<40} {', '.join(r['ips'])}{tag}")
        print(f"[*] {len(found)} subdomain(s) resolved")
    return 0


if __name__ == "__main__":
    sys.exit(main())
