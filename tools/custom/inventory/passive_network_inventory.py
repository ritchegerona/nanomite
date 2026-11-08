#!/usr/bin/env python3
"""
Passive Network Inventory Tool
------------------------------
Discovers active hosts on the local subnet by passively observing ARP traffic
and reading the system ARP cache.

Intended for defensive network inventory on networks you own or have explicit
written authorization to monitor. Does not perform active scanning, probing,
or exploitation.

Requirements:
  - root (for packet sniffing)
  - scapy (optional; enables live passive sniffing)
    pip install scapy

Usage:
  python3 passive_network_inventory.py [-i IFACE] [-t SECONDS] [-o OUTPUT.json] [--force]

Modes:
  - root:   live passive ARP sniffing + ARP cache snapshot
  - non-root: ARP cache snapshot only (no sniffing). Use --force to attempt
    sniffing anyway (scapy may fail without privileges).

Requirements:
  - scapy (optional; enables live passive sniffing when run as root)
    pip install scapy
"""

import argparse
import json
import os
from datetime import datetime, timezone
from pathlib import Path

try:
    from scapy.all import ARP, conf, sniff
    HAS_SCAPY = True
except Exception:  # noqa: BLE001 - scapy is optional
    HAS_SCAPY = False


def parse_arp_cache():
    """Read /proc/net/arp for known ARP entries (fallback when scapy is absent)."""
    entries = []
    try:
        with open("/proc/net/arp", "r") as f:
            next(f)  # skip header line
            for line in f:
                parts = line.split()
                if len(parts) >= 6:
                    ip = parts[0]
                    mac = parts[3]
                    iface = parts[5]
                    if mac and mac.lower() != "00:00:00:00:00:00":
                        entries.append({
                            "ip": ip,
                            "mac": mac.lower(),
                            "interface": iface,
                            "source": "arp_cache",
                            "timestamp": datetime.now(timezone.utc).isoformat(),
                        })
    except FileNotFoundError:
        pass
    return entries


def run_passive(iface=None, duration=None, output=None, require_root=True):
    inventory = {}
    stop_flag = {"stop": False}

    def add_entry(entry):
        key = entry["mac"]
        # Prefer live sniffed entries over stale cache entries
        if key not in inventory or entry["source"] == "arp_sniff":
            inventory[key] = entry

    is_root = os.geteuid() == 0
    can_sniff = is_root and HAS_SCAPY

    if require_root and not is_root:
        print("[*] Skipping live sniff (non-root mode). ARP cache snapshot only.\n")
    elif can_sniff:
        if iface:
            conf.iface = iface

        def handle_pkt(pkt):
            if pkt.haslayer(ARP):
                arp = pkt[ARP]
                ip = arp.psrc
                mac = arp.hwsrc.lower()
                if mac == "00:00:00:00:00:00":
                    return
                entry = {
                    "ip": ip,
                    "mac": mac,
                    "interface": iface or conf.iface,
                    "source": "arp_sniff",
                    "timestamp": datetime.now(timezone.utc).isoformat(),
                    "arp_op": int(arp.op),  # 1 = request, 2 = reply
                }
                add_entry(entry)

        def stop_filter(pkt):
            return stop_flag["stop"]

        print(f"[*] Passive ARP inventory on {conf.iface}. Press Ctrl+C to stop.")
        try:
            sniff(prn=handle_pkt, store=0, stop_filter=stop_filter, timeout=duration if duration else 0)
        except KeyboardInterrupt:
            stop_flag["stop"] = True
            print("\n[!] Sniffing interrupted by user.")
        except PermissionError:
            print("[!] Permission denied for sniffing. Falling back to ARP cache snapshot only.")
    elif HAS_SCAPY and not is_root:
        print("[!] scapy installed but not running as root. Skipping live sniff.")
    else:
        print("[!] scapy not installed. Falling back to ARP cache snapshot only.")
        print("    Install scapy for live passive sniffing: pip install scapy")

    # Merge ARP cache entries
    for entry in parse_arp_cache():
        add_entry(entry)

    # Print table
    print("\n=== Passive Network Inventory ===")
    print(f"{'IP':<18} {'MAC':<20} {'Interface':<12} {'Source':<14} {'First Seen'}")
    print("-" * 80)
    for entry in sorted(inventory.values(), key=lambda x: x["ip"]):
        print(f"{entry['ip']:<18} {entry['mac']:<20} {entry['interface']:<12} {entry['source']:<14} {entry['timestamp']}")

    print(f"\n[+] Total unique hosts observed: {len(inventory)}")

    if output:
        data = {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "interface": iface,
            "host_count": len(inventory),
            "hosts": list(inventory.values()),
        }
        path = Path(output)
        path.write_text(json.dumps(data, indent=2))
        print(f"[+] Results written to {path.resolve()}")

    return inventory


def main():
    parser = argparse.ArgumentParser(
        description="Passive network inventory via ARP observation (defensive use only)."
    )
    parser.add_argument("-i", "--interface", help="Network interface to listen on (default: system default)")
    parser.add_argument("-t", "--time", type=int, help="Seconds to sniff before exiting (default: run until Ctrl+C)")
    parser.add_argument("-o", "--output", help="Write JSON output to file")
    parser.add_argument("--force", action="store_true", help="Attempt sniffing even without root (scapy may fail)")
    args = parser.parse_args()

    is_root = os.geteuid() == 0

    if not is_root and not args.force:
        print("[!] Not running as root. Live packet sniffing requires root privileges.")
        print("    Continuing with ARP cache snapshot only (non-root mode).")
        print("    To attempt sniffing anyway, use: python3 passive_network_inventory.py --force\n")

    run_passive(iface=args.interface, duration=args.time, output=args.output, require_root=not args.force)


if __name__ == "__main__":
    main()