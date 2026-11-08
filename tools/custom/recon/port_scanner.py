#!/usr/bin/env python3
"""Concurrent TCP connect port scanner with optional banner grabbing.

Uses plain connect() scanning — no raw packets, no root required. For hosts
you own or are explicitly authorized to assess.

Usage:
  python3 port_scanner.py 192.168.1.0/24 --ports 22,80,443,8000-8100
  python3 port_scanner.py host.example.com --top --banners
  python3 port_scanner.py --target-file hosts.txt --ports 1-1024 --json
"""

import argparse
import concurrent.futures
import ipaddress
import json
import socket
import sys
from datetime import datetime, timezone

TOP_PORTS = [
    21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 443, 445,
    993, 995, 1433, 1521, 1723, 3306, 3389, 5432, 5900, 6379,
    8000, 8080, 8443, 8888, 9200, 11211, 27017,
]

# Probes that elicit a useful banner without side effects.
BANNER_PROBES = {
    80: b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n",
    8080: b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n",
    8000: b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n",
    8443: b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n",
    8888: b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n",
}


def parse_ports(spec):
    """Expand a port spec ('22,80,8000-8100', 'top', '1-1024') into a list."""
    if spec in ("top", "top100"):
        return sorted(set(TOP_PORTS))
    ports = set()
    for part in spec.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            lo, hi = part.split("-", 1)
            lo, hi = int(lo), int(hi)
            if lo > hi:
                lo, hi = hi, lo
            ports.update(range(max(1, lo), min(65535, hi) + 1))
        else:
            ports.add(int(part))
    return sorted(ports)


def expand_targets(target=None, target_file=None):
    """Resolve positional target / CIDR / target file into a host list."""
    hosts = []
    raw = []
    if target:
        raw.append(target.strip())
    if target_file:
        with open(target_file) as fh:
            raw.extend(line.strip() for line in fh if line.strip() and not line.startswith("#"))
    for item in raw:
        try:
            net = ipaddress.ip_network(item, strict=False)
            if net.num_addresses <= 1:
                hosts.append(str(net.network_address))
            else:
                hosts.extend(str(h) for h in net.hosts())
        except ValueError:
            hosts.append(item)
    # de-dup, preserve order
    seen = set()
    out = []
    for h in hosts:
        if h not in seen:
            seen.add(h)
            out.append(h)
    return out


def service_name(port):
    try:
        return socket.getservbyport(port, "tcp")
    except OSError:
        return "?"


def grab_banner(sock, port):
    probe = BANNER_PROBES.get(port, b"")
    try:
        if probe:
            sock.sendall(probe)
        data = sock.recv(256)
        return data.decode("utf-8", "replace").strip().replace("\r", " ").replace("\n", " ")[:200]
    except (OSError, socket.timeout):
        return ""


def scan_host(host, ports, timeout, banners):
    results = []
    for port in ports:
        try:
            with socket.create_connection((host, port), timeout=timeout) as sock:
                banner = grab_banner(sock, port) if banners else ""
                results.append({
                    "host": host,
                    "port": port,
                    "state": "open",
                    "service": service_name(port),
                    "banner": banner,
                })
        except (OSError, socket.timeout):
            continue
    return results


def main():
    ap = argparse.ArgumentParser(description="TCP connect port scanner")
    ap.add_argument("target", nargs="?", help="host, IP, or CIDR")
    ap.add_argument("--target-file", help="file with one host/CIDR per line")
    ap.add_argument("--ports", default="1-1024", help="port list/ranges, or 'top'")
    ap.add_argument("--timeout", type=float, default=1.0, help="per-socket timeout seconds")
    ap.add_argument("--workers", type=int, default=256, help="parallel sockets")
    ap.add_argument("--banners", action="store_true", help="grab short banners from open ports")
    ap.add_argument("--json", action="store_true", help="emit JSON")
    args = ap.parse_args()

    if not args.target and not args.target_file:
        ap.error("provide a target or --target-file")

    try:
        ports = parse_ports("top" if args.ports == "top" else args.ports)
    except ValueError:
        ap.error("bad --ports spec")
    hosts = expand_targets(args.target, args.target_file)
    if not hosts:
        print("[!] no targets", file=sys.stderr)
        return 2

    if not args.json:
        print(f"[*] scanning {len(hosts)} host(s) x {len(ports)} port(s) ...")

    open_ports = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(scan_host, h, ports, args.timeout, args.banners) for h in hosts]
        for fut in concurrent.futures.as_completed(futures):
            open_ports.extend(fut.result())

    open_ports.sort(key=lambda r: (r["host"], r["port"]))

    if args.json:
        print(json.dumps({
            "tool": "port_scanner",
            "scanned_at": datetime.now(timezone.utc).isoformat(),
            "hosts": len(hosts),
            "ports_per_host": len(ports),
            "open": open_ports,
        }, indent=2))
    else:
        for r in open_ports:
            line = f"{r['host']}:{r['port']:<5} open  {r['service']}"
            if r["banner"]:
                line += f"  | {r['banner']}"
            print(line)
        print(f"[*] {len(open_ports)} open port(s) on {len(hosts)} host(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
